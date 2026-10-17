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
cargo xtask verify macos      # architecture + fmt + check + lint + test; no normal editor launch
```

`start` fails with a build instruction if the executable is missing. It deliberately opens the last build, so use `run` after changing source. macOS and GNOME launch a new instance, preserving existing sessions. Cargo may still compile the xtask runner itself before any command if it has changed.

The same tasks can target smaller components:

```sh
cargo xtask check core        # type-check shared Rust crates
cargo xtask lint core         # Clippy, warnings denied
cargo xtask test core         # shared Rust tests, without GUI dependencies
cargo xtask verify core       # architecture + fmt + check + lint + test
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

GNOME's `NATIVE_WORKSPACE_SMOKE=1` and `NATIVE_WORKSPACE_SMOKE_WORKSPACE=/path/to/disposable-fixture` enable native startup and edit/save smoke paths when launching the built executable. KDE's `--smoke-sidebar` and `--smoke-drag` modes require disposable fixtures; see [KDE development](kde.md) for contents and Qt style variants. Set these only for deliberate GUI verification; no normal run task injects them. Headless Linux GUI checks need a display server such as Xvfb.

## Development behavior

The initial vertical slice covers opening a directory, listing files, opening, editing, and saving UTF-8 documents. The frontends now collect queued `notify` events every 250 ms and reconcile periodically for external changes. Clean open documents reload automatically. If local edits and disk content diverge, normal Save refuses to overwrite the disk; the user can reload or explicitly overwrite it. All frontends use the shared Session runtime.

Normal frontend launches open a SQLite metadata database in the platform application-data directory. Set `NATIVE_WORKSPACE_STATE_DB=/absolute/path/metadata.sqlite3` to use an isolated database. `cargo test` uses temporary files; GUI smoke modes use in-memory metadata. Migration files live in `crates/storage/src/migrations`, with schema version tracked by SQLite `PRAGMA user_version`.

The Search sidebar matches UTF-8 file contents; the Files field filters entry names. Both ignore case and surrounding whitespace. Every content occurrence has a separate result with line, column, and a short preview; selecting it opens the file and highlights that text. Unsaved document buffers take precedence over disk content. Files above 1 MiB and unreadable files are skipped for content search, while their paths remain searchable. An empty content query clears matches; an empty name filter restores the complete file tree. Search runs in a worker and does not use SQLite.

Normal run commands launch the selected editor. Developer Tools open only through the editor button and run in separate service/console processes. The console interface is in English. See [implementation and verification](devtools/IMPLEMENTATION.md) for standalone diagnostics commands, protocol budgets and coverage still pending.

## Deletion verification

Use a disposable workspace with a nested folder and at least two files. In Files or Search, choose **Delete…** from a file or folder context menu. Cancel and verify that the disk, editor and tree remain unchanged. Confirm **Delete Permanently** to remove an unmodified file or populated folder; its open documents must close, while unrelated documents remain open. The entry must disappear from both Files and Search, including when a search was already running.

Modify a nested document, switch to another file, then try deleting its parent folder. The operation must fail with an instruction to save modified documents, preserving both the disk files and unsaved buffers. Save the document and retry. While a deletion is running, the editor temporarily prevents typing; completion restores editing even after a filesystem error. Deletion does not use the system Trash and cannot be undone. A filesystem error can leave a partially removed directory; the normal watcher refresh reconciles the remaining entries without discarding buffers.

The shared deletion tests cover inactive unsaved descendants, unrelated dirty buffers, successful subtree removal, search invalidation, background execution, filesystem failure, and symlink boundaries. The Swift bridge smoke test also exercises deletion rejection and success through UniFFI.

## Workspace hub verification

In each frontend, open a workspace without opening a document. The editor area must show **No Document Open**, an **Open Folder…** action and keyboard shortcuts, with no text caret, line numbers or minimap. Resize the window to its minimum size and verify that the hub remains readable by scrolling. Open a file from the sidebar: the hub must give way to the editor. Close the last tab and verify that the hub returns; repeat after deleting the last clean open file. The sidebar must remain usable throughout. Check the displayed Open Folder, Save and Save All shortcuts (Command on macOS, Control on Linux, including GTK/Qt when running on macOS); Save shortcuts apply after opening/editing documents.

## OS file drop verification

With a disposable workspace open, drag several local files (including a name with
spaces/Unicode and two files with the same basename) from the file manager onto
the editor and tab strip. They must open as permanent, independently editable
tabs. The workspace tree and search must not list external tabs. Edit an external
file, save it and check its original disk path. Change it on disk and verify that
Save refuses the conflict until explicit reload or overwrite.

Drop external files onto folder/file rows and empty tree background. Verify the
destination highlight, 450 ms closed-folder expansion and cancellation cleanup.
A file row uses its parent; background uses the root. Moving an open dirty external
file must preserve its text, tab and native editor identity, then save at the new
location. Editing pauses during transfer. Repeat with a name collision, a missing
source, a read-only destination, a symlink leaf and files on different filesystems:
failures must retain the source and buffers, never overwrite destinations, and
show the core error. Earlier successful moves in a batch remain visible.

The GNOME workspace smoke includes native FileList drop signals through the same
asynchronous callbacks as the app. The macOS pointer smoke sends AppKit file URL
drags to the SwiftUI drop delegate, and WorkspaceStoreSmoke checks external
buffers and moves through UniFFI. KDE's native URI-list smoke is:

```sh
target/debug/native-workspace-kde --smoke-external=/path/to/disposable-workspace \
  --smoke-external-source=/path/to/external-file
```

The workspace must contain `src/main.rs`; the external source must be a disposable
UTF-8 regular file. The smoke sends native drag events through QQuickWindow,
checks folder expansion and moves the edited source into `src`. Run on supported
native desktops; macOS toolkit checks do not verify Linux file-manager integration.

## Document tabs verification

Open three files in a different order from their sidebar order. Tabs must keep the order in which they were opened; reopening a tab selects it without duplicating it. Edit two files and switch between them: both buffers and dirty markers must remain intact. Close an inactive clean tab and confirm that the active document stays selected. Close the active tab and confirm that its right neighbor (or the last remaining tab) becomes active.

Close a modified tab and exercise Cancel, Save, and Discard Changes. Cancel preserves the buffer; Save closes only after a successful save; Discard closes without writing and reopening loads the disk content. Change the file externally before choosing Save: the conflict must leave the tab and local edits intact. Save All must save inactive dirty tabs too, stop at the first failure, and accurately reflect files already saved. Rename or move an open file or its parent folder and confirm that tabs retain their order and buffers. Delete a clean active file and confirm that its tab disappears and a remaining tab becomes active.

After editing one document, switch to another with identical text and try Undo: it must never replay the previous document's edits. The shared native editor clears its undo history on a document switch. Edit again, then rename that document: its selection and undo history must remain intact. In GNOME, click an already selected tab repeatedly and verify that it stays highlighted. On macOS, navigate to a search match, then switch or close its tab: the new document must not inherit the old search selection. The tab strip must leave most of the workspace height available to the editor. Every tab must use the same Material icon as its file-tree row and update it on extension changes. A modified document has a fixed dot to the left of the icon, independent of the title: editing must not change tab width, and truncating a long title must not hide its dirty marker. Save All belongs to the window header/toolbar. Open enough GNOME tabs to overflow and scroll away from the selected one: idle refreshes and edits must retain the scroll position. In KDE, verify sidebar tabs in the header, equal search/document-row heights, and buttons fitting their bars after resizing. On macOS, pass the pointer repeatedly over active and inactive glass tabs; text and icons must remain visible, and sidebar/document controls must share a vertical center.

## Architecture checks

`cargo xtask architecture` validates the Cargo dependency graph. `verify` runs it before formatting, type checking, warnings-denied Clippy and tests. Check, lint and test include all targets, so examples and test binaries cannot retain obsolete APIs. CI runs the same tasks for core, xtask, Developer Tools and each native adapter; native workspace, sidebar and delayed-drag smoke tests also run on Linux. The workflow is defined in `.github/workflows/verify.yml`; branch protection must be configured in the repository host to require its jobs before merging.

Use the repository's `.agents/skills/editor-change/SKILL.md` when extending editor behavior. `AGENTS.md` and ADR 0004 describe ownership. GTK code is split into window construction, presentation controller, sidebar materialization and a debug-only smoke driver. Qt keeps Home and entry dialogs in separate components and its smoke entrypoint under `tests/`. Neither production adapter exposes synchronous search/filter shortcuts.
