CREATE TABLE IF NOT EXISTS watched_folders (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    path TEXT NOT NULL UNIQUE,
    recursive BOOLEAN NOT NULL DEFAULT 1,
    target_album_id TEXT
);

CREATE TABLE IF NOT EXISTS sync_state (
    local_path TEXT PRIMARY KEY,
    file_hash TEXT NOT NULL,
    last_modified INTEGER NOT NULL,
    size INTEGER NOT NULL,
    status TEXT NOT NULL,
    remote_id TEXT
);
