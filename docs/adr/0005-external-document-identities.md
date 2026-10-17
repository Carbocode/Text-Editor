# ADR 0005: external document identities and OS file drops

Status: accepted

## Context

OS file drops must open files outside the workspace as tabs and move dropped
files into a workspace folder. A basename is insufficient to distinguish files
from different directories. Native adapters must not change the workspace to
open a file, own filesystem operations, or maintain a second document store.

## Decision

`DocumentId::parse` continues to accept workspace-relative entry paths only.
`DocumentId::from_document_path` also decodes absolute external document identities
from snapshots. External identities are canonical UTF-8 absolute paths resolved
by the filesystem port after rejecting non-files and symlink leaves. A file
inside the current workspace uses its existing relative identity.

The application document table owns both kinds of buffer. External documents
participate in tabs, dirty/conflict checks, Save All and explicit reload/overwrite,
while workspace search, name filtering and sidebar rows exclude them. Refresh
also reconciles external buffers; closing an external tab removes its entry.

`OpenFiles` and `MoveFiles` execute ordered composite steps on the Session worker.
Each successful step commits independently, so a later failure preserves completed
opens/moves. Entry registration and relocation are reducer events emitted only
after successful I/O. Moving an open external document reuses the relocation
reducer, preserving its buffer, saved baseline, tab position and editor generation.

External moves and deletion use the exclusive scheduling policy. Their snapshots
publish `activity.editing_paused`; native editors disable editing until completion.
Regular workspace moves retain their existing optimistic projection and deferred
editing policy. Exclusive transfers do not project unconfirmed destination rows.

Native adapters decode local file URLs, own drag previews, target highlighting
and temporary folder expansion, and send paths plus a workspace-relative folder
to Rust. `Session::can_move_files` supplies cheap hover validation; the handler
and filesystem adapter revalidate before moving. OS drops on editor/tabs open
permanent tabs; tree rows use the folder itself or a file's parent, and tree
background uses the workspace root.

## Consequences and limits

The filesystem port resolves user-selected files and moves regular files with
an atomic no-replace rename. A move across filesystems fails with the source
retained; no copy/delete fallback or silent overwrite is performed. Directories
and non-local URLs are not supported by this phase. Opening through application
icons and treating dropped folders as workspaces are separate roadmap items.

Normal saves and document opening by existing tab identity retain their current
execution policy. External documents are reconciled during workspace polling and
explicit refresh; they do not install separate directory watchers. No ownership
boundary changes: Session owns scheduling, handlers validate, adapters own I/O,
and only the reducer mutates document state.
