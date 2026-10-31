-- Keep durable queue window selection bounded and ordered by the earliest member
-- of each Upload Unit. The sequence index already has the requested shape.
CREATE INDEX IF NOT EXISTS sync_queue_pending_order
    ON sync_queue(job_id, status, sequence);

-- Replace the broader legacy index so the new lookup index does not add redundant
-- write overhead. Queue window ordering is served by sync_queue_pending_order.
DROP INDEX IF EXISTS sync_queue_pending_units;

CREATE INDEX IF NOT EXISTS sync_queue_pending_upload_unit
    ON sync_queue(job_id, status, upload_unit_id);
