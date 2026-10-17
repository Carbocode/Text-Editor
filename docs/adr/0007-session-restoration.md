# ADR 0007: restore the last confirmed editor session

Status: accepted

## Context

Recent workspace metadata does not describe the open tabs. Normal launches should
reopen the last workspace with its ordered tabs, active document and preview.
Filesystem contents remain authoritative; this phase does not recover unsaved
buffers after a crash or persist native focus, selection, undo, scrolling or tree
expansion.

## Decision

The storage adapter migrates SQLite from schema 1 to 2, preserving recent
workspaces and adding a singleton workspace session plus its ordered document
paths. Workspace paths are canonical absolute paths, internal document paths are
workspace relative, and external identities retain their canonical absolute paths.
A layout replacement is one transaction. No document content is stored in SQLite.
An upgraded database has no saved layout until a workspace is opened.

`Application::execute` compares the confirmed layout around a command and persists
changes after its committed steps, including successes before a composite failure.
Session projections never write metadata. Failed metadata writes preserve committed
editor state, publish the storage error and retry on the next user command, including
quit preparation. A committed relocation retains its confirmed paths even when
the metadata write fails, so deferred edits and saves replay at the new path. Polling and ordinary typing without a layout change do not write
metadata. Small metadata transactions use the existing storage adapter on the
command's execution thread; there is no second frontend queue or persistence worker.

`RestoreSession` is an exclusive Session worker command. It is a no-op when a
workspace is already open and is skipped rather than queued behind running work.
Persistence activation only loads recent metadata; startup explicitly requests
restoration. The command reuses workspace and document validation, normal loading,
events and the reducer. Individual unavailable, invalid or unreadable tabs are
skipped while later tabs still open; the first error is reported. A missing active
tab falls back to the first surviving tab. A missing workspace leaves Home visible.
Restoration never overwrites the saved layout, allowing unavailable files to return
on a later launch; a later user change to the layout establishes the new checkpoint.

Native startup handles initial OS opens first: AppKit defers restoration until
`applicationDidFinishLaunching`, GTK restores only on normal initial activation,
and Qt drains initial requests before considering restoration. Later OS requests
use Session's existing exclusive queue and cannot be overwritten by a deferred
restore. Native adapters present the restored workspace from its published state.

`PrepareToQuit { save }` is inline and rejects every running Session worker, even
when discarding. Saving uses the existing ordered Save All steps, stopping on a
failure while retaining earlier saves and all tabs. Discarding permits quitting
without closing tabs or changing their saved layout. Native adapters own the shared
Save All / Cancel / Discard Changes confirmation and invoke the command before
terminating. Normal quit entry points (menus, shortcuts, native quit delivery and
windows that terminate the process) use that confirmation; returning Home keeps
the live session.

## Consequences and limits

There remains one workspace per frontend instance and one Session per editor
window. The singleton layout is shared across instances using the same metadata
database: the last completed layout write wins. Atomic transactions avoid mixed
layouts. The feature does not restore multiple process instances.

Unsaved buffers require an explicit save/discard choice on normal quit and are not
recovered after a crash or forced termination. Native presentation state and undo
history start fresh. No filesystem write occurs during restoration. Core tests use
temporary databases; native restart smoke drivers require an isolated database and
disposable files. See the session restoration checks in `docs/development.md`.
