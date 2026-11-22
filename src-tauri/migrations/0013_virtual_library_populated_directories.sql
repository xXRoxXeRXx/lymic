-- Directories of the Virtual Library that were listed once and handed to the platform (#280).
-- Reconciliation diffs exactly these. `path` is relative to the Virtual Library root, `/`
-- separated; the root itself is the empty string.
CREATE TABLE IF NOT EXISTS virtual_library_populated_directories (
    path TEXT PRIMARY KEY NOT NULL
);
