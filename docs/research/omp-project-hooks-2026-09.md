# OMP project hook configuration

Research date: 2026-09-03

## Conclusion

OMP 18.0.10 exposes 45 runtime events to project-loaded extension factories. A factory placed in
`.omp/hooks/pre/*.ts` or `.omp/hooks/post/*.ts` is loaded through the current `ExtensionRunner`, so it can register
any event below with `pi.on(...)`. The `pre` and `post` directory names are discovery categories, not restrictions on
which events the module may register.

For new code, type the factory with `ExtensionAPI`. The legacy `HookAPI` type intentionally exposes a smaller subset
and omits newer extension-only events such as `input`, `tool_execution_*`, `user_bash`, and `user_python`.

## Project loading and configuration

Project-scoped automatic discovery is rooted at the current working directory. It does not walk ancestors for hooks:

- `.omp/hooks/pre/*.ts` and `.omp/hooks/post/*.ts`: native hook discovery. Importable `.ts` and `.js` files are loaded.
- `.omp/extensions/`: native extension discovery. Use this for new full-surface extensions.
- `.omp/config.yml#extensions` or `.omp/settings.json#extensions`: explicit project extension paths.
- `omp --hook PATH`: explicit hook path; currently an alias for `--extension`.
- `omp -e PATH` or `omp --extension PATH`: explicit extension path. Repeatable.

Explicit files support `.ts`, `.js`, `.mjs`, and `.cjs`. `--no-extensions` disables ambient project/user/plugin
discovery, but explicit `--hook` and `--extension` paths still load.

Installed CLI configuration exposes these related settings:

| Setting                               | OMP 18.0.10 default | Effect                                                                                                                         |
| ------------------------------------- | ------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| `extensions`                          | `[]`                | Additional extension paths. May be set in project `.omp/config.yml`.                                                           |
| `disabledExtensions`                  | `[]`                | Disables ambient extension modules by `extension-module:<derivedName>`. It does not additionally filter hook-capability files. |
| `extensionHandlers.toolCallTimeoutMs` | `30000`             | Active-work timeout for `tool_call` handlers. Time in OMP-owned dialogs is excluded.                                           |
| `statusLine.showHookStatus`           | `true`              | Shows hook status messages below the status line.                                                                              |

There is no separate event enable/disable list and no public `disabledHooks` setting in the installed configuration
schema. Events become active when loaded code registers handlers. To suppress all ambient project hooks, use
`--no-extensions`; to control individual project hooks, remove them from automatic discovery or load selected files
explicitly.

## Complete event catalog

“Mutating effect” means the handler return value changes runtime behavior. Events marked “observe” are notifications;
the handler may still call `ExtensionAPI` methods with their normal side effects.

### Input, provider, agent, and message lifecycle

| Event                     | When it fires                                                                       | Mutating effect                                                                                                        |
| ------------------------- | ----------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------- |
| `input`                   | Input is submitted; payload identifies `interactive`, `rpc`, or `extension` source. | Consume it with `handled`, or replace `text` and `images`.                                                             |
| `before_agent_start`      | After prompt submission, before the agent loop.                                     | Add a custom message or replace the turn's system prompt. Multiple prompt replacements chain.                          |
| `context`                 | Before each LLM call, with a deep copy of provider-bound messages.                  | Replace the messages sent to the provider without rewriting stored session messages.                                   |
| `before_provider_request` | Immediately before a provider request.                                              | Replace the provider request payload. Not emitted for `devin-agent`.                                                   |
| `after_provider_response` | After provider response receipt, before its stream body is consumed.                | Observe only.                                                                                                          |
| `agent_start`             | Agent loop starts for a user prompt.                                                | Observe only.                                                                                                          |
| `agent_end`               | Agent loop ends; `willContinue` identifies an already-scheduled continuation.       | Observe only.                                                                                                          |
| `session_stop`            | A main-session turn is about to settle. It does not run for task/subagent sessions. | Request another turn with context, or return a compatible block decision. OMP caps consecutive continuations at eight. |
| `turn_start`              | A model turn starts.                                                                | Observe only.                                                                                                          |
| `turn_end`                | A model turn ends, with its message and tool results.                               | Observe only.                                                                                                          |
| `message_start`           | A user, assistant, or tool-result message starts.                                   | Observe only.                                                                                                          |
| `message_update`          | Assistant output streams.                                                           | Observe only.                                                                                                          |
| `message_end`             | A message completes. The payload is a detached snapshot.                            | Observe only; mutate provider context with `context` or tool output with `tool_result` instead.                        |

### Tool and user-command lifecycle

| Event                     | When it fires                                                      | Mutating effect                                                                                                                                                                                             |
| ------------------------- | ------------------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `tool_call`               | Before a tool executes and before approval for model-issued calls. | Block with a reason or replace the execution input. Revised input is revalidated and is what scheduling, display, persistence, approval, and execution see. Input replacement does not apply to `computer`. |
| `tool_result`             | After a tool executes.                                             | Replace `content`, `details`, or `isError`. Handlers compose in registration order.                                                                                                                         |
| `tool_execution_start`    | Tool execution starts.                                             | Observe only.                                                                                                                                                                                               |
| `tool_execution_update`   | A tool publishes partial or streaming output.                      | Observe only.                                                                                                                                                                                               |
| `tool_execution_end`      | Tool execution finishes.                                           | Observe only.                                                                                                                                                                                               |
| `tool_approval_requested` | An approval-gated tool requests approval.                          | Observe only. Emitted only when an approval handler is registered.                                                                                                                                          |
| `tool_approval_resolved`  | An approval request is approved or denied.                         | Observe only. Emitted only when an approval handler is registered.                                                                                                                                          |
| `user_bash`               | The user invokes `!` or `!!`.                                      | Supply the complete `BashResult` and bypass normal execution.                                                                                                                                               |
| `user_python`             | The user invokes `$` or `$$`.                                      | Supply the complete `PythonResult` and bypass normal execution.                                                                                                                                             |

### Session lifecycle

| Event                    | When it fires                                  | Mutating effect                                                             |
| ------------------------ | ---------------------------------------------- | --------------------------------------------------------------------------- |
| `session_start`          | Initial session load.                          | Observe only.                                                               |
| `session_before_switch`  | Before new, resume, or fork session switching. | Cancel the switch.                                                          |
| `session_switch`         | After a session switch.                        | Observe only.                                                               |
| `session_before_branch`  | Before branching from an entry.                | Cancel, or branch without rewinding the in-memory conversation.             |
| `session_branch`         | After branching.                               | Observe only.                                                               |
| `session_before_compact` | Before context compaction.                     | Cancel or provide the complete custom compaction result.                    |
| `session.compacting`     | Before compaction summarization.               | Add summary context, replace the compaction prompt, or persist custom data. |
| `session_compact`        | After compaction.                              | Observe only.                                                               |
| `session_before_tree`    | Before navigating the session tree.            | Cancel or provide a custom branch summary.                                  |
| `session_tree`           | After session-tree navigation.                 | Observe only.                                                               |
| `session_shutdown`       | Process shutdown from `SIGINT` or `SIGTERM`.   | Observe and clean up.                                                       |

### Reliability, goals, credentials, and reminders

| Event                      | When it fires                                                                             | Mutating effect                                                         |
| -------------------------- | ----------------------------------------------------------------------------------------- | ----------------------------------------------------------------------- |
| `auto_compaction_start`    | Automatic compaction starts.                                                              | Observe only.                                                           |
| `auto_compaction_end`      | Automatic compaction finishes, skips, aborts, or fails.                                   | Observe only.                                                           |
| `auto_retry_start`         | A provider retry starts.                                                                  | Observe only.                                                           |
| `auto_retry_end`           | The retry sequence ends.                                                                  | Observe only.                                                           |
| `retry_fallback_applied`   | Auto-retry switches to a configured fallback model/provider.                              | Observe only.                                                           |
| `retry_fallback_succeeded` | A request succeeds on the fallback model.                                                 | Observe only.                                                           |
| `ttsr_triggered`           | A TTSR rule interrupts generation.                                                        | Observe only.                                                           |
| `todo_reminder`            | Todo reminder logic finds unfinished todos.                                               | Observe only.                                                           |
| `goal_updated`             | Goal-mode state changes.                                                                  | Observe only.                                                           |
| `credential_disabled`      | Authentication storage soft-disables a credential after an error such as `invalid_grant`. | Observe only; user removals and duplicate deduplication do not emit it. |

### MCP and resource discovery

| Event                | When it fires                                                                                                                     | Mutating effect                                                                                                                                                         |
| -------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `mcp_notification`   | Any MCP JSON-RPC notification arrives, after OMP handles known list/resource/prompt updates. Unknown custom methods are included. | Observe only. OMP buffers up to 100 notifications before the first subscriber and drops the oldest on overflow.                                                         |
| `resources_discover` | Declared for startup or reload resource-path contribution.                                                                        | Return extra skill, prompt, or theme paths. Current OMP documentation reports no normal `AgentSession` callsite, so this event is presently inert in ordinary sessions. |

## Runtime rules that matter

- Extensions and hooks run in process, unsandboxed, with the same permissions as OMP.
- Registration order is deterministic: native extension modules, discovered hook factories, installed plugin entries,
  then explicit configured paths.
- Multiple handlers run in extension order. `tool_result` handlers see earlier result patches. `tool_call` handlers see
  the original input; the last returned replacement input wins.
- A blocking `tool_call` short-circuits execution. `tool_call` exceptions and timeouts fail closed.
- General handler failures are surfaced as extension errors rather than stopping subsequent handlers.
- `mcp_notification` isolates handlers, so one throwing handler does not prevent later subscribers.

## Minimal project example

```ts
// .omp/hooks/pre/project-policy.ts
import type { ExtensionAPI } from "@oh-my-pi/pi-coding-agent";

export default function projectPolicy(pi: ExtensionAPI): void {
  pi.on("tool_call", async (event) => {
    if (
      event.toolName === "bash" &&
      event.input.command === "forbidden-command"
    ) {
      return { block: true, reason: "Blocked by project policy" };
    }
  });
}
```

No `.omp/config.yml` entry is required for an automatically discovered file.

## Sources

- Installed `omp` 18.0.10: `omp --help`, `omp config --help`, and `omp config list --json`, run 2026-09-03.
- OMP bundled docs: `omp://config-usage.md`, `omp://extension-loading.md`, `omp://extensions.md`,
  `omp://hooks.md`, and `omp://skills/authoring-hooks.md`.
- [OMP 18.0.10 `ExtensionAPI` event types](https://github.com/can1357/oh-my-pi/blob/v18.0.10/packages/coding-agent/src/extensibility/extensions/types.ts)
- [OMP 18.0.10 shared lifecycle and result types](https://github.com/can1357/oh-my-pi/blob/v18.0.10/packages/coding-agent/src/extensibility/shared-events.ts)
