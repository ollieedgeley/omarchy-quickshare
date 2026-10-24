import { createExecutionMutationTracker } from "./execution-mutations.js";

function resultText(result) {
  return result.content
    .filter((chunk) => chunk.type === "text")
    .map((chunk) => chunk.text)
    .join("\n\n");
}

export function registerExecutionChecks(pi, handleToolResult) {
  const tracker = createExecutionMutationTracker();
  const check = (event, ctx) =>
    handleToolResult(pi, event, {
      cwd: ctx?.cwd,
      mutationTracker: tracker,
    });
  const deferred = { content: [], toolName: "deferred-execution" };

  pi.on("tool_call", (event, ctx) => {
    if (event.toolName === "bash" || event.toolName === "eval") {
      tracker.capture(ctx?.cwd);
    }
  });
  pi.on("tool_result", check);

  // Delayed writes can land after an execution tool returns a background job.
  pi.on("agent_end", async (_event, ctx) => {
    const result = await check(deferred, ctx);
    if (result) {
      pi.sendMessage(
        {
          content: resultText(result),
          customType: "post-edit-check",
          display: true,
        },
        { deliverAs: "followUp", triggerTurn: true },
      );
    }
  });
  // Unlike agent_end, session_stop waits for owned background jobs to settle.
  pi.on("session_stop", async (_event, ctx) => {
    const result = await check(deferred, ctx);
    if (result) {
      return { additionalContext: resultText(result), continue: true };
    }
    return globalThis.undefined;
  });
}
