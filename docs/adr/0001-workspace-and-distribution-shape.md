---
status: accepted
---

# Use a layered Cargo workspace and separate plugin distribution

The project will use a virtual Cargo workspace whose crates own wire formats, cryptography, Connections, Sharing, local control, Linux adapters, and the executable. The dependency direction is fixed from the executable and adapters toward the core, never from the core toward Linux or Omarchy.

The source repository will publish the native Arch package and export `packaging/omarchy-plugin/` to a small plugin repository. Omarchy requires `manifest.json` at the plugin repository root, clones the whole repository, and runs no installer. Keeping the plugin artifact separate avoids shipping Rust source, test infrastructure, and upstream material to every user while preserving one source of truth for local releases.

Future release automation may publish prebuilt native artifacts, but hosted CI is outside the current scope and never becomes the only installation path. The full repository must always build the binary with locked Cargo inputs, and release tooling must produce an allowlisted runtime-source bundle that builds without test suites, test tools, upstream reference trees, simulators, or Android tooling. The same allowlist supports a Git partial-clone and sparse-checkout route for users who prefer to build from a repository checkout. The Omarchy plugin remains independently installable, checks for a compatible native binary and control protocol, and reports a useful installation or compatibility error instead of downloading or installing the binary itself.
