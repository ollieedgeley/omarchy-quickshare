import test from "node:test";
import { runJourney } from "./capture-journey-runner.mjs";

for (const type of ["file", "text", "url"]) {
  for (const automatic of [false, true]) {
    for (const captureFirst of [true, false]) {
      const name = `${type}: auto=${automatic}, first=${captureFirst}`;
      test(name, async () => {
        await runJourney({ automatic, captureFirst, type });
      });
    }
  }
}

test("explicit Paste replaces a pending automatic read", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    replacement: true,
    type: "text",
  });
});

test("failed explicit replacement cannot fall back to auto", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    failReplacement: true,
    replacement: true,
    type: "text",
  });
});

for (const invalidate of ["close", "clear", "cancel"]) {
  for (const automatic of [false, true]) {
    test(`${invalidate}: auto=${automatic}`, async () => {
      await runJourney({
        automatic,
        captureFirst: false,
        invalidate,
        type: "text",
      });
    });
  }
}

test("close invalidates an already queued replacement", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    invalidate: "close",
    queuedReplacement: true,
    type: "text",
  });
});

test("fresh Paste and recipient recover after preparation Cancel", async () => {
  await runJourney({
    automatic: false,
    captureFirst: false,
    invalidate: "cancel",
    recover: true,
    type: "text",
  });
});

test("authoritative admission consumes its captured preparation", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    type: "text",
  });
});

for (const newer of ["different", "same"]) {
  test(`admission preserves independent ${newer} capture`, async () => {
    await runJourney({
      automatic: false,
      captureFirst: true,
      newer,
      submissionMode: "after",
      type: "text",
    });
  });
}

test("captured option-like text is sent literally", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    contentCase: "flag",
    type: "text",
  });
});

test("plain text matching a working-directory file stays text", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    contentCase: "existing-path",
    type: "text",
  });
});

test("closed-panel Paste never sends to a preferred peer", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    closedPaste: true,
    type: "text",
  });
});

test("failed dispatch retains capture for deliberate retry", async () => {
  await runJourney({
    automatic: false,
    captureFirst: true,
    consume: true,
    failSubmission: true,
    submissionMode: "before",
    type: "file",
  });
});

test("changing recipient during a read captures afresh", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    peerChange: true,
    type: "text",
  });
});

test("pending explicit capture prevents automatic competition", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    explicitPending: true,
    type: "text",
  });
});

test("a stalled clipboard read permits deliberate recovery", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    readTimeout: true,
    type: "text",
  });
});

test("expired automatic read preserves queued explicit capture", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    readTimeout: true,
    timeoutReplacement: true,
    type: "text",
  });
});

test("empty automatic capture waits for deliberate recovery", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    invalidRead: "empty",
    type: "text",
  });
});

test("unsupported clipboard MIME never becomes a text share", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    invalidRead: "unsupported",
    type: "text",
  });
});

test("provided empty value invalidates pending automatic capture", async () => {
  await runJourney({
    automatic: true,
    captureFirst: false,
    consume: true,
    providedEmpty: true,
    type: "text",
  });
});

for (const automatic of [false, true]) {
  for (const explicitFailure of ["empty", "unsupported", "failed"]) {
    test(`A survives ${explicitFailure}, auto=${automatic}`, async () => {
      await runJourney({
        automatic,
        captureFirst: true,
        consume: true,
        explicitFailure,
        type: "text",
      });
    });
  }
}

for (const preference of [
  "enable",
  "disable",
  "disable-current",
  "disable-queued",
]) {
  test(`selection: preference ${preference}`, async () => {
    await runJourney({
      automatic: preference !== "enable",
      captureFirst: false,
      consume: true,
      explicitPending: preference === "disable-current",
      preference,
      type: "text",
    });
  });
}

for (const automatic of [false, true]) {
  for (const peerProjection of ["drop", "replace", "reorder"]) {
    test(`selection: ${peerProjection} P, auto=${automatic}`, async () => {
      await runJourney({
        automatic,
        captureFirst: false,
        consume: true,
        explicitPending: !automatic,
        peerProjection,
        type: "text",
      });
    });
  }
}

for (const automatic of [false, true]) {
  test(`selection: duplicate public gestures, auto=${automatic}`, async () => {
    await runJourney({
      automatic,
      captureFirst: false,
      duplicates: true,
      explicitPending: !automatic,
      type: "text",
    });
  });
}

for (const automatic of [false, true]) {
  test(`selection: preferred peer appears, auto=${automatic}`, async () => {
    await runJourney({
      automatic,
      captureFirst: true,
      consume: true,
      peerProjection: "appear",
      type: "text",
    });
  });
}
