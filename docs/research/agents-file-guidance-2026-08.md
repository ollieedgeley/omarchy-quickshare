# Agent instruction file guidance as of August 2026

Research cutoff: 2026-08-31

## Question

What belongs in a repository `AGENTS.md`, and how should it be written so coding agents can act reliably without loading a project handbook into every task?

## Source currency

The cutoff check used official documentation and its source history where available. The AGENTS.md specification README was last changed on [2025-12-10](https://github.com/agentsmd/agents.md/commit/557da8b39c6f5b4dee2239df09a6ab97a82ff4df). The Codex instruction loader had a relevant change on [2026-08-25](https://github.com/openai/codex/commit/5ca4175295220d2c5e9494b16e47e4be7720d6b5). GitHub's instruction-writing guidance was updated on [2026-07-13](https://github.com/github/docs/commit/7d77302125d913822c0292873a3ad9b40e437242). Anthropic and Cursor did not display publication dates, so their current official pages were reviewed at the cutoff.

## First-party findings

The [AGENTS.md specification](https://agents.md/) defines plain Markdown with no required schema. It recommends project context, exact build and test commands, code conventions, testing guidance, security concerns, and contribution rules. The nearest nested file takes precedence, and an explicit user instruction overrides repository guidance. Listed checks may be run by an agent, so planned commands must not be presented as available commands.

The [OpenAI Codex guide](https://developers.openai.com/codex/guides/agents-md) and [Codex agent-loop description](https://openai.com/index/unrolling-the-codex-agent-loop/) show that Codex loads project instructions from the repository root toward the working directory. The combined project-instruction budget defaults to 32 KiB. More specific guidance should therefore live close to its scope, while the root should contain only repository-wide rules.

The [Claude Code memory guide](https://code.claude.com/docs/en/memory) gives the clearest writing constraint for an equivalent persistent file: target fewer than 200 lines, use headings and bullets, and make instructions specific enough to verify. Keep facts needed in every session, such as commands, conventions, layout, and always-do rules. Move multi-step or area-specific procedures to skills or path-scoped guidance. Imported files still consume startup context.

Anthropic also distinguishes guidance from enforcement. Persistent Markdown shapes agent behavior but does not guarantee it. A formatter, lint, test, Make gate, or hook should enforce any rule that must always hold.

GitHub's [response customization guidance](https://docs.github.com/en/copilot/concepts/prompting/response-customization) says repository-wide instructions accompany every request and should be short, self-contained, and broadly applicable. Its [instruction-writing tutorial](https://docs.github.com/en/copilot/tutorials/customize-code-review) recommends focused imperative rules, distinct headings, bullets, and concrete examples. It warns that vague, conflicting, or excessive instructions reduce reliability.

Cursor documents repository rules as persistent context but has historically exposed narrower `AGENTS.md` scoping than Codex and GitHub. Essential rules must therefore remain in the root even when nested files later provide local detail. See [Cursor rules documentation](https://docs.cursor.com/context/rules-for-ai).

## Applied design

These conclusions are project decisions inferred from the sources:

- Treat the root file as an operating guide, not a handoff, project specification, mission statement, or research report.
- Keep the root near 100 to 160 lines. The project permits 800 lines, but that is a safety ceiling rather than a target.
- State the current authority, the normal work loop, hard repository-wide constraints, and exact commands that exist.
- Route conditional work with actionable triggers such as "changing gates: read this document first." A bare document list is insufficient.
- Keep product goals, crate maps, simulator inventories, lint catalogs, hook algorithms, packaging layouts, and research evidence in their authoritative documents.
- Do not duplicate linked policy. Update the owner and its routing entry together when a path or trigger changes.
- Use nested instructions only for real subtree-specific behavior. Do not rely on nesting for essential safety or workflow rules.
- Enforce deterministic rules in code. Use `AGENTS.md` to tell the agent what gate to run and when.
- Add persistent rules after observed repeated failures or for facts that cannot be inferred from the repository.

## Rewrite checks

The rewritten root file should let an agent answer these questions without reading every document:

1. What am I currently authorized to change?
2. Which document applies to this task?
3. How do I locate code and preserve existing work?
4. What TDD and verification loop applies?
5. Which limits and safety rules apply everywhere?
6. What exact narrow command should I run now?
7. What must pass before a commit or push?

It should not repeat the answers to branch-specific product, architecture, protocol, simulator, lint, hook, packaging, or release questions. The routing entry should tell the agent where to load those answers when needed.
