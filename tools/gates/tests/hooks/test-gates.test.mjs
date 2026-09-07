import assert from "node:assert/strict";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { planAffectedGates } from "../../../hooks/test-gates.mjs";
import { listProjectFiles } from "../../lib/analysis.mjs";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../../../..");
const SCENARIOS = "tests/environments/diverse-lan/rust/scenarios";
const INBOUND = `${SCENARIOS}/rust-lan-inbound.e2e.mjs`;
const FAILURE = `${SCENARIOS}/rust-lan-failure-inbound.e2e.mjs`;
const CONSENT = "crates/app/src/daemon/network/inbound/consent.rs";
const CAPTURE = "tools/release/tests/capture-journey.e2e.mjs";
const CASES = [
  "cancellation-inbound",
  "cancellation-outbound",
  "failure-inbound",
  "failure-outbound",
  "inbound",
  "inbound-content",
  "outbound",
  "outbound-content",
  "rejection",
  "retry",
];

function plan(
  changedPaths,
  affectedTests = [],
  repositoryFiles = changedPaths,
) {
  return planAffectedGates({ affectedTests, changedPaths, repositoryFiles });
}

function targets(selection) {
  return selection.gates.map((gate) => gate.target);
}

test("daemon consent selects all Rust LAN cases without graph edges", () => {
  const selection = plan([CONSENT]);
  assert.deepEqual(targets(selection), [
    "test-plugin-capture",
    ...CASES.map((name) => `test-rust-lan-${name}`),
  ]);
  assert.deepEqual(selection.fastNodeTests, []);
  const failure = selection.gates.find(
    (gate) => gate.target === "test-rust-lan-failure-inbound",
  );
  assert.deepEqual(failure.prepare, [
    "sources-fetch",
    "nearby-linux-provision",
    "rust-lan-provision",
  ]);
  assert.deepEqual(failure.reasons, [{ kind: "changed-input", path: CONSENT }]);
  assert.deepEqual(failure.cleanup, []);
});

test("staged and graph E2E select Make children, never raw Node", () => {
  const selection = plan([INBOUND], [FAILURE, INBOUND], [INBOUND, FAILURE]);
  assert.deepEqual(targets(selection), [
    "test-rust-lan-failure-inbound",
    "test-rust-lan-inbound",
  ]);
  assert.deepEqual(selection.fastNodeTests, []);
  assert.deepEqual(selection.gates[1].testPaths, [INBOUND]);
  assert.deepEqual(selection.gates[1].reasons, [
    { kind: "changed", path: INBOUND },
    { kind: "codegraph", path: INBOUND },
  ]);
});

test("every current E2E has an exact runnable child registration", () => {
  const files = listProjectFiles(ROOT);
  const e2e = files.filter((path) => path.includes(".e2e."));
  assert.deepEqual(
    e2e.sort(),
    [
      CAPTURE,
      ...CASES.map((name) => `${SCENARIOS}/rust-lan-${name}.e2e.mjs`),
    ].sort(),
  );
  for (const path of e2e) {
    const selection = plan([], [path], files);
    const name = path.split("/").pop().replace(".e2e.mjs", "");
    let target = `test-${name}`;
    if (path === CAPTURE) {
      target = "test-plugin-capture";
    }
    assert.deepEqual(targets(selection), [target]);
    assert.deepEqual(selection.gates[0].testPaths, [path]);
  }
});

test("unknown staged and graph executable markers fail closed", () => {
  for (const path of [
    "tests/environments/diverse-lan/rust/scenarios/new.e2e.mjs",
    "tools/hooks/new.test.ts",
    "tests/test_transport.py",
    "tests/transport_test.cc",
  ]) {
    for (const [changed, graph] of [
      [[path], []],
      [[], [path]],
    ]) {
      assert.throws(
        () => plan(changed, graph, [path]),
        (error) =>
          error.message.includes(path) && error.message.includes("Register"),
      );
    }
  }
});

test("experimental Android executable cannot admit its live route", () => {
  const path =
    "tests/environments/android/probe/runner/nearby_connections_test.py";
  const unsupported = /Android live runner is experimental/u;
  assert.throws(() => plan([path]), unsupported);
  assert.deepEqual(
    targets(plan(["tests/environments/android/provision.mjs"])),
    [],
  );
});

test("deleted sources and tests retain remaining family coverage", () => {
  const selection = plan([CONSENT, FAILURE], [FAILURE], [INBOUND]);
  assert.deepEqual(targets(selection), [
    "test-plugin-capture",
    ...CASES.filter((name) => name !== "failure-inbound").map(
      (name) => `test-rust-lan-${name}`,
    ),
  ]);
  assert.ok(selection.gates.every((gate) => !gate.testPaths.includes(FAILURE)));
});

test("documentation and ordinary helpers are not executable candidates", () => {
  assert.deepEqual(
    plan(
      ["docs/README.md"],
      ["tests/helpers/assertions.py"],
      ["docs/README.md", "tests/helpers/assertions.py"],
    ),
    { fastNodeTests: [], gates: [], rustInputs: [] },
  );
  assert.deepEqual(targets(plan(["tools/gates/hooks.mk"])), []);
});

test("fast domain fallback and supplied Node tests remain a union", () => {
  const fast = "tools/gates/tests/hooks/hook-contracts.test.mjs";
  const graph = "tools/release/tests/native-release-contract.test.mjs";
  const helper = "tools/gates/tests/hooks/fixture.mjs";
  const timing = "tools/hooks/pre-push-timing.test.mjs";
  const selection = plan(
    ["tools/hooks/affected.mjs"],
    [graph],
    [fast, graph, helper, timing],
  );
  assert.deepEqual(selection.fastNodeTests, [fast, timing, graph]);
  assert.deepEqual(selection.gates, []);
});

test("environment inputs select their cross-process consumers", () => {
  const cases = [
    ["tests/environments/proxies/environment.json", "test-proxy-toxiproxy"],
    [
      "tests/environments/bluez/dbus-environment.json",
      "test-dbus-networkmanager",
    ],
    [
      "tests/environments/bluez/radio-bumble-gatt-peer.py",
      "test-bluetooth-ble",
    ],
    ["tests/environments/network/drop.cfg", "test-network-netem"],
    ["tests/environments/nearshare/environment.json", "test-diverse-lan"],
    [
      "tests/environments/nearby-linux/cli-actions.patch",
      "test-rust-lan-failure-inbound",
    ],
    ["tests/environments/live-bwu-kvm/guest_peer.py", "test-live-bwu-kvm"],
    [
      "tests/fixtures/sharing/google-v1/trace.json",
      "test-nearby-linux-sharing-fixtures",
    ],
    ["packaging/systemd/omarchy-quickshare.toml", "test-rust-lan-inbound"],
    ["Cargo.lock", "test-rust-lan-inbound"],
  ];
  for (const [path, target] of cases) {
    assert.ok(
      targets(plan([path])).includes(target),
      `${path} must select ${target}`,
    );
  }
});

test("shared oracle lifecycle and plans are deterministic", () => {
  const paths = [
    "tests/environments/oracle/selected-gtest.mjs",
    "tools/oracle/sharing-fixtures/main.cc",
  ];
  const selection = plan(paths);
  assert.deepEqual(selection, plan([...paths].reverse()));
  const medium = selection.gates.find(
    (gate) => gate.target === "test-oracle-ble",
  );
  assert.deepEqual(medium.prepare, [
    "sources-fetch",
    "oracle-provision",
    "oracle-reference-provision",
    "oracle-reference-up",
  ]);
  assert.deepEqual(medium.cleanup, ["oracle-reference-down"]);
  assert.equal(new Set(targets(selection)).size, selection.gates.length);
  assert.ok(
    !targets(selection).some((target) => target.startsWith("test-rust-lan-")),
  );
});

test("shared source inputs retain all admitted environment families", () => {
  for (const path of ["Makefile", "upstream/sources.toml"]) {
    const selection = targets(plan([path]));
    assert.ok(selection.includes("test-source-cache"));
    assert.ok(selection.includes("test-live-bwu-kvm"));
    assert.ok(selection.includes("test-rust-lan-failure-inbound"));
    assert.ok(selection.includes("test-oracle-bwu-fallback"));
    assert.ok(!selection.includes("test-android-nearby"));
  }
});

test("composed capture prepares the CLI before its Make gate", () => {
  for (const input of [CAPTURE, "packaging/omarchy-plugin/BarWidget.qml"]) {
    const selection = plan([input], [], [input, CAPTURE]);
    assert.deepEqual(selection.fastNodeTests, []);
    assert.deepEqual(targets(selection), ["test-plugin-capture"]);
    assert.deepEqual(selection.gates[0].prepare, ["plugin-capture-prepare"]);
  }
});

test("Rust crypto inputs select the enabled oracle integration only", () => {
  for (const path of [
    "crates/core/crypto/src/lib.rs",
    "crates/core/wire/src/lib.rs",
    "Cargo.lock",
  ]) {
    const selected = targets(plan([path]));
    assert.ok(selected.includes("test-oracle-reference"));
    assert.ok(!selected.includes("test-oracle-ble"));
  }
});

test("fixture inputs select actual Cargo consumers across languages", () => {
  const cases = [
    [
      "tests/fixtures/control/v5/status-request.jsonl",
      [
        "crates/app/tests/daemon_process/process_contract.rs",
        "tests/suites/contracts/tests/suite/control_contract.rs",
      ],
    ],
    [
      "tests/fixtures/sharing/scenarios/v1/decisions.json",
      ["tests/suites/contracts/tests/suite/scenario_contract.rs"],
    ],
    [
      "tests/fixtures/sharing/google-v1/incoming/responses/accept.bin",
      [
        "crates/core/sharing/tests/outbound_completion.rs",
        "crates/core/wire/tests/wire/tests.rs",
      ],
    ],
  ];
  for (const [fixture, consumers] of cases) {
    for (const [changed, graph] of [
      [[fixture], []],
      [[], [fixture]],
    ]) {
      const selection = plan(changed, graph, [fixture, ...consumers]);
      assert.deepEqual(selection.rustInputs, consumers);
      assert.ok(!targets(selection).includes("test-rust-lan-inbound"));
    }
  }
});

test("payload text inputs are not mistaken for documentation", () => {
  assert.ok(
    targets(plan(["tests/environments/diverse-lan/rust/payload.txt"])).includes(
      "test-rust-lan-inbound",
    ),
  );
});

test("source-selected gates record existing executable endpoints", () => {
  const selection = plan([CONSENT], [], [CONSENT, INBOUND]);
  const inbound = selection.gates.find(
    (gate) => gate.target === "test-rust-lan-inbound",
  );
  assert.deepEqual(inbound.testPaths, [INBOUND]);
});
