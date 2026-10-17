# ADR 0006: operating-system open requests

Status: accepted

## Context

Application-icon drops and Open With can deliver files or folders before native
windows exist, or during ongoing filesystem work. Native adapters must not
classify filesystem entries, choose workspace identities, scan directories or
implement another backend queue. Existing frontends have one workspace per
instance; opening another folder must protect every dirty tab.

## Decision

`OpenPaths` accepts an ordered list of local paths. The injected filesystem port
resolves each to a canonical file or directory, rejecting relative paths,
symlink leaves and special files. Directories invoke the existing workspace
command, except that reopening the current canonical root is a no-op. Files
invoke the existing permanent-file command. When no workspace exists, the first
file first opens its parent directory as the workspace. Each successful step
commits independently through the existing application pipeline and reducer.

The command uses exclusive worker scheduling. Further OS requests defer even
during exclusive work. Queuing a folder-capable request pauses editing at once,
and deferred replay remains exclusive, so newer projected edits cannot be lost
by a workspace switch. Already queued edits replay before the request and the
ordinary unsaved-workspace guard protects them.

AppKit handles application open URLs and buffers window presentation until
SwiftUI can open the workspace window. GTK uses GApplication's open signal and
unique-instance forwarding. Qt collects positional launch arguments and native
file-open events until its QML window is ready. These are native delivery
buffers only: Session owns execution, ordering, state and errors.

## Consequences and limits

One process still owns one workspace. Multiple folders select workspaces in
delivery order; a later folder can replace earlier clean tabs. Dirty documents
prevent changing roots. Files outside an already-open workspace retain the
external identities introduced by ADR 0005. OS entry delivery does not move
anything on disk.

macOS declares alternate text/data and folder document handlers. Linux desktop
entries advertise local-file arguments without taking over MIME defaults.
GNOME forwards requests to its primary instance; the KDE Linux launcher starts
a new process per invocation. Shell support for dropping onto launchers varies.
Qt file-open events are handled where the platform emits them; no Linux event
or D-Bus single-instance service is assumed. See the primary platform contracts:
[AppKit application open URLs](https://developer.apple.com/documentation/appkit/nsapplicationdelegate/application(_:open:)),
[GApplication open](https://docs.gtk.org/gio/signal.Application.open.html), and
[QFileOpenEvent](https://doc.qt.io/qt-6/qfileopenevent.html).
