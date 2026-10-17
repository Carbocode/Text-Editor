# Native Workspace

Use the repository skill [editor-change](.agents/skills/editor-change/SKILL.md) for changes to editor behavior, commands, snapshots, filesystem operations or their native adapters. The architecture contract is [docs/architecture.md](docs/architecture.md); changes to its ownership boundaries need an ADR.

- One `Session` per editor window. Frontends render its snapshots and send intentions. The synchronous engine and reducer are private to `application`.
- Commands → handlers → events → reducer → snapshots. Only the reducer changes document state; the application commit owns its revision. Session projections use the same reducer.
- Filesystem and storage adapters own I/O. Native adapters own focus, hover, selection, expanded folders and toolkit widgets. Editor rules, path validation, search, sorting, optimistic changes, queueing and error wording belong to Rust.
- Reuse `domain::path`, `AppSnapshot` sidebar methods, shared icons and confirmation text. A new command must make all exhaustive policy matches compile. Do not add wildcard command fallbacks.
- DRY and KISS: remove duplicate rules, use the smallest abstraction justified by current callers, and keep dependencies pointing inward. Do not introduce a generic framework for a single feature.
- Verify with `cargo xtask architecture` and `cargo xtask verify core`; run the adapter checks and relevant native smoke tests when that boundary changes. Build/run commands intentionally do not run tests.

Use standard Cargo formatting and warnings-denied Clippy. New behavior tests belong beside the owning module or in `application/src/tests`; toolkit regression drivers live in each app's test area. Preserve the actual failure/concurrency cases when migrating tests to a new API.
