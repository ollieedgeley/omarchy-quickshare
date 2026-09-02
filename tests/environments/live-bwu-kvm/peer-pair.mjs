const STATUS_SUCCESS = "STATUS 0";
const PEER_EVENTS = /PEER_EVENTS .*/u;

async function evidence(transaction) {
  const results = await Promise.allSettled(
    ["a", "b"].map((peer) =>
      transaction({ peer, command: "PEER_EVIDENCE", expected: STATUS_SUCCESS }),
    ),
  );
  return results
    .filter(({ status }) => status === "fulfilled")
    .map(({ value }) => value.match(PEER_EVENTS)?.[0] ?? "")
    .join("; ");
}

async function clean(transaction) {
  await Promise.all(
    ["a", "b"].map(async (peer) => {
      await transaction({
        peer,
        command: "PEER_STOP",
        expected: STATUS_SUCCESS,
      });
      await transaction({
        peer,
        command: "PEER_CLEAN",
        expected: "PEER_CLEAN",
      });
    }),
  );
}

export async function runPeerPair(transaction) {
  try {
    await transaction({
      peer: "a",
      command: "PEER_ADVERTISE",
      expected: STATUS_SUCCESS,
    });
    await transaction({
      peer: "b",
      command: "PEER_DISCOVER",
      expected: STATUS_SUCCESS,
    });
    const results = await Promise.all(
      ["a", "b"].map((peer) =>
        transaction({ peer, command: "PEER_RESULT", expected: STATUS_SUCCESS }),
      ),
    );
    if (results.some((value) => !value.includes("BWU_BLE_TO_BLUETOOTH_OK"))) {
      throw new Error("BLE to Classic upgrade evidence is incomplete");
    }
  } catch (error) {
    throw new Error(`${error.message}; ${await evidence(transaction)}`, {
      cause: error,
    });
  } finally {
    await clean(transaction);
  }
}
