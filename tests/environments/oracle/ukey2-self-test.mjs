import { spawn } from "node:child_process";

const BINARY = process.env.UKEY2_SHELL;
if (!BINARY) throw new Error("UKEY2_SHELL is required");

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
        10_000,
      );
      this.#waiters.push((frame) => {
        clearTimeout(timer);
        resolve(frame);
      });
      this.#drain();
    });
  }

  #drain() {
    while (this.#waiters.length && this.#buffer.length >= 4) {
      const length = this.#buffer.readUInt32BE(0);
      if (this.#buffer.length < length + 4) return;
      const frame = this.#buffer.subarray(4, length + 4);
      this.#buffer = this.#buffer.subarray(length + 4);
      this.#waiters.shift()(frame);
    }
  }
}

function writeFrame(process, value) {
  const payload = Buffer.isBuffer(value) ? value : Buffer.from(value);
  const header = Buffer.alloc(4);
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

async function exchange() {
  const initiator = peer("initiator");
  const responder = peer("responder");
  try {
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
    if (
      !initiatorSession.equals(responderSession) ||
      !initiatorSession.length
    ) {
      throw new Error("UKEY2 peers derived different session identifiers");
    }

    const outbound = Buffer.from("file/plain-text/URL outbound");
    writeFrame(
      initiator.process,
      Buffer.concat([Buffer.from("encrypt "), outbound]),
    );
    const encryptedOutbound = await initiator.reader.read();
    if (encryptedOutbound.includes(outbound)) {
      throw new Error("UKEY2 ciphertext exposes its plaintext");
    }
    writeFrame(
      responder.process,
      Buffer.concat([Buffer.from("decrypt "), encryptedOutbound]),
    );
    if (!(await responder.reader.read()).equals(outbound)) {
      throw new Error("UKEY2 responder failed to decrypt initiator payload");
    }

    const inbound = Buffer.from("file/plain-text/URL inbound");
    writeFrame(
      responder.process,
      Buffer.concat([Buffer.from("encrypt "), inbound]),
    );
    const encryptedInbound = await responder.reader.read();
    writeFrame(
      initiator.process,
      Buffer.concat([Buffer.from("decrypt "), encryptedInbound]),
    );
    if (!(await initiator.reader.read()).equals(inbound)) {
      throw new Error("UKEY2 initiator failed to decrypt responder payload");
    }
  } finally {
    initiator.process.kill();
    responder.process.kill();
  }
}

await exchange();
process.stdout.write(
  "Google UKEY2 bidirectional reference self-test passed.\n",
);
