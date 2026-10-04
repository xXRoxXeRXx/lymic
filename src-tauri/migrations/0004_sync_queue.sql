CREATE TABLE IF NOT EXISTS sync_jobs (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    status TEXT NOT NULL CHECK (status IN ('RUNNING', 'PAUSED', 'IDLE')),
    next_sequence INTEGER NOT NULL DEFAULT 1,
    total_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    current_path TEXT
);

INSERT OR IGNORE INTO sync_jobs (id, status) VALUES (1, 'IDLE');

CREATE TABLE IF NOT EXISTS sync_queue (
    job_id INTEGER NOT NULL REFERENCES sync_jobs(id) ON DELETE CASCADE,
    sequence INTEGER NOT NULL,
    local_path TEXT NOT NULL,
    size INTEGER NOT NULL,
    last_modified INTEGER NOT NULL,
    sha1 TEXT,
    md5 TEXT,
    sha256 TEXT,
    status TEXT NOT NULL CHECK (status IN ('PENDING', 'SYNCED', 'FAILED')) DEFAULT 'PENDING',
    failure_reason TEXT,
    PRIMARY KEY (job_id, local_path)
);

CREATE INDEX IF NOT EXISTS sync_queue_pending_order
    ON sync_queue(job_id, status, sequence);
