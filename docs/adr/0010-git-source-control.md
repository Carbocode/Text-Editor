# ADR 0010: Git Source Control and native history graph

Status: accepted. Extends ADR 0009 beyond inspection.

Each window still owns one Session. Git mutations are typed `GitOperation`
intentions, queries are `GitQuery` intentions, and all published Git state travels
through events and the existing reducer. A small shared action catalog describes
forms for the three native adapters; it never accepts shell commands. Git CLI I/O,
credential helpers, hooks and index locks belong to the filesystem adapter.

The application owns repository selection, history limits, graph lanes and edges,
partial-staging patch generation, conflict-marker interpretation and checks for
unsaved buffers. Native adapters own drafts, selections, dialogs, scrolling and a
vertical sidebar divider, initially at half height. The upper pane contains
changes and commit controls; the lower pane draws shared graph geometry and opens
commit details. No web view or extra frontend worker is introduced.

Workspace listings discover up to 32 repository roots (including nested repositories
and worktrees represented by `.git` files). Selection changes only the Git context,
not the document workspace. Subfolder workspaces retain their inspection scope;
committing staged paths outside that scope requires opening the repository root.
A workspace switch clears repository selection and all Git reviews.

Mutations run on the Session worker. Worktree/staging operations require saved
buffers and pause editing. Metadata-only operations, commit, fetch and push use the
background policy, retaining projected edits and deferred saves. Concurrent Git
mutations are rejected. A failed merge, rebase, pull or multi-step operation can
leave real changes: reconciliation and Git state are committed before its error is
published. Earlier successes are retained. Git decides whether a remote operation
is a fast-forward and protects pushes with its ordinary rules; optional force
push uses `--force-with-lease`.

The sidebar polls while visible, at five-second intervals, on the same worker.
Polling preserves errors and an open historical review, and updates a current diff
without replacing native drafts. Changed or missing diff baselines reject partial
staging. Selected lines are converted to patches in Rust, then applied to the index;
frontends never construct patches. Full UTF-8 before/after views accompany ordinary
text diffs. Binary and large content retains a bounded unified preview/summary.

The merge editor presents current and incoming content plus an editable result.
Shared marker parsing offers current, incoming or both without duplicating context.
Saving rechecks the disk baseline, rejects unresolved markers, writes through the
filesystem port and stages the result. If staging fails after writing, the saved
file remains and the error is published. Continue and Abort use Git's current
merge/rebase/cherry-pick/revert state. Unsaved buffers are never overwritten.

Git runs without a shell, with literal pathspecs and explicit reference validation.
Inspection disables external diff and text-conversion drivers. Explicit mutations
honor the user's Git identity, signing, hooks, SSH and credential-helper settings.
Remote credentials are not stored by the editor. Operation output is drained with
bounded retention; remote commands time out after two minutes. Inspection/diffs
are bounded to 2 MiB per output/content, and history loads 200 commits at a time up
to 5,000. Git metadata and UI drafts remain transient rather than entering SQLite.

## Graph, references and commit review

History lists branches, remote-tracking branches, tags and HEAD only; stashes,
notes and tool namespaces such as checkpoint refs never enter the graph. The
filesystem adapter parses decorations into typed references (detached HEAD,
branch, remote, tag) and marks the checked-out branch. The application assigns
each lane a color for its lifetime, keeps first-parent lines straight by merging a
reserved lane into the commit's lane, draws no edge above a branch tip, and
publishes the widest row so every frontend reserves one gutter. Rows also carry
merge and HEAD flags; adapters choose palette, dot style and badge styling only.

A commit query also returns author email, full message and first-parent changed
files; a commit-file query returns one file's patch with the same summary, so an
inline commit detail survives opening a file. Patch-like inspections and diffs are
classified in Rust into meta, hunk, context, added and removed lines with old/new
line numbers, tracking hunk sizes so content beginning with `--- ` is not a
header. Line indices still address the reviewed text for partial staging. The
action catalog carries a menu group shared by every frontend.
