CREATE TABLE workspace_session (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    root TEXT NOT NULL,
    active_document TEXT,
    preview_document TEXT
);
CREATE TABLE session_documents (
    position INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE
);
