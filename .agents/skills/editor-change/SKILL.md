---
name: editor-change
description: Implement or review editor features in Native Workspace across the Rust core and native adapters, preserving shared command, state, path and execution rules.
---

Read `AGENTS.md` and `docs/architecture.md` from the repository root. Use this skill for changes to editor behavior, commands, workspace operations, snapshots or their native adapters.

Locate the authoritative behavior before editing. `command.rs` defines execution policy, `handlers/` validates and performs I/O, `state.rs` applies facts, and `Session` publishes the frontend view. Prefer extending this path over adding another coordinator, cache, worker or adapter-side rule. Keep a helper local until more than one real caller needs it.

For a new command, decide every exhaustive policy in `command.rs`: diagnostics, rebasing, composite steps, refresh behavior, scheduling and behavior during background work. Give filesystem work an injected `WorkspaceFilesystem` operation. Produce an event only after the operation succeeds. Composite operations preserve successful earlier commits when a later step fails.

Keep UI state (focus, hover, selection, expanded folders and dialogs) in the native adapter. Use shared paths, sidebar rows, icon associations, confirmation text and error messages. Treat paths in snapshots as already projected. Use `editor_generation` to decide when to reset an editor; do not infer identity from renamed paths or tab counts. Every frontend owns one `Session`; Swift reaches it through UniFFI.

Test behavior at its owning boundary. Cover the relevant failure too: a save conflict keeps its tab open; a rejected relocation restores paths and keeps edits and queued saves; an older search or scan cannot overwrite newer state. Use the common hooks in `application/src/tests/mod.rs` for core tests. Native smoke tests should drive the same asynchronous API used by the app, waiting for observable completion rather than adding synchronous production shortcuts.

Run `cargo xtask architecture` and `cargo xtask verify core`. For an adapter change, also run `cargo xtask verify macos|gnome|kde` as appropriate and the affected GUI smoke mode from `docs/development.md`. Report unavailable platform checks accurately. Update the architecture or an ADR when ownership or a public contract changes.
