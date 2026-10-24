import { createHash } from "node:crypto";
import {
  closeSync,
  constants,
  fstatSync,
  lstatSync,
  openSync,
  readFileSync,
  realpathSync,
} from "node:fs";
import path from "node:path";

import { listProjectFiles } from "../../../tools/gates/lib/analysis.mjs";
import { pipelineForFile } from "./edited-file-pipeline.js";

function missing(error) {
  return error.code === "ENOENT" || error.code === "ENOTDIR";
}

function realRoot(cwd) {
  try {
    return realpathSync(cwd);
  } catch (error) {
    if (!missing(error)) {
      throw error;
    }
    return null;
  }
}

function regularFile(root, file) {
  const absolute = path.resolve(root, file);
  const relative = path.relative(root, absolute);
  if (
    !relative ||
    relative === ".." ||
    relative.startsWith(`..${path.sep}`) ||
    path.isAbsolute(relative)
  ) {
    return null;
  }
  const parts = relative.split(path.sep);
  let current = root;
  for (let index = 0; index < parts.length; index += 1) {
    current = path.join(current, parts[index]);
    const stat = lstatSync(current);
    const last = index === parts.length - 1;
    if ((last && !stat.isFile()) || (!last && !stat.isDirectory())) {
      return null;
    }
  }
  return absolute;
}

function digest(bytes) {
  return createHash("sha256").update(bytes).digest("hex");
}

function fileDigest(root, file) {
  let descriptor = null;
  try {
    const absolute = regularFile(root, file);
    if (!absolute) {
      return null;
    }
    descriptor = openSync(
      absolute,
      constants.O_RDONLY + constants.O_NOFOLLOW + constants.O_NONBLOCK,
    );
    if (fstatSync(descriptor).isFile()) {
      return [absolute, digest(readFileSync(descriptor))];
    }
  } catch (error) {
    if (!missing(error)) {
      throw error;
    }
  } finally {
    if (descriptor !== null) {
      closeSync(descriptor);
    }
  }
  return null;
}

function acceptFile(root, baseline, { file, bytes }) {
  try {
    const absolute = regularFile(root, file);
    if (absolute && pipelineForFile(absolute, root)) {
      // Accept only the bytes observed before checks, never a fresh scan.
      baseline.set(absolute, digest(bytes));
    }
  } catch (error) {
    if (!missing(error)) {
      throw error;
    }
  }
}

function scan(root) {
  const files = listProjectFiles(root);
  const snapshot = new Map();
  for (const file of files) {
    if (pipelineForFile(file, root)) {
      const entry = fileDigest(root, file);
      if (entry) {
        snapshot.set(...entry);
      }
    }
  }
  return snapshot;
}

export function createExecutionMutationTracker() {
  const baselines = new Map();
  return {
    capture(cwd) {
      const root = realpathSync(cwd);
      if (baselines.has(root)) {
        return;
      }
      baselines.set(root, scan(root));
    },
    changes(cwd) {
      const root = realRoot(cwd);
      const previous = baselines.get(root);
      if (!previous) {
        return [];
      }
      const snapshot = scan(root);
      const changed = [];
      for (const [file, hash] of snapshot) {
        if (previous.get(file) !== hash) {
          changed.push(file);
        }
      }
      baselines.set(root, snapshot);
      return changed;
    },
    accept(cwd, file, bytes) {
      const root = realRoot(cwd);
      const baseline = baselines.get(root);
      if (!baseline) {
        return;
      }
      acceptFile(root, baseline, { file, bytes });
    },
  };
}
