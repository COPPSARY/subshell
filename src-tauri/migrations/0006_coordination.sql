CREATE TABLE approval_requests (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(id) ON DELETE CASCADE,
    action TEXT NOT NULL,
    arguments_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(arguments_json)),
    status TEXT NOT NULL DEFAULT 'pending' CHECK (status IN ('pending', 'approved', 'denied', 'expired')),
    requested_by TEXT NOT NULL DEFAULT 'agent',
    created_at TEXT NOT NULL,
    decided_at TEXT
);

CREATE TABLE attention_acknowledgements (
    item_key TEXT PRIMARY KEY NOT NULL,
    state_fingerprint TEXT NOT NULL,
    acknowledged_at TEXT NOT NULL
);

CREATE TABLE notification_deliveries (
    item_key TEXT PRIMARY KEY NOT NULL,
    state_fingerprint TEXT NOT NULL,
    last_notified_at TEXT NOT NULL
);

ALTER TABLE context_shares ADD COLUMN preview_sha256 TEXT;
ALTER TABLE context_shares ADD COLUMN size_bytes INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0);
ALTER TABLE context_shares ADD COLUMN delivery_error TEXT;

CREATE INDEX approval_requests_project_status_idx ON approval_requests(project_id, status, created_at);
