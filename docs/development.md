# Development

Rust 1.99.0 is pinned in rust-toolchain.toml, and Cargo.lock pins crate resolution. Default workspace members avoid GUI dependencies. The `core` xtask target selects domain, filesystem, storage, application and syntax explicitly, including syntax tests that plain `cargo test` does not select by default.

Material Icon Theme SVGs, 64 px PNGs, and the association manifest are bundled in `assets/material-icon-theme`. To update them, prepare an upstream checkout as described in `tools/update_material_icons.py`, then run `python3 tools/update_material_icons.py /path/to/vscode-material-icon-theme`. The script requires `rsvg-convert` and regenerates the GNOME embedded icon lookup table. Review the pinned upstream revision in `ATTRIBUTION.md` when updating.

## Terminal commands

Use `cargo xtask help` to list commands and `cargo xtask doctor` to inspect local tools, or `cargo xtask doctor --strict macos|gnome|kde` for one frontend. Commands use the existing Rust xtask runner; Node.js and npm are not required.

For each frontend (`macos`, `gnome` or `kde`), choose the work needed:

```sh
cargo xtask build macos       # compile and package; no tests or launch
cargo xtask start macos       # open the existing build; no frontend compilation
cargo xtask run macos         # build + start; no tests
cargo xtask test macos        # Rust bridge tests + build prerequisites + Swift smoke tests
cargo xtask verify macos      # fmt + check + lint + test; no normal editor launch
```

`start` fails with a build instruction if the executable is missing. It deliberately opens the last build, so use `run` after changing source. macOS and GNOME launch a new instance, preserving existing sessions. Cargo may still compile the xtask runner itself before any command if it has changed.

The same tasks can target smaller components:

```sh
cargo xtask check core        # type-check shared Rust crates
cargo xtask lint core         # Clippy, warnings denied
cargo xtask test core         # shared Rust tests, without GUI dependencies
cargo xtask verify core       # fmt + check + lint + test
cargo xtask build devtools    # collector + console executables
cargo xtask test devtools     # protocol, producer, collector and console Rust tests
cargo xtask test xtask        # command composition and Qt discovery regression tests
cargo xtask fmt               # check formatting across the workspace
```

`build`, `check`, `lint`, `test` and `verify` accept `core`, `macos`, `gnome`, `kde`, `devtools` or `xtask`. `start` and `run` accept only editor frontends. `check macos` and `lint macos` check the Rust bridge; `build macos` compiles Swift, and `test macos` also executes the seven Swift smoke tests. `test gnome` and `test kde` run the selected Rust package's tests; native GUI smoke modes remain explicit as documented below. Tasks stop at the first failed step.

Inspect composition without running it:

```sh
cargo xtask run macos --dry-run
cargo xtask verify core --dry-run
```

The dry run lists task steps; the macOS build and smoke steps are shell scripts. `apps/macos/build.sh` compiles and packages, `apps/macos/start.sh` launches, and `apps/macos/test.sh` compiles and executes smoke tests against an already built bridge. `apps/macos/run.sh` composes build and start. The normal frontend builds still include Developer Tools, but never execute tests.

Existing action-target commands (`build-macos`, `run-macos`, `build-gnome`, `run-gnome`, `build-kde`, `run-kde`, `check-core`) remain aliases of the corresponding two-word commands. New tasks also support that spelling, for example `test-macos` and `verify-core`.

## Explicit GUI smoke modes

GNOME's `NATIVE_WORKSPACE_SMOKE=1` and `NATIVE_WORKSPACE_SMOKE_WORKSPACE=/path/to/disposable-fixture` enable native diagnostic and edit/save smoke paths when launching the built executable. KDE's `--smoke-sidebar` and `--smoke-drag` modes require disposable fixtures; see [KDE development](kde.md) for contents and Qt style variants. Set these only for deliberate GUI verification; no normal run task injects them. Headless Linux GUI checks need a display server such as Xvfb.

## Development behavior

The initial vertical slice covers opening a directory, listing files, opening, editing, and saving UTF-8 documents. The frontends now collect queued `notify` events every 250 ms and reconcile periodically for external changes. Clean open documents reload automatically. If local edits and disk content diverge, normal Save refuses to overwrite the disk; the user can reload or explicitly overwrite it. The diagnostic counter remains in the core and smoke tests, but is hidden from normal windows.

Normal frontend launches open a SQLite metadata database in the platform application-data directory. Set `NATIVE_WORKSPACE_STATE_DB=/absolute/path/metadata.sqlite3` to use an isolated database. `cargo test` uses temporary files; GUI smoke modes use in-memory metadata. Migration files live in `crates/storage/src/migrations`, with schema version tracked by SQLite `PRAGMA user_version`.

The sidebar search matches relative paths and UTF-8 file contents throughout the directory tree, ignoring case and surrounding whitespace in the query. Every content occurrence has a separate result with line, column, and a short preview; selecting it opens the file and highlights that text. Unsaved document buffers take precedence over disk content. Files above 1 MiB and unreadable files are skipped for content search, while their paths remain searchable. An empty query shows every listed document. Search runs in a worker and does not use SQLite.

Normal run commands launch the selected editor. Developer Tools open only through the editor button and run in separate service/console processes. The console interface is in English. See [implementation and verification](devtools/IMPLEMENTATION.md) for standalone diagnostics commands, protocol budgets and coverage still pending.

## Deletion verification

Use a disposable workspace with a nested folder and at least two files. In Files or Search, choose **Delete…** from a file or folder context menu. Cancel and verify that the disk, editor and tree remain unchanged. Confirm **Delete Permanently** to remove an unmodified file or populated folder; its open documents must close, while unrelated documents remain open. The entry must disappear from both Files and Search, including when a search was already running.

Modify a nested document, switch to another file, then try deleting its parent folder. The operation must fail with an instruction to save modified documents, preserving both the disk files and unsaved buffers. Save the document and retry. While a deletion is running, the editor temporarily prevents typing; completion restores editing even after a filesystem error. Deletion does not use the system Trash and cannot be undone. A filesystem error can leave a partially removed directory; the normal watcher refresh reconciles the remaining entries without discarding buffers.

The shared deletion tests cover inactive unsaved descendants, unrelated dirty buffers, successful subtree removal, search invalidation, background execution, filesystem failure, and symlink boundaries. The Swift bridge smoke test also exercises deletion rejection and success through UniFFI.

## Document tabs verification

Open three files in a different order from their sidebar order. Tabs must keep the order in which they were opened; reopening a tab selects it without duplicating it. Edit two files and switch between them: both buffers and dirty markers must remain intact. Close an inactive clean tab and confirm that the active document stays selected. Close the active tab and confirm that its right neighbor (or the last remaining tab) becomes active.

Close a modified tab and exercise Cancel, Save, and Discard Changes. Cancel preserves the buffer; Save closes only after a successful save; Discard closes without writing and reopening loads the disk content. Change the file externally before choosing Save: the conflict must leave the tab and local edits intact. Save All must save inactive dirty tabs too, stop at the first failure, and accurately reflect files already saved. Rename or move an open file or its parent folder and confirm that tabs retain their order and buffers. Delete a clean active file and confirm that its tab disappears and a remaining tab becomes active.

After editing one document, switch to another with identical text and try Undo: it must never replay the previous document's edits. The shared native editor clears its undo history on a document switch. Edit again, then rename that document: its selection and undo history must remain intact. In GNOME, click an already selected tab repeatedly and verify that it stays highlighted. On macOS, navigate to a search match, then switch or close its tab: the new document must not inherit the old search selection. The tab strip must leave most of the workspace height available to the editor.
