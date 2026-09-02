import {
  existsSync,
  mkdirSync,
  readFileSync,
  renameSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { createConnection } from "node:net";
import { dirname, join } from "node:path";

const { recordFailureArtifact } = await import(
  new URL("../../../tools/gates/lib/failure-artifact.mjs", import.meta.url)
);

const GUEST_RETRY_MS = 25;
const GUEST_TIMEOUT_MS = 60_000;
const EXPECTED_CONTROLLER_COUNT = 2;
const CONTROLLER_TRANSCRIPT_PATTERN = /^OUT Controller /gmu;
const BTSNOOP_HEADER_BYTES = 16;
const BTSNOOP_RECORD_HEADER_BYTES = 24;
const BTSNOOP_ORIGINAL_LENGTH_OFFSET = 0;
const BTSNOOP_INCLUDED_LENGTH_OFFSET = 4;
const BTSNOOP_FLAGS_OFFSET = 8;
const BTSNOOP_DROPS_OFFSET = 12;
const BTSNOOP_PAYLOAD_OFFSET = 24;
const BTSNOOP_VERSION_OFFSET = 8;
const BTSNOOP_DATALINK_OFFSET = 12;
const BTSNOOP_VERSION = 1;
const BTSNOOP_UART_H4_DATALINK = 1002;
const BTSNOOP_MAGIC = Buffer.from("btsnoop\0");
const BTSNOOP_RECEIVED_FLAG = 1;
const BTSNOOP_DIRECTION_DIVISOR = 2;
const H4_COMMAND_PACKET = 1;
const H4_EVENT_PACKET = 4;
const H4_COMMAND_HEADER_BYTES = 3;
const H4_EVENT_HEADER_BYTES = 2;
const H4_META_EVENT_BYTES = 4;
const H4_LE_META_EVENT = 0x3e;
const H4_COMMAND_OPCODE_OFFSET = 1;
const H4_EVENT_CODE_OFFSET = 1;
const H4_SUBEVENT_CODE_OFFSET = 3;

function packetSummary(source, offset, includedBytes) {
  const flags = source.readUInt32BE(offset + BTSNOOP_FLAGS_OFFSET);
  const packetOffset = offset + BTSNOOP_PAYLOAD_OFFSET;
  let direction = "sent";
  if (flags % BTSNOOP_DIRECTION_DIVISOR === BTSNOOP_RECEIVED_FLAG) {
    direction = "received";
  }
  let packetType = 0;
  if (includedBytes > 0) {
    packetType = source[packetOffset];
  }
  const packet = {
    direction,
    droppedPackets: source.readUInt32BE(offset + BTSNOOP_DROPS_OFFSET),
    includedBytes,
    originalBytes: source.readUInt32BE(offset + BTSNOOP_ORIGINAL_LENGTH_OFFSET),
    packetType,
  };
  if (
    packetType === H4_COMMAND_PACKET &&
    includedBytes >= H4_COMMAND_HEADER_BYTES
  ) {
    packet.commandOpcode = source.readUInt16LE(
      packetOffset + H4_COMMAND_OPCODE_OFFSET,
    );
  }
  if (
    packetType === H4_EVENT_PACKET &&
    includedBytes >= H4_EVENT_HEADER_BYTES
  ) {
    packet.eventCode = source[packetOffset + H4_EVENT_CODE_OFFSET];
  }
  if (
    packet.eventCode === H4_LE_META_EVENT &&
    includedBytes >= H4_META_EVENT_BYTES
  ) {
    packet.subeventCode = source[packetOffset + H4_SUBEVENT_CODE_OFFSET];
  }
  return packet;
}

function parseBtsnoop(path) {
  const source = readFileSync(path);
  if (
    source.length < BTSNOOP_HEADER_BYTES ||
    !source.subarray(0, BTSNOOP_MAGIC.length).equals(BTSNOOP_MAGIC) ||
    source.readUInt32BE(BTSNOOP_VERSION_OFFSET) !== BTSNOOP_VERSION ||
    source.readUInt32BE(BTSNOOP_DATALINK_OFFSET) !== BTSNOOP_UART_H4_DATALINK
  ) {
    throw new Error("invalid btsnoop header");
  }
  let bytes = 0;
  let offset = BTSNOOP_HEADER_BYTES;
  const packets = [];
  while (offset + BTSNOOP_RECORD_HEADER_BYTES <= source.length) {
    const length = source.readUInt32BE(offset + BTSNOOP_INCLUDED_LENGTH_OFFSET);
    const next = offset + BTSNOOP_RECORD_HEADER_BYTES + length;
    if (next > source.length) {
      throw new Error("truncated btsnoop record");
    }
    packets.push(packetSummary(source, offset, length));
    bytes += length;
    offset = next;
  }
  if (offset !== source.length) {
    throw new Error("truncated btsnoop record header");
  }
  return {
    metadata: { bytes, format: "btsnoop", records: packets.length },
    packets,
  };
}

export function guestTransaction(options) {
  const { command, control, expected, timeoutMs = GUEST_TIMEOUT_MS } = options;
  return new Promise((resolvePromise, rejectPromise) => {
    const started = performance.now();
    let transcript = "";
    let socket = null;
    let retry = null;
    let sent = false;
    const deadline = setTimeout(() => {
      socket?.destroy();
      rejectPromise(new Error("guest control timed out"));
    }, timeoutMs);
    const finish = (error) => {
      clearTimeout(deadline);
      clearTimeout(retry);
      socket?.destroy();
      if (error) {
        rejectPromise(error);
      } else {
        resolvePromise({ elapsed: performance.now() - started, transcript });
      }
    };
    const connect = () => {
      socket = createConnection(control);
      socket.setEncoding("utf8");
      socket.once("error", (error) => {
        socket.destroy();
        if (performance.now() - started < timeoutMs) {
          retry = setTimeout(connect, GUEST_RETRY_MS);
        } else {
          finish(error);
        }
      });
      socket.on("data", (chunk) => {
        transcript += chunk;
        if (command && transcript.includes("READY\n") && !sent) {
          sent = true;
          socket.write(`${command}\n`);
        }
        if (transcript.includes(expected)) {
          finish();
        }
      });
    };
    connect();
  });
}

export function btsnoopTraceMetadata(path) {
  return parseBtsnoop(path).metadata;
}

export function writeBtsnoopTraceSummary(source, destination) {
  const { packets } = parseBtsnoop(source);
  const summary = { format: "btsnoop-summary-v1", packets, schema: 1 };
  mkdirSync(dirname(destination), { recursive: true });
  const temporary = `${destination}.temporary`;
  writeFileSync(temporary, `${JSON.stringify(summary, null, 2)}\n`);
  renameSync(temporary, destination);
  return destination;
}

function preserveFailure(paths, kind, outcome) {
  const detail = {
    events: [{ event: kind, status: "failed" }],
    gate: "bluetooth-radio",
    outcome: { kind: outcome },
    stage: "guest-control",
  };
  if (existsSync(paths.trace)) {
    const summary = join(paths.reports, "bluetooth-radio-trace.json");
    writeBtsnoopTraceSummary(paths.trace, summary);
    detail.trace = {
      ...btsnoopTraceMetadata(paths.trace),
      summary: "bluetooth-radio-trace",
    };
  }
  recordFailureArtifact(paths.reports, detail);
}

function prove(proof) {
  const { kind, marker, message, paths, result } = proof;
  if (!result.transcript.includes(marker)) {
    preserveFailure(paths, kind, "incomplete");
    throw new Error(message);
  }
}

function proveControllers(kind, result, paths) {
  const count =
    result.transcript.match(CONTROLLER_TRANSCRIPT_PATTERN)?.length ?? 0;
  if (count !== EXPECTED_CONTROLLER_COUNT) {
    preserveFailure(paths, kind, "incomplete");
    throw new Error(`guest reported ${count} controllers`);
  }
}

async function runRadioCommand(paths, kind, command) {
  try {
    return await guestTransaction({
      command,
      control: paths.control,
      expected: "STATUS ",
    });
  } catch {
    preserveFailure(paths, kind, "guest-control");
    throw new Error(`Bluetooth ${kind} guest command failed`);
  }
}

export async function runBluetoothRadioSelfTest(paths, kind) {
  const commands = {
    ble: "RUN_BLE",
    classic: "RUN_CLASSIC",
    controller: "RUN_CONTROLLER",
  };
  if (!commands[kind]) {
    throw new Error(`unknown radio self-test: ${kind}`);
  }
  rmSync(join(paths.reports, "bluetooth-radio-failure.json"), { force: true });
  rmSync(join(paths.reports, "bluetooth-radio-trace.json"), { force: true });
  const result = await runRadioCommand(paths, kind, commands[kind]);
  if (!result.transcript.includes("STATUS 0")) {
    preserveFailure(paths, kind, "failed");
    throw new Error(`Bluetooth ${kind} self-test failed`);
  }
  if (kind === "ble") {
    prove({
      kind,
      marker: "OUT BLUEZ_GATT_BIDIRECTIONAL_OK",
      message: "Bluetooth LE proof is incomplete",
      paths,
      result,
    });
  } else if (kind === "classic") {
    prove({
      kind,
      marker: "OUT BLUEZ_RFCOMM_BIDIRECTIONAL_OK",
      message: "Bluetooth Classic proof is incomplete",
      paths,
      result,
    });
  } else {
    proveControllers(kind, result, paths);
  }
}
