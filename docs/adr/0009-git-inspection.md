# ADR 0009: Read-only Git inspection

Status: accepted for initial inspection; extended by [ADR 0010](0010-git-source-control.md).

Each window's existing Session exposes Git inspection through ordinary
`AppCommand` intentions, handler events, the reducer and an immutable shared
`GitSnapshot`. `RefreshGit` and `ShowGitDiff` use the Session background schedule.
Editing remains available; edits are projected and queued saves replay through
the existing command policy. Workspace changes clear the Git view.

The injected `WorkspaceFilesystem` port owns repository discovery, status and
diff I/O. Its real adapter invokes the installed Git executable directly, without
a shell, optional index locks, external diff commands, text conversions or a
filesystem-monitor hook. No native adapter executes Git or interprets porcelain
output. Unsupported injected filesystems report no repository by default.

Inspection is limited to the opened workspace, including when it is a subfolder
of a repository. Paths in Git snapshots are repository-relative; they are not
document identities and are never rebased by optimistic editor relocations.
Status uses NUL-delimited porcelain records, including rename sources. A diff
request checks current status and workspace scope again before reading. Linked
worktrees and unborn branches use Git's normal discovery and index semantics.
Staged diffs compare HEAD with the index; working-tree diffs compare the index
with disk. Untracked regular UTF-8 files show a bounded content preview. Binary
changes retain Git's textual summary; untracked binary content is not rendered.

The view updates when entering Git or pressing Refresh. It deliberately describes
on-disk changes: unsaved buffers are not staged or substituted into diffs. Native
adapters own the review dialog, selection and scrolling. Git inspection never
writes files, stages changes, creates commits or contacts remotes. Status output
above 2 MiB reports an error; diff/content previews are capped at 2 MiB and marked
incomplete. Git state is transient and is not stored in SQLite.
