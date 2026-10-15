CREATE TABLE IF NOT EXISTS removable_watched_folders (
    watched_folder_id INTEGER PRIMARY KEY REFERENCES watched_folders(id) ON DELETE CASCADE,
    volume_id TEXT NOT NULL,
    relative_path TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_removable_watched_folders_volume
    ON removable_watched_folders(volume_id);
