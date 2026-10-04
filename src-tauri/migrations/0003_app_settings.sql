CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

INSERT INTO app_settings (key, value)
VALUES ('upload_parallelism', '3')
ON CONFLICT(key) DO NOTHING;
