import { spawn } from "node:child_process";

const BINARY = process.env.UKEY2_SHELL;
const FRAME_HEADER_BYTES = 4;
const READ_TIMEOUT_MS = 10_000;
if (!BINARY) {
  throw new Error("UKEY2_SHELL is required");
}

class FrameReader {
  #buffer = Buffer.alloc(0);
  #waiters = [];

  constructor(stream) {
    stream.on("data", (chunk) => {
      this.#buffer = Buffer.concat([this.#buffer, chunk]);
      this.#drain();
    });
  }

  read() {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(
        () => reject(new Error("timed out waiting for UKEY2 frame")),
        READ_TIMEOUT_MS,
      );
      this.#waiters.push((frame) => {
        clearTimeout(timer);
        resolve(frame);
      });
      this.#drain();
    });
  }

  #drain() {
    while (this.#waiters.length && this.#buffer.length >= FRAME_HEADER_BYTES) {
      const length = this.#buffer.readUInt32BE(0);
      if (this.#buffer.length < length + FRAME_HEADER_BYTES) {
        return;
      }
      const frame = this.#buffer.subarray(
        FRAME_HEADER_BYTES,
        length + FRAME_HEADER_BYTES,
      );
      this.#buffer = this.#buffer.subarray(length + FRAME_HEADER_BYTES);
      this.#waiters.shift()(frame);
    }
  }
}

function writeFrame(process, value) {
  let payload = value;
  if (!Buffer.isBuffer(value)) {
    payload = Buffer.from(value);
  }
  const header = Buffer.alloc(FRAME_HEADER_BYTES);
  header.writeUInt32BE(payload.length);
  process.stdin.write(Buffer.concat([header, payload]));
}

function peer(mode) {
  const process = spawn(BINARY, [`--mode=${mode}`], {
    stdio: ["pipe", "pipe", "pipe"],
  });
  let errors = "";
  process.stderr.setEncoding("utf8");
  process.stderr.on("data", (chunk) => {
    errors += chunk;
  });
  return {
    process,
    reader: new FrameReader(process.stdout),
    errors: () => errors,
  };
}

async function establishSession({ initiator, responder }) {
  writeFrame(responder.process, await initiator.reader.read());
  writeFrame(initiator.process, await responder.reader.read());
  writeFrame(responder.process, await initiator.reader.read());

  const [initiatorVerification, responderVerification] = await Promise.all([
    initiator.reader.read(),
    responder.reader.read(),
  ]);
  if (!initiatorVerification.equals(responderVerification)) {
    throw new Error("UKEY2 peers derived different verification strings");
  }
  writeFrame(initiator.process, "ok");
  writeFrame(responder.process, "ok");

  writeFrame(initiator.process, "session_unique ");
  writeFrame(responder.process, "session_unique ");
  const [initiatorSession, responderSession] = await Promise.all([
    initiator.reader.read(),
    responder.reader.read(),
  ]);
  if (!initiatorSession.equals(responderSession) || !initiatorSession.length) {
    throw new Error("UKEY2 peers derived different session identifiers");
  }
}

async function proveDirection(sender, receiver, payload) {
  writeFrame(sender.process, Buffer.concat([Buffer.from("encrypt "), payload]));
  const encrypted = await sender.reader.read();
  if (encrypted.includes(payload)) {
    throw new Error("UKEY2 ciphertext exposes its plaintext");
  }
  writeFrame(
    receiver.process,
    Buffer.concat([Buffer.from("decrypt "), encrypted]),
  );
  if (!(await receiver.reader.read()).equals(payload)) {
    throw new Error("UKEY2 receiver failed to decrypt sender payload");
  }
}

async function exchange() {
  const initiator = peer("initiator");
  const responder = peer("responder");
  try {
    await establishSession({ initiator, responder });
    await proveDirection(
      initiator,
      responder,
      Buffer.from("file/plain-text/URL outbound"),
    );
    await proveDirection(
      responder,
      initiator,
      Buffer.from("file/plain-text/URL inbound"),
    );
  } finally {
    initiator.process.kill();
    responder.process.kill();
  }
}

await exchange();
process.stdout.write(
  "Google UKEY2 bidirectional reference self-test passed.\n",
);
