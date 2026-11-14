-- Keep uploaded Remote Asset IDs across subsequent synchronization jobs until retry.
CREATE TABLE album_retry_remote_ids (
    local_path TEXT PRIMARY KEY NOT NULL,
    size INTEGER NOT NULL,
    last_modified INTEGER NOT NULL,
    remote_id TEXT NOT NULL
);
