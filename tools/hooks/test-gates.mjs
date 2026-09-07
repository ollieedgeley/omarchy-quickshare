import { getRepositoryDomain } from "./affected.mjs";

const NODE_TEST = /\.(?:test|spec)\.(?:mjs|cjs|js)$/u;
const EXECUTABLE_TEST =
  /\.(?:e2e|test|spec)\.|(?:^|\/)test_[^/]+$|_test\.[^/]+$/u;
const DOCUMENT = /\.md$/u;
const RUST_LAN_DIRECTORY = "tests/environments/diverse-lan/rust/scenarios";
const RUST_LAN_CASES = [
  "outbound",
  "inbound",
  "inbound-content",
  "outbound-content",
  "cancellation-inbound",
  "cancellation-outbound",
  "failure-inbound",
  "failure-outbound",
  "rejection",
  "retry",
];
const ORACLE_CASES = [
  "bluetooth",
  "ble",
  "lan",
  "hotspot",
  "wifi-direct",
  "bwu-handler",
  "bwu-fallback",
];
const SOURCE_PREPARE = ["sources-fetch"];
const NEARBY_PREPARE = [...SOURCE_PREPARE, "nearby-linux-provision"];
const ORACLE_PREPARE = [
  ...SOURCE_PREPARE,
  "oracle-provision",
  "oracle-reference-provision",
];

function family(targets, prepare, cleanup = []) {
  return targets.map((target) => ({ cleanup, prepare, target }));
}

const FAMILIES = {
  dbus: family(
    ["test-dbus-bluez", "test-dbus-networkmanager"],
    ["dbus-provision"],
  ),
  diverse: family(
    ["test-diverse-lan"],
    [...NEARBY_PREPARE, "nearshare-provision"],
  ),
  liveBwu: family(
    ["test-live-bwu-kvm"],
    [...NEARBY_PREPARE, "bluetooth-radio-provision", "live-bwu-kvm-provision"],
  ),
  nearby: family(
    [
      "test-nearby-linux-connections",
      "test-nearby-linux-sharing",
      "test-nearby-linux-sharing-actions",
      "test-nearby-linux-sharing-fixtures",
    ],
    NEARBY_PREPARE,
  ),
  nearshare: family(
    ["test-nearshare-reference"],
    [...SOURCE_PREPARE, "nearshare-provision"],
  ),
  network: family(
    [
      "test-network-wmediumd",
      "test-network-netem",
      "test-network-lan",
      "test-network-hotspot-client",
      "test-network-hotspot-owner",
      "test-network-wifi-direct-client",
    ],
    [...SOURCE_PREPARE, "network-provision"],
  ),
  oracle: [
    ...family(["test-oracle-toolchain"], ["oracle-provision"]),
    ...family(["test-oracle-reference"], ORACLE_PREPARE),
    ...family(
      ORACLE_CASES.map((name) => `test-oracle-${name}`),
      [...ORACLE_PREPARE, "oracle-reference-up"],
      ["oracle-reference-down"],
    ),
  ],
  oracleReference: family(["test-oracle-reference"], ORACLE_PREPARE),
  pluginCapture: family(["test-plugin-capture"], ["plugin-capture-prepare"]),
  proxy: family(
    ["test-proxy-toxiproxy"],
    [...SOURCE_PREPARE, "proxy-provision"],
  ),
  radio: family(
    [
      "test-bluetooth-controller",
      "test-bluetooth-ble",
      "test-bluetooth-classic",
    ],
    [...SOURCE_PREPARE, "bluetooth-radio-provision"],
  ),
  rustLan: family(
    RUST_LAN_CASES.map((name) => `test-rust-lan-${name}`),
    [...NEARBY_PREPARE, "rust-lan-provision"],
  ),
  sources: family(["test-source-cache"], SOURCE_PREPARE),
};
const REGISTERED_TESTS = new Map(
  RUST_LAN_CASES.map((name) => [
    `${RUST_LAN_DIRECTORY}/rust-lan-${name}.e2e.mjs`,
    FAMILIES.rustLan.find(({ target }) => target === `test-rust-lan-${name}`),
  ]),
);
REGISTERED_TESTS.set(
  "tools/release/tests/capture-journey.e2e.mjs",
  FAMILIES.pluginCapture[0],
);
const ALL_FAMILIES = Object.keys(FAMILIES);

function within(path, directory) {
  return path === directory || path.startsWith(`${directory}/`);
}

function sharedInputFamilies(path) {
  if (
    within(path, "packaging/omarchy-plugin") ||
    within(path, "tools/release/tests")
  ) {
    return ["pluginCapture"];
  }
  if (
    within(path, "crates/core/crypto") ||
    within(path, "crates/core/wire") ||
    within(path, ".cargo") ||
    [
      "Cargo.toml",
      "Cargo.lock",
      "rust-toolchain.toml",
      "rustfmt.toml",
      "clippy.toml",
    ].includes(path)
  ) {
    return ["rustLan", "oracleReference", "pluginCapture"];
  }
  if (within(path, "crates") || within(path, "packaging/systemd")) {
    return ["rustLan", "pluginCapture"];
  }
  if (
    ["Makefile", "tools/gates/environments.mk"].includes(path) ||
    within(path, "upstream") ||
    within(path, "tests/support") ||
    [
      "tools/gates/sources.mjs",
      "tools/gates/lib/process.mjs",
      "tools/gates/lib/failure-artifact.mjs",
    ].includes(path)
  ) {
    return ALL_FAMILIES;
  }
  return null;
}

function inputFamilies(path) {
  if (DOCUMENT.test(path) || within(path, "docs") || NODE_TEST.test(path)) {
    return [];
  }
  const shared = sharedInputFamilies(path);
  if (shared) {
    return shared;
  }
  if (
    within(path, "tests/environments/nearby-linux") ||
    within(path, "tools/oracle/connections-peer")
  ) {
    return ["nearby", "diverse", "rustLan", "liveBwu"];
  }
  if (within(path, "tests/environments/diverse-lan/rust")) {
    return ["rustLan"];
  }
  if (within(path, "tests/environments/diverse-lan")) {
    return ["diverse", "rustLan"];
  }
  if (within(path, "tests/environments/nearshare")) {
    return ["nearshare", "diverse"];
  }
  if (
    within(path, "tests/environments/oracle") ||
    within(path, "tools/oracle/sharing-fixtures")
  ) {
    return ["oracle"];
  }
  if (within(path, "tests/fixtures/sharing/google-v1")) {
    return ["nearby"];
  }
  if (within(path, "tests/environments/bluez")) {
    if (
      path.split("/").pop().startsWith("dbus") ||
      path.endsWith("Dockerfile.dbus")
    ) {
      return ["dbus"];
    }
    return ["radio", "liveBwu"];
  }
  const environments = [
    ["proxies", "proxy"],
    ["network", "network"],
    ["live-bwu-kvm", "liveBwu"],
  ];
  return environments
    .filter(([directory]) => within(path, `tests/environments/${directory}`))
    .map(([, name]) => name);
}

function domainNodeTests(changedPaths, repositoryFiles) {
  const domains = new Set(changedPaths.map(getRepositoryDomain));
  return repositoryFiles.filter((path) => {
    if (!NODE_TEST.test(path)) {
      return false;
    }
    for (const domain of domains) {
      if (
        domain === "tooling" &&
        (within(path, "tools/gates/tests") || within(path, "tools/hooks"))
      ) {
        return true;
      }
      if (domain === "plugin-release" && within(path, "tools/release/tests")) {
        return true;
      }
      if (
        (domain.startsWith("tests/environments/") ||
          domain.startsWith("tests/suites/")) &&
        within(path, domain)
      ) {
        return true;
      }
      if (
        domain === "oracle" &&
        (within(path, "tests/environments/oracle") ||
          (within(path, "tools/gates/tests") && path.includes("oracle")))
      ) {
        return true;
      }
    }
    return false;
  });
}

function addGate(gates, definition, { reason, testPath }) {
  let gate = gates.get(definition.target);
  if (!gate) {
    gate = {
      cleanup: [...definition.cleanup],
      id: definition.target,
      prepare: [...definition.prepare],
      reasons: [],
      target: definition.target,
      testPaths: [],
    };
    gates.set(definition.target, gate);
  }
  if (
    !gate.reasons.some(
      (item) => item.kind === reason.kind && item.path === reason.path,
    )
  ) {
    gate.reasons.push(reason);
  }
  if (testPath && !gate.testPaths.includes(testPath)) {
    gate.testPaths.push(testPath);
  }
}

function executableGate(path) {
  const registered = REGISTERED_TESTS.get(path);
  if (registered) {
    return registered;
  }
  if (
    EXECUTABLE_TEST.test(path) &&
    !NODE_TEST.test(path) &&
    !path.endsWith(".rs")
  ) {
    let detail =
      "Register its existing noninteractive Make gate and preparation " +
      "lifecycle in tools/hooks/test-gates.mjs.";
    if (within(path, "tests/environments/android")) {
      detail =
        "The Android live runner is experimental and is not admitted " +
        "to automated hooks.";
    }
    throw new Error(
      `No automated executable test runner for ${path}. ${detail}`,
    );
  }
  return null;
}

function addInputGates(gates, path, kind) {
  for (const name of inputFamilies(path)) {
    for (const definition of FAMILIES[name]) {
      addGate(gates, definition, {
        reason: { kind: `${kind}-input`, path },
      });
    }
  }
}

function reconcileTestPaths(gates, changedPaths, files) {
  for (const path of changedPaths) {
    const registered = REGISTERED_TESTS.get(path);
    if (registered && !files.has(path)) {
      gates.delete(registered.target);
    }
  }
  for (const [path, registered] of REGISTERED_TESTS) {
    const gate = gates.get(registered.target);
    if (gate && files.has(path) && !gate.testPaths.includes(path)) {
      gate.testPaths.push(path);
    }
  }
}

function fixtureRustInputs(paths, files) {
  const owners = [
    [
      "tests/fixtures/control/v5",
      [
        "crates/app/tests/daemon_process/process_contract.rs",
        "tests/suites/contracts/tests/suite/control_contract.rs",
      ],
    ],
    [
      "tests/fixtures/sharing/scenarios/v1",
      ["tests/suites/contracts/tests/suite/scenario_contract.rs"],
    ],
    [
      "tests/fixtures/sharing/google-v1",
      [
        "crates/core/wire/tests/wire/tests.rs",
        "crates/core/sharing/tests/outbound_completion.rs",
      ],
    ],
  ];
  return [
    ...new Set(
      owners
        .filter(([directory]) =>
          paths.some((path) => !DOCUMENT.test(path) && within(path, directory)),
        )
        .flatMap(([, consumers]) => consumers)
        .filter((path) => files.has(path)),
    ),
  ].sort();
}

export function planAffectedGates({
  changedPaths,
  affectedTests,
  repositoryFiles,
}) {
  const files = new Set(repositoryFiles);
  const gates = new Map();
  const fastNodeTests = new Set(domainNodeTests(changedPaths, repositoryFiles));
  for (const [kind, candidates] of [
    ["changed", changedPaths],
    ["codegraph", affectedTests],
  ]) {
    for (const path of new Set(candidates)) {
      const exists = files.has(path);
      let registered = null;
      if (exists) {
        registered = executableGate(path);
      }
      if (registered) {
        addGate(gates, registered, { reason: { kind, path }, testPath: path });
      } else {
        if (exists && NODE_TEST.test(path)) {
          fastNodeTests.add(path);
        }
        addInputGates(gates, path, kind);
      }
    }
  }
  reconcileTestPaths(gates, [...changedPaths, ...affectedTests], files);
  for (const gate of gates.values()) {
    gate.reasons.sort(
      (left, right) =>
        left.kind.localeCompare(right.kind) ||
        left.path.localeCompare(right.path),
    );
    gate.testPaths.sort();
  }
  return {
    fastNodeTests: [...fastNodeTests].sort(),
    gates: [...gates.values()].sort((left, right) =>
      left.id.localeCompare(right.id),
    ),
    rustInputs: fixtureRustInputs([...changedPaths, ...affectedTests], files),
  };
}
