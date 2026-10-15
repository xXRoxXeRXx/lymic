ALTER TABLE sync_queue ADD COLUMN upload_unit_id TEXT;
ALTER TABLE sync_queue ADD COLUMN is_primary INTEGER NOT NULL DEFAULT 1;

-- Existing one-asset queue records are each their own Upload Unit.
UPDATE sync_queue SET upload_unit_id = local_path WHERE upload_unit_id IS NULL;

CREATE INDEX IF NOT EXISTS sync_queue_pending_units
    ON sync_queue(job_id, status, upload_unit_id, sequence);
