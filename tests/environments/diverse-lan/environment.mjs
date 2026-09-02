import { createHash, randomBytes, randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import {
  chmodSync,
  existsSync,
  mkdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { output, run } from "../../../tools/gates/lib/process.mjs";
import { sha256FromOutput } from "./integrity.mjs";
import { prepareNearShareSource } from "./nearshare-source.mjs";
import { assertPreparedImages } from "./prepared-images.mjs";
import { assertProcessSuccess } from "./process-evidence.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const STATE = join(CACHE, "diverse-lan", "running.json");
const CASE_ROOT = join(CACHE, "diverse-lan", "cases");
const COMPOSE = join(DIRECTORY, "compose.yaml");
const DRIVER = join(DIRECTORY, "nearshare_driver.py");
const NEARSHARE_SOURCE = join(CACHE, "sources", "trees", "nearshare");
const GOOGLE_MANIFEST = join(
  ROOT,
  "tests/environments/nearby-linux/environment.json",
);
const NEARSHARE_MANIFEST = join(
  ROOT,
  "tests/environments/nearshare/environment.json",
);
const LIFECYCLE_GOAL_MS = 30_000;
const TRANSFER_TIMEOUT_SECONDS = 18;
const TEST_LIMIT_MS = 60_000;
const PIN_SALT_BYTES = 32;
const CASE_DIRECTORY_MODE = 0o777;
const RECEIVER_START_DELAY_MS = 500;
const PIN_FINGERPRINT_PATTERN = /fingerprint[=":]+(?<fingerprint>[a-f0-9]+)/gu;
const XDG = {
  XDG_CONFIG_HOME: "/run/quickshare/config",
  XDG_DOWNLOAD_DIR: "/cases/received",
  XDG_RUNTIME_DIR: "/run/quickshare",
  XDG_STATE_HOME: "/run/quickshare/state",
};

function manifests() {
  return {
    google: JSON.parse(readFileSync(GOOGLE_MANIFEST, "utf8")),
    nearshare: JSON.parse(readFileSync(NEARSHARE_MANIFEST, "utf8")),
  };
}

function fingerprint(value) {
  return createHash("sha256").update(value).digest("hex");
}

function environment(directories = {}) {
  const images = manifests();
  let nearShareSource = NEARSHARE_SOURCE;
  if (directories.nearshare) {
    nearShareSource = join(directories.nearshare, "source");
  }
  return {
    ...process.env,
    GOOGLE_CASE_DIR: directories.google ?? "",
    NEARBY_LINUX_IMAGE: images.google.image,
    NEARSHARE_CASE_DIR: directories.nearshare ?? "",
    NEARSHARE_DRIVER: DRIVER,
    NEARSHARE_IMAGE: images.nearshare.image,
    NEARSHARE_SOURCE: nearShareSource,
  };
}

function compose(arguments_, directories) {
  return run(
    process.env.DOCKER ?? "docker",
    ["compose", "--file", COMPOSE, ...arguments_],
    {
      env: environment(directories),
    },
  );
}

function assertPrepared() {
  const images = manifests();
  assertPreparedImages({
    googleImage: images.google.image,
    nearShareImage: images.nearshare.image,
    nearShareManifest: readFileSync(NEARSHARE_MANIFEST, "utf8"),
    root: ROOT,
  });
  if (!existsSync(NEARSHARE_SOURCE)) {
    throw new Error("NearShare source is missing; run make sources-fetch");
  }
}

function caseDirectories() {
  const root = join(CASE_ROOT, `${Date.now()}-${randomUUID()}`);
  const directories = {
    google: join(root, "google"),
    nearshare: join(root, "nearshare"),
    root,
    salt: randomBytes(PIN_SALT_BYTES).toString("hex"),
  };
  for (const directory of [directories.google, directories.nearshare]) {
    for (const name of ["outbound", "received"]) {
      const path = join(directory, name);
      mkdirSync(path, { recursive: true });
      chmodSync(path, CASE_DIRECTORY_MODE);
    }
  }
  return directories;
}

function running() {
  if (!existsSync(STATE)) {
    throw new Error("Diverse LAN is not running; run make diverse-lan-up");
  }
  return JSON.parse(readFileSync(STATE, "utf8"));
}

function report(action, started) {
  const elapsed = Date.now() - started;
  let result = "missed";
  if (elapsed <= LIFECYCLE_GOAL_MS) {
    result = "met";
  }
  process.stdout.write(
    `Diverse LAN ${action} in ${elapsed}ms; goal ${result}.\n`,
  );
}

function prepareNearShare(directories) {
  compose(
    [
      "exec",
      "--tty=false",
      "nearshare",
      "sh",
      "-ceu",
      "cp -a /source/. /work/nearshare/; mkdir -p /work/home /work/runtime; " +
        "chmod 700 /work/runtime; " +
        "bash /work/nearshare/tools/genproto.sh python3 >/dev/null",
    ],
    directories,
  );
}

function up() {
  const started = Date.now();
  assertPrepared();
  const directories = caseDirectories();
  try {
    prepareNearShareSource(
      NEARSHARE_SOURCE,
      join(directories.nearshare, "source"),
    );
    compose(["up", "--detach", "--wait", "--wait-timeout", "30"], directories);
    prepareNearShare(directories);
    mkdirSync(dirname(STATE), { recursive: true });
    writeFileSync(STATE, `${JSON.stringify(directories)}\n`);
  } catch (error) {
    compose(["down", "--remove-orphans", "--volumes"], directories);
    rmSync(directories.root, { force: true, recursive: true });
    throw error;
  }
  report("startup ready", started);
}

function down() {
  const started = Date.now();
  let directories = caseDirectories();
  if (existsSync(STATE)) {
    directories = running();
  }
  compose(["down", "--remove-orphans", "--volumes"], directories);
  rmSync(STATE, { force: true });
  if (dirname(resolve(directories.root)) === resolve(CASE_ROOT)) {
    rmSync(directories.root, { force: true, recursive: true });
  }
  report("teardown ready", started);
}

function child(arguments_, directories) {
  const command = process.env.DOCKER ?? "docker";
  const result = spawn(command, ["compose", "--file", COMPOSE, ...arguments_], {
    env: environment(directories),
  });
  let logs = "";
  result.stdout.on("data", (chunk) => {
    logs += chunk;
  });
  result.stderr.on("data", (chunk) => {
    logs += chunk;
  });
  return {
    logs: () => logs,
    wait: () =>
      new Promise((resolve_, reject) => {
        result.once("error", reject);
        result.once("close", (code) => resolve_({ code }));
      }),
  };
}

function googleCommand({ file, name, role, salt }) {
  const argumentsList = ["exec", "--tty=false"];
  for (const [key, value] of Object.entries(XDG)) {
    argumentsList.push("--env", `${key}=${value}`);
  }
  argumentsList.push("--env", `QUICKSHARE_PIN_SALT=${salt}`);
  argumentsList.push(
    "google",
    "runuser",
    "--user",
    "quickshare",
    "--",
    "/usr/local/bin/nearby_sharing_cli",
    role,
  );
  if (role === "send") {
    argumentsList.push(`/cases/outbound/${file}`);
  } else {
    argumentsList.push("--action", "accept");
  }
  argumentsList.push(
    "--name",
    name,
    "--timeout",
    `${TRANSFER_TIMEOUT_SECONDS}`,
  );
  return argumentsList;
}

function nearShareCommand({ file, role, salt }) {
  const argumentsList = [
    "exec",
    "--tty=false",
    "--env",
    `QUICKSHARE_PIN_SALT=${salt}`,
    "nearshare",
    "python3",
    "/driver/nearshare_driver.py",
    role,
  ];
  argumentsList.push(
    "--name",
    "NearShare-A",
    "--timeout",
    `${TRANSFER_TIMEOUT_SECONDS}`,
  );
  if (role === "send") {
    argumentsList.push(
      "--file",
      `/cases/outbound/${file}`,
      "--target",
      "Google-B",
    );
  } else {
    argumentsList.push("--received", "/cases/received");
  }
  return argumentsList;
}

function delay(milliseconds) {
  return new Promise((resolve_) => {
    setTimeout(resolve_, milliseconds);
  });
}

function peerFileFingerprint(directories, peer, path) {
  const result = output(
    process.env.DOCKER ?? "docker",
    [
      "compose",
      "--file",
      COMPOSE,
      "exec",
      "--tty=false",
      peer,
      "sha256sum",
      "--binary",
      path,
    ],
    { env: environment(directories) },
  );
  return sha256FromOutput(result, peer);
}

function directionDetails(directories, direction) {
  const googleSends = direction === "google-to-nearshare";
  if (googleSends) {
    return {
      receiverDirectory: directories.nearshare,
      receiverPeer: "nearshare",
      senderDirectory: directories.google,
    };
  }
  return {
    receiverDirectory: directories.google,
    receiverPeer: "google",
    senderDirectory: directories.nearshare,
  };
}

function directionFile(direction, repeated) {
  let suffix = "";
  if (repeated) {
    suffix = "-repeat";
  }
  return `${direction}${suffix}.txt`;
}

function receiverCommand(googleSends, file, salt) {
  if (googleSends) {
    return nearShareCommand({ file, role: "receive", salt });
  }
  return googleCommand({ file, name: "Google-B", role: "receive", salt });
}

function senderCommand(googleSends, file, salt) {
  if (googleSends) {
    return googleCommand({ file, name: "Google-A", role: "send", salt });
  }
  return nearShareCommand({ file, role: "send", salt });
}

function assertTransferEvidence(direction, logs, received) {
  const pins = [...logs.matchAll(PIN_FINGERPRINT_PATTERN)].map(
    (match) => match.groups.fingerprint,
  );
  if (!existsSync(received) || pins.length < 2 || new Set(pins).size !== 1) {
    throw new Error(
      `diverse LAN ${direction} lacks PIN or payload evidence ` +
        `(file ${existsSync(received)}, fingerprints ${pins.length})`,
    );
  }
  if (
    !logs.includes("discovered") ||
    !logs.includes("kComplete") ||
    !logs.includes("complete")
  ) {
    throw new Error(
      `diverse LAN ${direction} lacks discovery or completion evidence`,
    );
  }
}

function assertMatchingHash({
  directories,
  direction,
  file,
  receiverPeer,
  sent,
}) {
  const receivedFingerprint = peerFileFingerprint(
    directories,
    receiverPeer,
    `/cases/received/${file}`,
  );
  if (fingerprint(readFileSync(sent)) !== receivedFingerprint) {
    throw new Error(`diverse LAN ${direction} SHA-256 mismatch`);
  }
}

async function runDirection(directories, direction, repeated = false) {
  const file = directionFile(direction, repeated);
  const googleSends = direction === "google-to-nearshare";
  const details = directionDetails(directories, direction);
  writeFileSync(
    join(details.senderDirectory, "outbound", file),
    `diverse LAN ${file}\n`,
  );
  const receiver = child(
    receiverCommand(googleSends, file, directories.salt),
    directories,
  );
  await delay(RECEIVER_START_DELAY_MS);
  const sender = child(
    senderCommand(googleSends, file, directories.salt),
    directories,
  );
  const results = await Promise.all([sender.wait(), receiver.wait()]);
  assertProcessSuccess({ direction, receiver, results, sender });
  const received = join(details.receiverDirectory, "received", file);
  const logs = `${sender.logs()}\n${receiver.logs()}`;
  assertTransferEvidence(direction, logs, received);
  const sent = join(details.senderDirectory, "outbound", file);
  assertMatchingHash({
    direction,
    directories,
    file,
    receiverPeer: details.receiverPeer,
    sent,
  });
  return {
    bytes: readFileSync(sent).byteLength,
    direction,
    pinMatch: true,
    repeated,
  };
}

async function selfTest() {
  const started = Date.now();
  const directories = running();
  const initialGoogleToNearshare = await runDirection(
    directories,
    "google-to-nearshare",
  );
  const nearshareToGoogle = await runDirection(
    directories,
    "nearshare-to-google",
  );
  const repeatedGoogleToNearshare = await runDirection(
    directories,
    "google-to-nearshare",
    true,
  );
  const evidence = [
    initialGoogleToNearshare,
    nearshareToGoogle,
    repeatedGoogleToNearshare,
  ];
  if (Date.now() - started > TEST_LIMIT_MS) {
    throw new Error("Diverse LAN self-test exceeded its time limit");
  }
  process.stdout.write(`${JSON.stringify({ evidence, schema: 1 })}\n`);
}

function validate() {
  const composeSource = readFileSync(COMPOSE, "utf8");
  for (const value of [
    "internal: true",
    "172.30.45.0/24",
    "nearshare",
    "google",
  ]) {
    if (!composeSource.includes(value)) {
      throw new Error(`Diverse LAN Compose lacks ${value}`);
    }
  }
  if (!readFileSync(DRIVER, "utf8").includes('event("pin"')) {
    throw new Error("Diverse LAN driver lacks PIN evidence");
  }
  process.stdout.write("Validated isolated diverse LAN inputs.\n");
}

const handlers = { down, "self-test": selfTest, up, validate };

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const handler = handlers[process.argv[2] ?? "validate"];
  if (!handler) {
    throw new Error("unknown diverse LAN command");
  }
  await handler();
}

export { caseDirectories, fingerprint, runDirection, validate };
