ALTER TABLE sync_state ADD COLUMN failure_reason TEXT;

UPDATE sync_state
SET failure_reason = 'Failure details are unavailable; retry to obtain details.'
WHERE status = 'FAILED' AND (failure_reason IS NULL OR failure_reason = '');
