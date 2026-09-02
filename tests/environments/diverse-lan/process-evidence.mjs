const STAGE_PATTERN = /(?:QS_EVENT event=|"event":")(?<stage>[a-z-]+)/gu;
const STATUS_PATTERN = /status=(?<status>k[A-Za-z]+)/gu;

function evidenceCount(log) {
  return log
    .split("\n")
    .filter((line) => line.startsWith("QS_EVENT") || line.startsWith("{"))
    .length;
}

function values(log, pattern, group) {
  return [
    ...new Set([...log.matchAll(pattern)].map((match) => match.groups[group])),
  ].join(",");
}

export function assertProcessSuccess({ direction, receiver, results, sender }) {
  const [senderResult, receiverResult] = results;
  if (senderResult.code === 0 && receiverResult.code === 0) {
    return;
  }
  const senderLog = sender.logs();
  const receiverLog = receiver.logs();
  throw new Error(
    `diverse LAN ${direction} process failed ` +
      `(sender ${senderResult.code}/${evidenceCount(senderLog)}, ` +
      `receiver ${receiverResult.code}/${evidenceCount(receiverLog)}; ` +
      `stages ${values(senderLog, STAGE_PATTERN, "stage")}|` +
      `${values(receiverLog, STAGE_PATTERN, "stage")}; ` +
      `statuses ${values(senderLog, STATUS_PATTERN, "status")}|` +
      `${values(receiverLog, STATUS_PATTERN, "status")})`,
  );
}
