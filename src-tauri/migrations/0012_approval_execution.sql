ALTER TABLE approval_requests ADD COLUMN execution_status TEXT NOT NULL DEFAULT 'not_started'
    CHECK (execution_status IN ('not_started', 'running', 'succeeded', 'failed'));
ALTER TABLE approval_requests ADD COLUMN execution_result_json TEXT
    CHECK (execution_result_json IS NULL OR json_valid(execution_result_json));
ALTER TABLE approval_requests ADD COLUMN execution_error_code TEXT;
ALTER TABLE approval_requests ADD COLUMN execution_error_message TEXT;
ALTER TABLE approval_requests ADD COLUMN executed_at TEXT;
