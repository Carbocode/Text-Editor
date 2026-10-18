# Architecture

The editor uses a layered command/event architecture with native presentation adapters. Its dependency direction is enforced by `cargo xtask architecture`.

```text
domain ← filesystem, storage
    ↑          ↑
    └── application::Session ← GTK, CXX-Qt, UniFFI ← SwiftUI
syntax ← native text views
application → optional diagnostic producer → diagnostic protocol
separate diagnostic service/UI → diagnostic protocol
```

Each editor window owns one `Session`. The synchronous `Application` and `AppState` are private to the application crate. Frontends send commands and render immutable snapshots; they do not execute editor rules, maintain backend queues, project pending paths, or manage worker failures themselves. Native presentation state—focus, selection, hover, scroll position, expanded folders and dialogs—stays in the native layer. See [ADR 0004](adr/0004-shared-session-and-editor-rules.md).

## Application modules

- `command.rs`: the public intentions and exhaustive policies for diagnostics, path rebasing, composite steps, re-searching, scheduling and commands received while busy.
- `handlers/`: workspace scanning/reconciliation, file operations, documents/tabs and search delivery. Handlers validate and produce events after successful I/O through injected ports.
- `application.rs`: the command pipeline and checked revision commit. Successful steps in a composite operation remain committed when a later step fails.
- `event.rs`, `state.rs`: facts and their reducer. `DocumentTable` maintains sorted entries and binary lookup; its mutators own summary-cache invalidation. Shared lists use copy-on-write.
- `session.rs`: the only frontend runtime. It schedules filesystem operations, projects relocations and edits through the same reducer, replays deferred commands, rolls paths back on failure and publishes errors until the next successful user command.
- `snapshot.rs`, `sidebar.rs`, `icons.rs`: shared immutable views, tree ordering and icon associations. `editor_generation` identifies the shown document independently of its path.
- `worker.rs`, `search.rs`, `path_filter.rs`: the shared debounced worker and the two query services. Cancellation and generation checks discard superseded work.

## I/O and concurrency

`WorkspaceFilesystem` is an injected port with explicit operations, implemented by `RealFilesystem`. It lists regular UTF-8-named entries, skips symlinks and validates ancestor directories for editor operations. Saves compare the disk version with the loaded baseline; conflicts require explicit reload or overwrite. Removed loaded files remain visible as conflicts. Open external files share the document table and save/conflict rules; normal workspace search and the Files sidebar exclude them, while explicit Open Editors search includes them. Closing their tabs removes their entries. Dirty buffers prevent changing workspaces and deleting their subtree. SQLite stores recent-workspace metadata and the last confirmed session layout, never document text.

A session polls worker, watcher, text-search and name-filter results through one `poll` API. Native adapters use a fast timer (16 ms) while work is active and an idle interval (250 ms) otherwise. The watcher starts a background reconciliation after filesystem events, periodically every 30 seconds, or every two seconds without a watcher. A result from an older state revision cannot replace newer edits or paths.

Rename, move, deletion and explicit refresh execute on the session worker. Relocation paths appear immediately; a progress indicator appears after 300 ms. Editing is projected during relocation and replayed with queued saves/opens; failed relocation rebases those commands to the original paths. Deletion and OS file transfers pause editing via `activity.editing_paused`. OS file opens/transfers and application-icon requests run ordered composite steps on the worker; successful earlier steps remain committed if a later file fails. Transfers preserve open external buffers through the relocation reducer and publish destination rows after confirmation. Concurrent destructive/relocating requests are rejected. `OpenPaths` resolves file/folder kinds through the filesystem port and executes the existing workspace/file commands. It pauses editing, including while queued behind another job and during replay, so a folder switch cannot discard newly typed text. Native launch delegates buffer only OS deliveries until their windows exist; Session owns command ordering and failures. See [ADR 0006](adr/0006-system-open-requests.md). Opening a document and ordinary saves are currently inline; moving them to background execution is a command-policy decision, never a frontend worker.

## Search and presentation

Text search debounces for 150 ms, prioritizes open buffers and shallow files, and by default honors ignore files and skips `.git`. It uses up to eight worker threads, skips non-UTF-8 or >1 MiB contents, streams current-generation matches and caps results at 20,000. Results carry Unicode-scalar and UTF-16 positions. Unsaved buffer edits update their matches; relocation preserves matches under the new path; explicit refresh re-searches. Empty queries cancel immediately. Name filtering runs through the same debounced worker and returns matching files/folders and their ancestors.

`SearchOptions` owns case, whole-word and regex matching, include/exclude globs,
ignore-file use and the Open Editors scope. Only that explicit scope includes open
external documents. Invalid expressions/patterns publish an inline `search_error`
and cancel older generations. Cancellation keeps partial results marked incomplete;
refresh starts a new generation. Buffer matching uses the same settings as workers.
Relocations with active path filters re-search so old scope membership cannot persist.

Replacement preparation runs on the Session worker without writing. The reducer
publishes an immutable preview containing exact before/after contents and shared
review snippets. Apply is exclusive: it checks all disk/buffer baselines before any
edit and checks each file again before writing. Open buffers receive ordinary
`DocumentChanged` events and retain their save baseline; closed files are written
through `WorkspaceFilesystem` and invalidate their matches. Each successful file is
committed separately, so a later failure retains prior successes. Search is restarted
after applying, including after a partial failure. A new query, settings or workspace
clears a pending preview; intervening buffer/disk changes reject it on apply. Native
adapters own disclosure controls, replacement drafts and preview dialogs, while
Rust owns edit ranges, capture expansion, case preservation and confirmation wording.

Snapshot lists and text are shared with `Arc`. A changed dirty flag invalidates document summaries; later typing that leaves summaries unchanged preserves their identity. GTK/Qt skip unchanged conversions. UniFFI sends `WorkspaceUpdate`: omitted lists are unchanged, while `snapshot()` explicitly sends all lists. Qt search occurrences use records with numeric offsets and a `QAbstractListModel`; syntax spans are structured records.

Sidebar rows are ordered in Rust: folders first, then case-insensitive name with an exact-name tie-break. Native adapters consume these rows or direct children; toolkit tree models may materialize collapsed branches lazily. Workspace entry paths use `DocumentId::parse` and `domain::path`; external document identities use canonical absolute paths decoded by `DocumentId::from_document_path` (see [ADR 0005](adr/0005-external-document-identities.md)); Swift calls the same functions through UniFFI.

## Session persistence and startup

Schema 2 stores the last workspace, ordered tab identities, active document and
preview in one metadata transaction. Application command execution checkpoints
confirmed layouts, including partial composite successes; Session projections do
not write metadata. Metadata failures retain editor state and retry on the next
user command. Polling and typing without a layout change do not write metadata.

`RestoreSession` runs exclusively on the shared worker, using normal workspace and
document operations and reducer events. It skips unavailable tabs, reports the
first error and preserves the saved layout until a user changes it. A missing root
leaves Home visible. Initial OS requests precede restoration; restoration is a
no-op after another workspace has opened and is never queued behind other work.
Native adapters present the restored workspace and own quit confirmations using
shared wording. `PrepareToQuit` rejects running work and optionally executes ordered
Save All steps before allowing termination. Saved tab layout survives discard;
unsaved text and native presentation state are not persisted. See
[ADR 0007](adr/0007-session-restoration.md).

## Integrated terminals

The existing Session owns transient terminal tabs and a separate terminal snapshot
revision. `terminal.rs` reduces terminal lifecycle events independently of document
state. The `terminal` I/O adapter owns PTY threads and the Alacritty emulator. Frontends draw
immutable visible grids; they never parse terminal output or own shell processes.
Session polls terminal updates even while filesystem work is active. See
[ADR 0008](adr/0008-integrated-terminals.md).

## Tabs and errors

Open tabs have visual order and at most one replaceable clean preview. Explicit opening, editing, saving, keeping open or reordering promotes the preview. Renames retain editor generation; switching documents advances it. Closing and deletion choose the visual neighbor. `SaveAndCloseDocument` saves before closing and stops on failure. `SaveAllDocuments` follows tab order and retains completed saves if another conflicts.

`AppError` owns error wording. A session snapshot carries the last failure; polls and deferred replay do not hide a relocation failure. Adapters render the text rather than classify errors by their wording. A caught worker panic returns the application and retains unsaved buffers.

## Extending the editor

Follow [the project skill](../.agents/skills/editor-change/SKILL.md) and [AGENTS.md](../AGENTS.md). Put behavior at its owning boundary; keep native widgets native. `cargo xtask verify core` includes dependency rules, formatting, Clippy and behavior tests. Adapter changes also require that platform's verification and the relevant [GUI smoke tests](development.md). No automatic check proves architectural perfection; these constraints make common regressions fail early and keep ownership reviewable.
