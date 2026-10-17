# ADR 0002: Transient application events

Status: accepted for Phase 2. Frontend API superseded by [ADR 0004](0004-shared-session-and-editor-rules.md).

The application distinguishes a frontend command from a successful fact. For example, SaveDocument asks the application to write a file; DocumentSaved is produced only after the filesystem write succeeds. AppState applies events and produces AppSnapshot for rendering. A command may produce multiple ordered events, such as DocumentLoaded followed by DocumentOpened. A no-op produces no events and does not advance the revision.

Events are returned by dispatch_with_events; they are not persisted or broadcast by an event bus. The existing dispatch method returns only the snapshot so the native frontends keep their simple API. Filesystem watcher events, background work, and undo can build on this distinction in later phases without introducing an async runtime now.
