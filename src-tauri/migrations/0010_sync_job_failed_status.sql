-- Execution setup failures must remain observable without changing queued work.
PRAGMA foreign_keys = OFF;

CREATE TABLE sync_jobs_new (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    status TEXT NOT NULL CHECK (status IN ('RUNNING', 'PAUSED', 'IDLE', 'FAILED')),
    next_sequence INTEGER NOT NULL DEFAULT 1,
    total_count INTEGER NOT NULL DEFAULT 0,
    success_count INTEGER NOT NULL DEFAULT 0,
    failure_count INTEGER NOT NULL DEFAULT 0,
    current_path TEXT
);

INSERT INTO sync_jobs_new (id, status, next_sequence, total_count, success_count, failure_count, current_path)
SELECT id, status, next_sequence, total_count, success_count, failure_count, current_path
FROM sync_jobs;

DROP TABLE sync_jobs;
ALTER TABLE sync_jobs_new RENAME TO sync_jobs;

PRAGMA foreign_keys = ON;
