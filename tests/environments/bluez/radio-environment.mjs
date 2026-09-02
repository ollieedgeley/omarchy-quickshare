import { createHash } from "node:crypto";
import { spawnSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { guestTransaction, runBluetoothRadioSelfTest } from "./radio-test.mjs";

const DIRECTORY = dirname(fileURLToPath(import.meta.url));
const ROOT = resolve(DIRECTORY, "../../..");
const CACHE = process.env.TEST_ENV_CACHE ?? join(ROOT, ".cache", "test-env");
const SOURCE_ROOT = join(CACHE, "sources", "trees");
const RADIO_CACHE = join(CACHE, "bluetooth-radio");
const BLUEZ_WORKSPACE = join(RADIO_CACHE, "bluez");
const ARTIFACTS = join(RADIO_CACHE, "artifacts");
const BTVIRT = join(ARTIFACTS, "btvirt");
const RUNTIME = join(RADIO_CACHE, "runtime");
const CONTROL = join(RUNTIME, "control.sock");
const TRACE = join(RUNTIME, "radio.btsnoop");
const REPORTS = join(ROOT, "reports", "failures");
const MANIFEST_PATH = join(DIRECTORY, "radio-environment.json");
const DOCKERFILE_PATH = join(DIRECTORY, "Dockerfile.radio");
const STOP_TIMEOUT_MS = 10_000;
const LIFECYCLE_TIMEOUT_MS = 60_000;
const BASE_IMAGE_PATTERN = /^debian@sha256:[0-9a-f]{64}$/u;
const SNAPSHOT_PATTERN = /^\d{8}T\d{6}Z$/u;
const REVISION_PATTERN = /^[0-9a-f]{40}$/u;
const GUEST_KERNEL_ARGUMENTS =
  "console=ttyS0,115200 earlyprintk=serial root=oqs-root " +
  "rootfstype=9p rootflags=trans=virtio,version=9p2000.u ro " +
  "init=/environment/radio-guest-init.sh";
const QEMU_ROOT_SECURITY = "security_model=none,multidevs=remap";

function runOptions(options) {
  const result = { encoding: "utf8", stdio: "inherit" };
  if (options.capture) {
    result.stdio = "pipe";
  }
  return result;
}

function run(command, args, options = {}) {
  if (!options.quiet) {
    process.stdout.write(`+ ${command} ${args.join(" ")}\n`);
  }
  const result = spawnSync(command, args, runOptions(options));
  if (result.error) {
    throw result.error;
  }
  if (result.status !== 0 && !options.allowFailure) {
    let detail = "";
    if (options.capture) {
      detail = `\n${result.stdout ?? ""}${result.stderr ?? ""}`;
    }
    throw new Error(`${command} exited with ${result.status}${detail}`);
  }
  return result;
}

function containerExists(name) {
  return (
    run("docker", ["container", "inspect", name], {
      allowFailure: true,
      capture: true,
      quiet: true,
    }).status === 0
  );
}

function assertPrepared(manifest, fingerprint) {
  const image = run("docker", ["image", "inspect", manifest.image], {
    allowFailure: true,
    capture: true,
    quiet: true,
  });
  if (image.status !== 0) {
    throw new Error(
      "Bluetooth radio image is missing; run bluetooth-radio-provision",
    );
  }
  const label = run(
    "docker",
    [
      "image",
      "inspect",
      "--format",
      '{{index .Config.Labels "org.omarchy-quickshare.fingerprint"}}',
      manifest.image,
    ],
    { capture: true, quiet: true },
  ).stdout.trim();
  if (label !== fingerprint) {
    throw new Error(
      "Bluetooth radio inputs changed; rerun bluetooth-radio-provision",
    );
  }
}

export function validateRadioEnvironment(manifestSource, dockerfile) {
  const manifest = JSON.parse(manifestSource);
  if (manifest.schema !== 1) {
    throw new Error("unsupported Bluetooth radio schema");
  }
  if (!BASE_IMAGE_PATTERN.test(manifest.base)) {
    throw new Error("Bluetooth radio base must use a SHA-256 digest");
  }
  if (!SNAPSHOT_PATTERN.test(manifest.debianSnapshot)) {
    throw new Error("Bluetooth radio Debian snapshot must be timestamped");
  }
  for (const [name, revision] of Object.entries(manifest.sources)) {
    if (!REVISION_PATTERN.test(revision)) {
      throw new Error(`${name} revision must be a full commit`);
    }
  }
  for (const value of [
    manifest.base,
    manifest.debianSnapshot,
    ...manifest.packages,
    "ENVIRONMENT_FINGERPRINT",
  ]) {
    if (!dockerfile.includes(value)) {
      throw new Error(`Bluetooth radio Dockerfile lacks ${value}`);
    }
  }
  return manifest;
}

export function radioEnvironmentFingerprint(manifestSource, dockerfile) {
  return createHash("sha256")
    .update(manifestSource)
    .update("\0")
    .update(dockerfile)
    .digest("hex");
}

function inputs() {
  const manifestSource = readFileSync(MANIFEST_PATH, "utf8");
  const dockerfile = readFileSync(DOCKERFILE_PATH, "utf8");
  const manifest = validateRadioEnvironment(manifestSource, dockerfile);
  for (const name of Object.keys(manifest.sources)) {
    if (!existsSync(join(SOURCE_ROOT, name))) {
      throw new Error(
        `pinned ${name} source is missing; run make sources-fetch`,
      );
    }
  }
  const typingProject = readFileSync(
    join(SOURCE_ROOT, "typing-extensions", "pyproject.toml"),
    "utf8",
  );
  if (
    !typingProject.includes(`version = "${manifest.versions.typingExtensions}"`)
  ) {
    throw new Error(
      "typing-extensions source version differs from the manifest",
    );
  }
  return {
    manifest,
    manifestSource,
    dockerfile,
  };
}

function buildBtvirt(manifest) {
  rmSync(BLUEZ_WORKSPACE, { recursive: true, force: true });
  mkdirSync(ARTIFACTS, { recursive: true });
  cpSync(join(SOURCE_ROOT, "bluez"), BLUEZ_WORKSPACE, { recursive: true });
  run("docker", [
    "run",
    "--rm",
    "--network=none",
    "--user",
    "1000:1000",
    "--volume",
    `${BLUEZ_WORKSPACE}:/source`,
    "--volume",
    `${ARTIFACTS}:/artifacts`,
    "--workdir",
    "/source",
    manifest.image,
    "sh",
    "-ceu",
    [
      "./bootstrap",
      "./configure --disable-systemd --disable-udev --disable-cups " +
        "--disable-manpages --disable-datafiles --disable-client " +
        "--disable-obex --disable-mesh --disable-admin " +
        "--disable-monitor --enable-tools --enable-testing",
      "make -j2 emulator/btvirt",
      "cp emulator/btvirt /artifacts/btvirt",
    ].join(" && "),
  ]);
}

function verifyRadioTools(manifest) {
  run("docker", [
    "run",
    "--rm",
    "--network=none",
    "--volume",
    `${join(SOURCE_ROOT, "bumble")}:/bumble:ro`,
    "--volume",
    `${join(SOURCE_ROOT, "typing-extensions")}:/typing-extensions:ro`,
    "--volume",
    `${BTVIRT}:/usr/local/bin/btvirt:ro`,
    "--env",
    "PYTHONPATH=/typing-extensions/src:/bumble",
    manifest.image,
    "sh",
    "-ceu",
    [
      `test "$(bluetoothd -v)" = '${manifest.versions.bluetoothd}'`,
      `test "$(btvirt --version)" = '${manifest.versions.btvirt}'`,
      `test "$(dpkg-query -W -f='\${Version}' linux-image-amd64)" = ` +
        `'${manifest.versions.kernelPackage}'`,
      `test "$(python3 --version)" = 'Python ${manifest.versions.python}'`,
      'test "$(qemu-system-x86_64 --version | head -n 1 | ' +
        `awk '{print $4}')" = '${manifest.versions.qemu}'`,
      "lsinitramfs /boot/initrd-oqs | grep -q '/9pnet_virtio.ko'",
      "python3 -c 'from typing_extensions import TypeIs'",
      "python3 -c 'from bumble.controller import Controller; " +
        "from bumble.device import Device'",
    ].join(" && "),
  ]);
}

function provision() {
  const { manifest, manifestSource, dockerfile } = inputs();
  run("docker", [
    "build",
    "--file",
    DOCKERFILE_PATH,
    "--build-arg",
    `DEBIAN_SNAPSHOT=${manifest.debianSnapshot}`,
    "--build-arg",
    `ENVIRONMENT_FINGERPRINT=${radioEnvironmentFingerprint(
      manifestSource,
      dockerfile,
    )}`,
    "--tag",
    manifest.image,
    DIRECTORY,
  ]);
  assertPrepared(
    manifest,
    radioEnvironmentFingerprint(manifestSource, dockerfile),
  );
  buildBtvirt(manifest);
  if (!existsSync(BTVIRT)) {
    throw new Error("btvirt build did not produce a binary");
  }
  verifyRadioTools(manifest);
  process.stdout.write("Prepared pinned Bluetooth radio environment.\n");
}

function cleanup(manifest) {
  if (containerExists(manifest.container)) {
    run("docker", ["container", "rm", "--force", manifest.container]);
  }
  rmSync(RUNTIME, { recursive: true, force: true });
}

function qemuMachineArguments() {
  return [
    "qemu-system-x86_64",
    "-machine",
    "q35,accel=kvm",
    "-cpu",
    "host",
    "-m",
    "512M",
    "-smp",
    "2",
    "-nodefaults",
    "-no-reboot",
    "-display",
    "none",
    "-monitor",
    "none",
    "-kernel",
    "/boot/vmlinuz-oqs",
    "-initrd",
    "/boot/initrd-oqs",
    "-append",
    GUEST_KERNEL_ARGUMENTS,
  ];
}

function qemuDeviceArguments() {
  return [
    "-fsdev",
    `local,id=root,path=/,readonly=on,${QEMU_ROOT_SECURITY}`,
    "-device",
    "virtio-9p-pci,fsdev=root,mount_tag=oqs-root",
    "-device",
    "virtio-serial-pci",
    "-serial",
    "file:/runtime/console.log",
    "-chardev",
    "socket,id=control,path=/runtime/control.sock,server=on,wait=off",
    "-device",
    "virtserialport,chardev=control,name=oqs.control",
  ];
}

function startRadioContainer(manifest) {
  run("docker", [
    "run",
    "--detach",
    "--name",
    manifest.container,
    "--network=none",
    "--device=/dev/kvm",
    "--user",
    "1000:1000",
    "--read-only",
    "--tmpfs",
    "/tmp:rw,nosuid,nodev,size=64m",
    "--volume",
    `${DIRECTORY}:/environment:ro`,
    "--volume",
    `${join(SOURCE_ROOT, "bumble")}:/bumble:ro`,
    "--volume",
    `${join(SOURCE_ROOT, "typing-extensions")}:/typing-extensions:ro`,
    "--volume",
    `${ARTIFACTS}:/artifacts:ro`,
    "--volume",
    `${RUNTIME}:/runtime`,
    manifest.image,
    ...qemuMachineArguments(),
    ...qemuDeviceArguments(),
  ]);
}

async function up() {
  const started = performance.now();
  const { manifest, manifestSource, dockerfile } = inputs();
  assertPrepared(
    manifest,
    radioEnvironmentFingerprint(manifestSource, dockerfile),
  );
  if (!existsSync(BTVIRT)) {
    throw new Error("run bluetooth-radio-provision first");
  }
  if (containerExists(manifest.container)) {
    throw new Error("Bluetooth radio environment is already running");
  }
  rmSync(RUNTIME, { recursive: true, force: true });
  mkdirSync(RUNTIME, { recursive: true });
  try {
    startRadioContainer(manifest);
    await guestTransaction({ control: CONTROL, expected: "READY\n" });
  } catch {
    cleanup(manifest);
    throw new Error("Bluetooth radio startup failed");
  }
  const elapsed = performance.now() - started;
  if (elapsed > LIFECYCLE_TIMEOUT_MS) {
    throw new Error(`Bluetooth radio startup took ${elapsed}ms`);
  }
  process.stdout.write(
    `Bluetooth radio environment ready in ${Math.round(elapsed)}ms.\n`,
  );
}

async function down() {
  const started = performance.now();
  const { manifest } = inputs();
  if (containerExists(manifest.container)) {
    await guestTransaction({
      command: "STOP",
      control: CONTROL,
      expected: "STOPPING\n",
      timeoutMs: STOP_TIMEOUT_MS,
    }).catch(() => null);
    run("docker", ["container", "rm", "--force", manifest.container]);
  }
  rmSync(RUNTIME, { recursive: true, force: true });
  const elapsed = performance.now() - started;
  if (elapsed > LIFECYCLE_TIMEOUT_MS) {
    throw new Error(`Bluetooth radio teardown took ${elapsed}ms`);
  }
  process.stdout.write(
    `Bluetooth radio environment stopped in ${Math.round(elapsed)}ms.\n`,
  );
}

function validate() {
  inputs();
  process.stdout.write("Pinned Bluetooth radio configuration passed.\n");
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const actions = {
    down,
    provision,
    "self-test": () =>
      runBluetoothRadioSelfTest(
        { control: CONTROL, reports: REPORTS, trace: TRACE },
        process.argv[3],
      ),
    up,
    validate,
  };
  const [, , command] = process.argv;
  if (!actions[command]) {
    throw new Error(`unknown Bluetooth radio command: ${command}`);
  }
  await actions[command]();
}
