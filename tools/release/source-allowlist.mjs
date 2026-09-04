import {
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  readFileSync,
  readdirSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
export const ALLOWLIST_FILE = join(
  ROOT,
  "tools",
  "release",
  "source-allowlist.json",
);
const DENY_NAMES = new Set([
  ".cache",
  "fuzz",
  "node_modules",
  "reports",
  "target",
  "tests",
  "tools",
  "upstream",
]);
const APP_VERSION_PATTERN = /^version = "(?<version>[^"]+)"/mu;
const DEFAULT_MEMBERS_PATTERN = /^default-members = \[[^\]]*\]/mu;
const MEMBERS_PATTERN = /^members = \[[^\]]*\]/mu;

export function loadAllowlist() {
  const allowlist = JSON.parse(readFileSync(ALLOWLIST_FILE, "utf8"));
  if (allowlist.schemaVersion !== 1) {
    throw new Error(
      `unsupported source allowlist schema ${allowlist.schemaVersion}`,
    );
  }
  return allowlist;
}

export function sparseCheckoutPatterns(allowlist = loadAllowlist()) {
  return [...allowlist.files, ...allowlist.trees].sort();
}

export function readAppVersion(root = ROOT) {
  const manifest = readFileSync(
    join(root, "crates", "app", "Cargo.toml"),
    "utf8",
  );
  const match = manifest.match(APP_VERSION_PATTERN);
  if (!match) {
    throw new Error("app package version is missing");
  }
  return match.groups.version;
}

function denied(relativePath) {
  return relativePath.split("/").some((segment) => DENY_NAMES.has(segment));
}

function walk(root, relativePath, paths) {
  const full = join(root, relativePath);
  const metadata = lstatSync(full);
  if (metadata.isSymbolicLink()) {
    throw new Error(`source allowlist rejects symlink: ${relativePath}`);
  }
  if (metadata.isDirectory()) {
    for (const entry of readdirSync(full).sort()) {
      const child = `${relativePath}/${entry}`;
      if (!denied(child)) {
        walk(root, child, paths);
      }
    }
    return;
  }
  if (!metadata.isFile()) {
    throw new Error(`source allowlist rejects special file: ${relativePath}`);
  }
  paths.push(relativePath);
}

function requirePath(root, relativePath, label) {
  if (!existsSync(join(root, relativePath))) {
    throw new Error(`missing allowlisted ${label}: ${relativePath}`);
  }
  if (denied(relativePath)) {
    throw new Error(`allowlist ${label} is denied: ${relativePath}`);
  }
}

export function collectAllowlistedPaths(root, allowlist = loadAllowlist()) {
  const paths = [];
  for (const file of allowlist.files) {
    requirePath(root, file, "file");
    walk(root, file, paths);
  }
  for (const tree of allowlist.trees) {
    requirePath(root, tree, "tree");
    walk(root, tree, paths);
  }
  return [...new Set(paths)].sort();
}

function listFiles(root, relativePath, paths) {
  let directory = root;
  if (relativePath) {
    directory = join(root, relativePath);
  }
  for (const entry of readdirSync(directory).sort()) {
    if (entry !== ".git") {
      let child = entry;
      if (relativePath) {
        child = `${relativePath}/${entry}`;
      }
      if (lstatSync(join(root, child)).isDirectory()) {
        listFiles(root, child, paths);
      } else {
        paths.push(child);
      }
    }
  }
}

export function assertClosedTree(root) {
  const paths = [];
  listFiles(root, "", paths);
  const leaked = paths.filter((path) => denied(path));
  if (leaked.length > 0) {
    throw new Error(`source-build leaked paths: ${leaked.join(", ")}`);
  }
  return paths.sort();
}

function rewriteWorkspaceMembers(toml, trees) {
  const list = trees.map((tree) => `  "${tree}",`).join("\n");
  const members = `members = [\n${list}\n]`;
  const defaults = `default-members = [\n${list}\n]`;
  let next = toml;
  if (DEFAULT_MEMBERS_PATTERN.test(toml)) {
    next = next.replace(DEFAULT_MEMBERS_PATTERN, defaults);
  }
  if (MEMBERS_PATTERN.test(next)) {
    next = next.replace(MEMBERS_PATTERN, members);
  }
  return next;
}

export function rewriteRuntimeWorkspace(root, allowlist = loadAllowlist()) {
  const path = join(root, "Cargo.toml");
  writeFileSync(
    path,
    rewriteWorkspaceMembers(readFileSync(path, "utf8"), allowlist.trees),
  );
}

export function copyAllowlistedSources(
  root,
  destination,
  allowlist = loadAllowlist(),
) {
  const paths = collectAllowlistedPaths(root, allowlist);
  for (const path of paths) {
    const to = join(destination, path);
    mkdirSync(dirname(to), { recursive: true });
    copyFileSync(join(root, path), to);
  }
  rewriteRuntimeWorkspace(destination, allowlist);
  return paths;
}
