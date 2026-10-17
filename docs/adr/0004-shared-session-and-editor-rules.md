# ADR 0004: One session and shared editor rules

Status: accepted. Supersedes the frontend dispatch API in ADR 0002.

Native applications keep their platform controls, but editor behavior has one owner. Each window drives `application::Session`. Its synchronous engine is private, so an adapter cannot bypass scheduling, projected state, deferred commands or error publication.

The pipeline is command → handler → events → reducer → snapshot. `command.rs` exhaustively defines rebasing, scheduling, busy behavior, diagnostics and composite steps. Handlers perform validated I/O through ports. The reducer applies successful facts. Composite commands commit successful steps before returning a later failure: save-and-close never closes after a failed save, and save-all retains completed saves.

Relocations project the same event on a copy of the state. Edits typed meanwhile are projected and replayed in order; queued paths are rebased on failure. Other file operations cannot overlap a relocation or deletion. Deletion rejects typing until confirmation. A second move/rename/delete while a session worker runs is rejected consistently on all platforms; the UI's drop validation reflects this policy. Polling never waits for a running filesystem job.

Snapshot paths are already projected. Frontends must not project them again. Native editor identity follows `editor_generation`, which survives relocation. Shared sidebar row construction, name filtering, icon associations and confirmation messages prevent frontend drift. Swift deltas omit unchanged shared lists; full snapshots remain available explicitly.

The file-name filter and text search share the debounced worker, cancellation and latest-generation policy. Watcher reconciliation remains a separate scan because it must reconcile a specific state revision; stale results are discarded.

The compiler protects private state and exhaustive command policies. `cargo xtask architecture` checks the dependency graph, and CI runs it together with formatting, lint and behavioral tests. `AGENTS.md` and the project skill guide future implementations, but executable checks remain the enforcement mechanism.
