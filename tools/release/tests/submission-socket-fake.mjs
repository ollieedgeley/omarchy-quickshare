import { appendFileSync, existsSync, mkdirSync, writeFileSync } from "node:fs";
import { createConnection, createServer } from "node:net";
import { dirname, join } from "node:path";

const POLL_MS = 10;
const CONNECTION_TIMEOUT_MS = 5_000;
const MAX_FRAME_BYTES = 1_048_576;
const PROTOCOL_VERSION = 7;
const MAX_REQUEST_ID_BYTES = 128;
const LF = 10;
const socketPath = join(
  process.env.XDG_RUNTIME_DIR,
  "omarchy-quickshare/control.sock",
);

// One LF-delimited frame per connection, bounded like control v7.
function readFrame(socket, complete) {
  let bytes = Buffer.alloc(0);
  function receive(chunk) {
    bytes = Buffer.concat([bytes, chunk]);
    const end = bytes.indexOf(LF);
    if (
      bytes.length > MAX_FRAME_BYTES + 1 ||
      (end < 0 && bytes.length > MAX_FRAME_BYTES)
    ) {
      socket.destroy(new Error("Control frame exceeded byte limit"));
      return;
    }
    if (end < 0) {
      return;
    }
    socket.off("data", receive);
    if (end !== bytes.length - 1) {
      socket.destroy(new Error("Unexpected bytes after control frame"));
      return;
    }
    complete(bytes);
  }
  socket.on("data", receive);
}

function forward(client, frame, complete) {
  const upstream = createConnection(process.env.SUBMISSION_UPSTREAM);
  let received = false;
  upstream.on("error", (error) => client.destroy(error));
  upstream.on("end", () => {
    if (!received) {
      client.destroy();
    }
  });
  readFrame(upstream, (reply) => {
    received = true;
    upstream.destroy();
    try {
      complete(reply);
    } catch (error) {
      client.destroy(error);
    }
  });
  upstream.on("connect", () => upstream.write(frame));
  return upstream;
}

function waitForRelease(complete) {
  writeFileSync(process.env.SUBMISSION_STARTED, "started");
  const timer = setInterval(() => {
    if (existsSync(process.env.SUBMISSION_RELEASE)) {
      clearInterval(timer);
      complete();
    }
  }, POLL_MS);
  return timer;
}

function isSubmission(frame) {
  const envelope = JSON.parse(frame.toString("utf8"));
  const { request } = envelope;
  if (envelope.version !== PROTOCOL_VERSION) {
    throw new Error("Expected control protocol v7");
  }
  if (process.env.SUBMISSION_ENVELOPES) {
    appendFileSync(process.env.SUBMISSION_ENVELOPES, frame);
  }
  const submission = ["submit_text", "submit_file", "submit_url"].includes(
    request?.type,
  );
  if (submission) {
    if (
      typeof envelope.request_id !== "string" ||
      Buffer.byteLength(envelope.request_id, "utf8") === 0 ||
      Buffer.byteLength(envelope.request_id, "utf8") > MAX_REQUEST_ID_BYTES
    ) {
      throw new Error("Submission requires a bounded opaque request ID");
    }
    appendFileSync(process.env.SUBMISSION_ATTEMPTS, "submit\n");
  } else if (Object.hasOwn(envelope, "request_id")) {
    throw new Error("Non-submission envelope must omit request_id");
  }
  return submission;
}

function acceptBeforeReply(client, reply) {
  const accepted = JSON.parse(reply.toString("utf8"));
  const operationId = accepted.response?.operation?.id_string;
  const decimalId = /^[1-9]\d*$/u;
  if (
    accepted.version !== PROTOCOL_VERSION ||
    accepted.response?.type !== "queued" ||
    typeof operationId !== "string" ||
    !decimalId.test(operationId)
  ) {
    throw new Error("Expected queued operation before client reconnect");
  }
  // Keep the daemon's u64 ID exact in its native numeric wire field.
  const acceptFrame = Buffer.from(
    `{"version":${PROTOCOL_VERSION},"request":` +
      `{"type":"simulate_peer_accept","share_id":${operationId}}}\n`,
  );
  isSubmission(acceptFrame);
  return forward(client, acceptFrame, (appliedReply) => {
    const applied = JSON.parse(appliedReply.toString("utf8"));
    if (
      applied.version !== PROTOCOL_VERSION ||
      applied.response?.type !== "applied"
    ) {
      throw new Error(
        `Peer acceptance was not applied: ${appliedReply
          .toString("utf8")
          .trim()}`,
      );
    }
    client.end(reply);
  });
}

function handleConnection(client) {
  let upstream = null;
  let releaseTimer = null;
  const deadline = setTimeout(() => client.destroy(), CONNECTION_TIMEOUT_MS);
  function stop() {
    clearTimeout(deadline);
    clearInterval(releaseTimer);
    upstream?.destroy();
  }
  client.on("error", (error) => {
    process.stderr.write(`${error.message}\n`);
  });
  client.on("close", stop);
  function dispatch(frame, submission) {
    upstream = forward(client, frame, (reply) => {
      if (submission && process.env.SUBMISSION_MODE === "drop") {
        const accepted = JSON.parse(reply.toString("utf8"));
        if (accepted.response?.type !== "queued") {
          client.destroy(
            new Error("Expected accepted queued reply before drop"),
          );
          return;
        }
        releaseTimer = waitForRelease(() => client.destroy());
      } else if (
        submission &&
        process.env.SUBMISSION_MODE === "client-reconnect"
      ) {
        upstream = acceptBeforeReply(client, reply);
      } else if (submission && process.env.SUBMISSION_MODE === "after") {
        releaseTimer = waitForRelease(() => client.end(reply));
      } else {
        client.end(reply);
      }
    });
  }
  readFrame(client, (frame) => {
    try {
      const submission = isSubmission(frame);
      if (submission && process.env.SUBMISSION_MODE === "before") {
        releaseTimer = waitForRelease(() => dispatch(frame, submission));
      } else {
        dispatch(frame, submission);
      }
    } catch (error) {
      client.destroy(error);
    }
  });
}

mkdirSync(dirname(socketPath), { recursive: true });
const server = createServer(handleConnection);
server.on("error", (error) => {
  process.stderr.write(`${error.stack}\n`);
  process.exit(1);
});
server.listen(socketPath, () => process.send("ready"));
