CREATE TABLE recent_workspaces (
    path TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    opened_order INTEGER NOT NULL
);
CREATE INDEX recent_workspaces_order_idx ON recent_workspaces(opened_order DESC);
