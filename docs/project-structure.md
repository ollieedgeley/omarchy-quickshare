# A structure that scales

Use this when deciding where work belongs, whether to split a component, or when an existing layout needs to change.

## Organize around responsibilities

- Group code that changes for the same reason. Give each component a purpose a reader can explain without listing its files.
- Keep the knowledge needed for an ordinary change close together. A feature scattered through many folders and a folder containing unrelated features both deserve review.
- Expose a small interface around a coherent responsibility. Keep implementation details behind it so callers need less knowledge of the internals.
- Separate components when their dependencies, lifecycle, verification or release needs differ. A folder, module, package and process are different boundaries; choose the smallest one that solves the actual problem.
- Keep runtime code independent of test environments and development tools. Place fixtures and test support where their users can find them without adding test dependencies to the shipped application.
- Make dependencies understandable and avoid cycles. When components must know too much about each other, reconsider ownership before adding another abstraction.

## Let the layout evolve

Start with the structure the current work needs. Add a directory or package when a real responsibility earns it. Future possibilities are reasons to leave room for change, not to create empty layers.

Split growing components along responsibilities that can be understood and checked independently. Prefer names that explain their purpose. Moving functions to a generic bucket or numbered fragment does not establish a useful boundary.

Move, merge or rename files when the evidence supports a clearer design. Preserve external compatibility where promised, but allow internal paths and ownership to change. Refactoring is normal maintenance.

## Before adding or splitting

1. Name the responsibility and its callers. State why the existing owner fits, or identify the specific problem with keeping the work there.
2. Compare the smallest useful options. Choose a boundary that reduces unrelated changes or unnecessary build/test work; explain any new dependency or indirection it introduces.
3. Identify the focused check for the behaviour and the interfaces that need wider verification. Finish with a command or a concrete test location, not just a new folder name.
4. After the change, confirm that imports, test discovery, packaging inputs and relevant documentation follow the new ownership. Report the improvement and any added maintenance cost.

Keep the explanation proportionate to the change.

## Review triggers

Reassess when unrelated changes repeatedly touch the same component, a reader must cross many files to understand one behaviour, small edits rebuild or retest unrelated work, or a boundary mostly forwards calls without hiding useful detail.

A split succeeds when understanding or focused verification improves, not merely when a counter passes.
