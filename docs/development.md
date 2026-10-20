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

`build`, `check`, `lint`, `test` and `verify` accept `core`, `macos`, `gnome`, `kde`, `devtools` or `xtask`. `start` and `run` accept only editor frontends. `check macos` and `lint macos` check the Rust bridge; `build macos` compiles Swift, and `test macos` also executes the Swift smoke tests. `test gnome` and `test kde` run the selected Rust package's tests; native GUI smoke modes remain explicit as documented below. Tasks stop at the first failed step.

Inspect composition without running it:

```sh
cargo xtask run macos --dry-run
cargo xtask verify core --dry-run
```

The dry run lists task steps; the macOS packaging and smoke steps are shell scripts. `apps/macos/build.sh` enters the xtask build pipeline, which calls `apps/macos/package-build.sh` to compile and package, `apps/macos/start.sh` launches, and `apps/macos/test.sh` compiles and executes smoke tests against an already built bridge. `apps/macos/run.sh` composes build and start. The normal frontend builds still include Developer Tools, but never execute tests.

Existing action-target commands (`build-macos`, `run-macos`, `build-gnome`, `run-gnome`, `build-kde`, `run-kde`, `check-core`) remain aliases of the corresponding two-word commands. New tasks also support that spelling, for example `test-macos` and `verify-core`.

## Build disk usage

```sh
cargo xtask cache              # disk usage, including Cargo target-dir overrides
cargo xtask clean --dry-run    # preview removal of workspace Rust artifacts
cargo xtask clean              # remove workspace artifacts, keep compiled dependencies
cargo clean                   # reset the entire Rust build cache, including dependencies
```

Development and test builds disable incremental compilation and keep only line
tables for workspace backtraces; dependencies omit debug information. This reduces
disk usage at the cost of slower recompilation after edits and no local-variable
inspection in a debugger. For a debugging session, set `CARGO_PROFILE_DEV_DEBUG=2`
(or `CARGO_PROFILE_TEST_DEBUG=2` for tests). Dependency overrides still apply.

Every `cargo xtask build` checks Cargo's target directory before and after the
build. If either measurement exceeds **8 GiB**, it automatically resets the Rust
cache at the end, after native packaging and before `run` starts the app. The same
rule covers build aliases, builds within macOS test/verify, and the macOS shell
build/run entry points. Failed builds also perform the final check and cleanup.
Set `NATIVE_WORKSPACE_BUILD_CACHE_LIMIT_MB=4096` for a 4 GiB threshold; the value
must be a positive integer in MiB. `--dry-run` lists the rule without cleaning.

Automatic cleanup uses `cargo clean`, preserving profile-root binaries and
libraries (including Swift bridge libraries and executable permissions) and the
packaged macOS app. Dependencies, fingerprints, intermediate native objects and
incremental caches are discarded, so the next build recompiles them. The threshold
triggers cleanup; it is not a hard disk quota during compilation, and retained
outputs may themselves exceed it. Standalone `cargo build`, check/lint/test tasks
without a build step, and other worktrees are outside this rule.

Build pipelines sharing a target directory serialize through a maintenance lock
outside that directory. Do not run raw Cargo builds or manual cleanup concurrently
with these pipelines. Each worktree has its own `target` by default; remove obsolete
worktrees' build caches too.

Run cleanup with builds and native packaging scripts stopped. `cargo xtask clean`
preserves third-party dependencies but removes workspace binaries (including the
xtask runner, rebuilt on its next invocation). `cargo clean` also preserves the
Cargo download registry. Neither command removes the packaged macOS app in
`apps/macos/.build`; remove that generated directory separately if needed, then
use `cargo xtask build macos` to recreate it. `start` alone never triggers cleanup.

## Explicit GUI smoke modes

GNOME's `NATIVE_WORKSPACE_SMOKE=1` and `NATIVE_WORKSPACE_SMOKE_WORKSPACE=/path/to/disposable-fixture` enable native startup and edit/save smoke paths when launching the built executable. KDE's `--smoke-sidebar` and `--smoke-drag` modes require disposable fixtures; see [KDE development](kde.md) for contents and Qt style variants. Set these only for deliberate GUI verification; no normal run task injects them. Headless Linux GUI checks need a display server such as Xvfb.

## Development behavior

The initial vertical slice covers opening a directory, listing files, opening, editing, and saving UTF-8 documents. The frontends now collect queued `notify` events every 250 ms and reconcile periodically for external changes. Clean open documents reload automatically. If local edits and disk content diverge, normal Save refuses to overwrite the disk; the user can reload or explicitly overwrite it. All frontends use the shared Session runtime.

Normal frontend launches open a SQLite metadata database in the platform application-data directory. Set `NATIVE_WORKSPACE_STATE_DB=/absolute/path/metadata.sqlite3` to use an isolated database. `cargo test` uses temporary files; GUI smoke modes use in-memory metadata, except the session restart suite, which requires an isolated temporary database. Migration files live in `crates/storage/src/migrations`, with schema version tracked by SQLite `PRAGMA user_version`.

The Search sidebar matches UTF-8 file contents; the Files field filters entry names. Both ignore case and surrounding whitespace by default. Search offers explicit case, whole-word and regex controls. Every content occurrence has a separate result with line, column, and a short preview; selecting it opens the file and highlights that text. Unsaved document buffers take precedence over disk content. Files above 1 MiB and unreadable files are skipped for content search, while their paths remain searchable. An empty content query clears matches; an empty name filter restores the complete file tree. Search runs in a worker and does not use SQLite.

## Git Source Control verification

The Files tree loads Git status automatically and refreshes every five seconds,
including when Source Control is closed. Added/untracked entries show green `A`,
modified entries amber `M`, renamed entries amber `R`, deleted entries red `D`,
and conflicts red `U`. Ancestor folders inherit the highest-priority change
(conflict, modified, deleted, renamed, added), including while collapsed. Ignored
files/folders show muted `!`; ignoring a folder also mutes its descendants without
marking unrelated ancestors. Decorations include discovered nested repositories
and use workspace-relative paths when opening a repository subfolder.

Git must be available on PATH. Configure author identity from Git Actions or the
installed Git configuration. Remote authentication uses existing Git credential
and SSH helpers. Use disposable repositories for mutation smoke tests.

The sidebar starts with a resizable 50/50 vertical split. Changes, staging and
commit controls occupy the upper pane; the branch/commit graph occupies the lower
pane. Git Actions contains repository initialization/cloning, author configuration,
branches, remote synchronization, merge/rebase/cherry-pick/revert, undo commit,
stashes, remotes, tags, worktrees, comparisons, file history and blame. Repository
selection includes nested repositories. Reviews offer full before/after views or
unified lines for partial staging. Current and incoming conflict versions can be
reviewed with an editable result and Save and Stage Result.

Verify these workflows in each native adapter:

1. Modify a tracked file, stage it, then modify it again. Staged Changes and Changes
   show distinct diffs. Stage or unstage selected lines without changing disk text.
   An external edit rejects an old partial-staging selection.
2. Enter a commit message; commit, amend and sign off. The graph, branch references
   and ahead/behind counts update. Undo Last Commit keeps the reverted commit's
   changes staged. Open a graph node to inspect it, and use its contextual actions
   or Git Actions to compare, cherry-pick, revert or create a branch.
3. Create divergent branches and merge them. The graph must show both parent
   edges and shared ancestry. Drag the divider, scroll each pane independently and
   load more history. Create a nested repository and switch repository contexts.
4. Exercise fetch, publish, push, pull and sync against a disposable bare remote.
   Reject non-fast-forward pushes normally. Force with lease is an explicit option.
   Identity, authentication, signing and hook failures must show Git's error text.
5. Create a merge conflict. Review current/incoming/result, choose a resolution,
   edit it and save/stage. Markers and externally changed baselines are rejected.
   Continue completes the operation; Abort restores its previous Git state. A
   failed merge still publishes conflicts and reconciles clean open documents.
6. Stash/apply/pop/drop, create/delete a tag, add/remove a remote, and create/remove
   a worktree. Ignore a filename with spaces or special pattern characters.
   Dirty buffers block worktree changes. Commit/fetch/push retain concurrent typing.
7. Open ordinary folders, unborn branches, linked worktrees and subfolder
   workspaces. Git Actions can initialize or clone a repository. Binary content is
   summarized; output/content above 2 MiB is limited. History loads 200 commits at a
   time up to 5,000; discovery includes up to 32 roots. Subfolder commits cannot
   include staged paths outside that workspace.

`cargo xtask verify core` uses temporary repositories and a local bare remote to
exercise these operations, graph topology, partial staging, conflict resolution,
stale baselines, repository isolation and failure/concurrency cases. `cargo xtask
verify macos` includes `GitSmoke`, which opens Git through the actual workspace
sidebar, checks automatic repository loading, visible toolbar controls with a long
branch name, tab switching and the initial equal split, then creates real branch
divergence and a merge, checking graph width, the current-branch badge, the merge
commit's changed files and a numbered commit-file diff. With long tags, branch
and subject at a 260-point sidebar, every element inside each commit row must stay
within the panel. Set `NATIVE_WORKSPACE_GIT_SCREENSHOT` to an absolute PNG
path to capture the workspace with its Git sidebar.

GNOME's workspace smoke opens a real Git review dialog and checks the initial split
when its fixture is inside a Git repository. Keep `.git` in the fixture's parent,
with only `README.md` and `src/main.rs` in the workspace, to preserve tree row counts. The
Git panel's minimum width must stay within 240 px whatever the branch, tag or
subject lengths.

KDE provides `--smoke-git=/path/to/disposable-repository`: commit `note.txt` with
`original`, stage `staged`, then write `working tree` to disk (each with a newline).
The driver runs at a 260 px width and checks both diffs, the split, staging,
commit, graph refresh, references and that long tags and subjects elide inside
each commit row. Run
`QT_QPA_PLATFORM=offscreen QT_QUICK_BACKEND=software` on a headless host.

## Session restoration verification

Normal launches reopen the last saved workspace layout. An upgraded schema-1
metadata database preserves recents but has no session layout until a workspace
is opened. Use an isolated `NATIVE_WORKSPACE_STATE_DB` for manual checks:

1. Open a workspace, two permanent tabs, a preview and a file outside the workspace.
   Reorder the tabs and activate a different document. Quit and restart: the
   workspace, tab order, active document, preview and external tab must return.
2. Close every tab, quit and restart: the workspace returns with its empty editor.
3. Remove one saved file while the app is closed: other tabs reopen with an error.
   Quit without changing the layout, return the file and restart: its tab returns.
   Remove the workspace instead: Home stays visible and reports the error.
4. Launch through Open With or a file/folder argument while another workspace is
   saved: the explicit request takes precedence. Deliver another OS open while
   restoration is pending and verify the final workspace follows that request.
5. Edit active and inactive tabs, then quit through menus, shortcuts and Home's
   terminating window close. Cancel retains the text. Save All writes every dirty
   tab; a changed disk baseline keeps the app open and preserves remaining edits.
   Discard quits while preserving tab layout and leaving disk contents unchanged.
   Quitting during a pending filesystem operation must keep the app open.

`cargo xtask verify macos` includes `SessionRestoreSmoke`: it drives the launch
delegate, asynchronous store and native alert buttons against temporary SQLite
metadata. GNOME and KDE restart drivers run separate processes using the actual
startup path. After building the selected frontend:

```sh
tools/smoke-session-restore.sh gnome
tools/smoke-session-restore.sh kde
```

The script creates and removes disposable files and a temporary database. On
headless Linux, run it under `dbus-run-session -- xvfb-run -a`; KDE can use
`QT_QUICK_BACKEND=software`. CI runs both suites. GNOME covers native Cancel and a
save conflict; KDE covers Cancel, Save All and Discard through its real dialog.
Existing KDE regression drivers explicitly choose discard through quit preparation
when ending a test, so unsaved-buffer scenarios do not stop at a production quit
confirmation. Core tests cover migration, atomic checkpoints, missing or invalid paths, partial
restore, relocation rollback, metadata write retries, quit conflicts and worker
ordering. Restoring does not recover unsaved text, Undo, cursor/scroll positions or
expanded folders. Concurrent instances share one last-writer-wins layout.

## Search and replacement verification

In the **Search** tab, search `cat` in `cat CAT Cat concatenate cat_ caté`.
`Aa` must restrict case; whole-word must reject `concatenate`, `cat_` and `caté`.
Enable `.*` to use Rust regex syntax, including captures, Unicode classes, anchors
and explicit newline patterns. Unsupported look-around/backreferences and malformed
expressions display an inline error and clear prior results. This engine does not
provide full VS Code/PCRE regex compatibility or capture case-conversion escapes.

Expand **Search details** and try `*.rs, *.{md,txt}`, an anchored `./src/**`, and an
excluded `target` folder. Commas separate patterns except inside brace alternatives;
unanchored patterns can match at any depth. The ignore toggle controls `.gitignore`
and `.git/info/exclude`; loaded buffers are still searchable. Open Editors restricts
the scope to loaded tabs and includes external tabs. Check that changing these
settings during a running search discards older batches and that buffer edits keep
the same scope. Cancel keeps incomplete results; Refresh searches again.

Expand **Replace**, enter replacement text (empty text deletes matches) and enable
**Preserve Case** to map `cat CAT Cat` to `dog DOG Dog`. In regex mode, `$1`,
`${name}` and `$$` expand capture groups and a literal dollar sign. Choose all
results, the selected file, or a single occurrence. macOS/KDE also expose replacement
in result context menus. All actions prepare a preview; Cancel must change nothing.
The preview shows matched/replacement excerpts, limited to 240 characters per side.
Replace applies to unsaved buffers for open tabs and saves closed files to disk.
Existing overlapping literal occurrences are replaced leftmost, without overlap.

After previewing, change a buffer or its disk file and try Replace. The entire
preflight must refuse before changing any other file. A write failure during a
batch retains earlier successful files and restarts search. A cancelled or truncated
search cannot prepare replacement until refreshed/narrowed. Ordinary Save after an
open-buffer replacement must still protect external conflicts.

Core tests live in `application/src/tests/replacement.rs`; macOS's
`WorkspaceStoreSmoke` and the GNOME workspace smoke drive asynchronous search,
option changes, replacement preparation/application and stale-buffer rejection.
KDE's `--smoke-workspace` covers the same flow through CXX-Qt/QML. Open the Search
tab with Command-Shift-F on macOS or Ctrl-Shift-F on Linux; Command/Ctrl-Shift-H
opens the replacement field.

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

## Application icon and Open With verification

On macOS build the app bundle, then drop a local UTF-8 file or folder onto its
Dock or Finder icon. Repeat after quitting the app. Files open permanent tabs;
folders select a workspace. Also exercise Finder's **Open With** or
`open -a /absolute/path/NativeWorkspace.app /absolute/path/to/file`.
The bundle declares text/data and folder document types with alternate handler
rank; it does not request default ownership. Files must still pass the shared
UTF-8 and regular-file validation. `SystemOpenSmoke` drives the same AppKit
application delegate before and after window presentation is available.

On Linux, put the selected frontend executable on `PATH`, then install that
frontend's `org.nativeworkspace.App.desktop` from `apps/gnome` or `apps/kde`
into `~/.local/share/applications/` and run
`update-desktop-database ~/.local/share/applications/`. The templates share the
application ID: install one frontend's template at a time. The `%F` entry accepts
local paths with spaces and Unicode from file managers and launchers. Installing
the entry does not change MIME defaults; choose **Open With** explicitly.
Launcher drag/drop availability depends on the desktop/shell. GNOME forwards
later requests through GApplication unless `NATIVE_WORKSPACE_NEW_INSTANCE=1`
is set. KDE's Linux desktop entry starts a new process for each invocation;
it does not provide D-Bus forwarding. Qt file-open events are also accepted on
platforms that deliver them (notably macOS); this does not imply such events are
available on Linux. GNOME/Qt checks on macOS do not verify Linux shell integration.

Each process has one workspace. Items are handled in supplied order, so a later
folder replaces an earlier clean workspace and its tabs. Without a workspace,
a file first selects its parent folder. Reopening the current workspace keeps
its tabs and edits. With a dirty inactive or active tab, a different folder must
fail and preserve the buffer. A missing, symlinked, non-local or non-UTF-8 file
must report an error; completed earlier items remain open. Deliver another
request during a delayed open and check that it queues and the UI stays responsive.

Dedicated native smoke modes use a disposable workspace containing `README.md`
and `src/main.rs`:

```sh
NATIVE_WORKSPACE_SMOKE_SYSTEM_OPEN=/absolute/fixture target/debug/native-workspace-gnome
QT_QPA_PLATFORM=offscreen target/debug/native-workspace-kde \
  --smoke-system-open=/absolute/fixture /absolute/fixture/README.md
```

The GNOME smoke emits the native GApplication `open` signal; the KDE smoke reads
real launch arguments and sends `QFileOpenEvent` through the application filter.
They check startup, repeated requests and dirty-buffer rejection. macOS's normal
`cargo xtask verify macos` includes its dedicated delegate smoke. End-to-end
Dock/Finder and Linux desktop-shell deliveries remain manual checks.

## Document tabs verification

Open three files in a different order from their sidebar order. Tabs must keep the order in which they were opened; reopening a tab selects it without duplicating it. Edit two files and switch between them: both buffers and dirty markers must remain intact. Close an inactive clean tab and confirm that the active document stays selected. Close the active tab and confirm that its right neighbor (or the last remaining tab) becomes active.

Close a modified tab and exercise Cancel, Save, and Discard Changes. Cancel preserves the buffer; Save closes only after a successful save; Discard closes without writing and reopening loads the disk content. Change the file externally before choosing Save: the conflict must leave the tab and local edits intact. Save All must save inactive dirty tabs too, stop at the first failure, and accurately reflect files already saved. Rename or move an open file or its parent folder and confirm that tabs retain their order and buffers. Delete a clean active file and confirm that its tab disappears and a remaining tab becomes active.

After editing one document, switch to another with identical text and try Undo: it must never replay the previous document's edits. The shared native editor clears its undo history on a document switch. Edit again, then rename that document: its selection and undo history must remain intact. In GNOME, click an already selected tab repeatedly and verify that it stays highlighted. On macOS, navigate to a search match, then switch or close its tab: the new document must not inherit the old search selection. The tab strip must leave most of the workspace height available to the editor. Every tab must use the same Material icon as its file-tree row and update it on extension changes. A modified document has a fixed dot to the left of the icon, independent of the title: editing must not change tab width, and truncating a long title must not hide its dirty marker. Save All belongs to the window header/toolbar. Open enough GNOME tabs to overflow and scroll away from the selected one: idle refreshes and edits must retain the scroll position. In KDE, verify sidebar tabs in the header, equal search/document-row heights, and buttons fitting their bars after resizing. On macOS, pass the pointer repeatedly over active and inactive glass tabs; text and icons must remain visible, and sidebar/document controls must share a vertical center.

## Architecture checks

`cargo xtask architecture` validates the Cargo dependency graph. `verify` runs it before formatting, type checking, warnings-denied Clippy and tests. Check, lint and test include all targets, so examples and test binaries cannot retain obsolete APIs. CI runs the same tasks for core, xtask, Developer Tools and each native adapter; native workspace, sidebar and delayed-drag smoke tests also run on Linux. The workflow is defined in `.github/workflows/verify.yml`; branch protection must be configured in the repository host to require its jobs before merging.

Use the repository's `.agents/skills/editor-change/SKILL.md` when extending editor behavior. `AGENTS.md` and ADR 0004 describe ownership. GTK code is split into window construction, presentation controller, sidebar materialization and a debug-only smoke driver. Qt keeps Home and entry dialogs in separate components and its smoke entrypoint under `tests/`. Neither production adapter exposes synchronous search/filter shortcuts.

## Integrated terminal verification

Terminal emulation uses the pure-Rust `alacritty_terminal` crate; it needs no
toolchain beyond Cargo. See the [attribution](../assets/ALACRITTY_ATTRIBUTION.md).

Every frontend draws terminals with the bundled JetBrains Mono Nerd Font Mono faces
and the cell grid from `terminal_font()` in the core, so Powerline/Nerd Font prompt
themes look the same on every desktop; the system fonts are only fallbacks for
glyphs such as emoji. macOS loads the faces from memory, KDE registers them with
`QFontDatabase`, and GNOME caches them under the user cache directory for Pango
(requires Pango 1.56). See the [font attribution](../assets/fonts/ATTRIBUTION.md).

Toggle the panel with the **Terminal** button at the end of the editor status bar
(also **View → Terminal** on macOS and **Ctrl+`** on macOS and KDE); the panel
opens below the status bar. Create two tabs, run `pwd` in
both, change directory in one, and verify independent shells. Hide/show the panel
and switch documents without restarting them. Resize while running an interactive
program such as `vi`; check arrows, Tab, Ctrl-C, Unicode, Powerline and Nerd Font prompt glyphs, colors and the alternate
screen. Select text with the mouse; use Cmd-C/V on macOS or Ctrl-Shift-C/V on Linux.
Scroll to inspect retained output. The first version uses a native CPU renderer;
Kitty graphics, terminal search UI, shell integration and process restoration are
not exposed. Each window permits at most 16 tabs. Scrollback is limited to
10,000 lines per terminal; individual input messages are limited to 64 KiB.

Generate sustained output in one tab while typing in another and editing a file.
Filesystem moves must not pause terminal input/output. Close a tab at an idle
prompt: it closes without confirmation or an error. Run `sleep 30` and close it:
Cancel retains it; Terminate closes it and its foreground processes. Try changing
workspace through both the chooser and OS requests with live terminals: the core
must reject the switch and preserve documents. Exit with terminals active:
Cancel keeps the app open; confirmation waits asynchronously for termination.
Shell startup failures and nonzero exit statuses must remain visible in the panel.

`cargo xtask verify core` includes real PTY tests, emulator mode/Unicode/style/reply tests,
tab lifecycle and filesystem concurrency tests. Run the adapter checks and native
terminal smoke modes on supported desktops; macOS GTK/Qt checks do not establish
Linux desktop behavior.

Native terminal smoke modes use disposable workspaces:

```sh
NATIVE_WORKSPACE_NEW_INSTANCE=1 NATIVE_WORKSPACE_SMOKE_TERMINAL=/path/to/fixture target/debug/native-workspace-gnome
target/debug/native-workspace-kde --smoke-terminal=/path/to/fixture
```

The macOS `TerminalSmoke` is included in `cargo xtask test macos`. These drivers
exercise native input callbacks, rendered snapshots, tab isolation and confirmed
termination through the same asynchronous Session API as normal interaction.
