CREATE TABLE projects (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    path TEXT NOT NULL UNIQUE,
    last_opened_at TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE provider_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    provider_type TEXT NOT NULL,
    display_name TEXT NOT NULL,
    config_scope_path TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL CHECK (status IN ('active', 'needs_reauth', 'revoked')),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    removed_at TEXT
);

CREATE TABLE tasks (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL CHECK (
        status IN (
            'idea', 'task', 'queued', 'working', 'waiting', 'review',
            'approved', 'merged', 'archived', 'failed', 'cancelled'
        )
    ),
    base_branch TEXT NOT NULL,
    base_revision TEXT NOT NULL,
    queue_position INTEGER,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    archived_at TEXT
);

CREATE TABLE agent_runs (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    provider_account_id TEXT NOT NULL REFERENCES provider_accounts(id) ON DELETE RESTRICT,
    instruction TEXT NOT NULL,
    status TEXT NOT NULL CHECK (
        status IN ('queued', 'preparing', 'running', 'waiting', 'succeeded', 'failed', 'cancelled')
    ),
    merge_order INTEGER NOT NULL DEFAULT 0,
    raw_log_path TEXT,
    process_identity TEXT,
    waiting_reason TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    ended_at TEXT
);

CREATE TABLE worktrees (
    id TEXT PRIMARY KEY NOT NULL,
    agent_run_id TEXT NOT NULL UNIQUE REFERENCES agent_runs(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE,
    base_branch TEXT NOT NULL,
    base_revision TEXT NOT NULL,
    branch_name TEXT,
    state TEXT NOT NULL CHECK (state IN ('active', 'merged', 'discarded')),
    created_at TEXT NOT NULL,
    cleaned_at TEXT
);

CREATE TABLE timeline_events (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(id) ON DELETE CASCADE,
    provider_account_id TEXT REFERENCES provider_accounts(id) ON DELETE RESTRICT,
    sequence INTEGER NOT NULL,
    event_type TEXT NOT NULL,
    payload_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(payload_json)),
    created_at TEXT NOT NULL,
    UNIQUE (project_id, sequence)
);

CREATE TABLE review_records (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL UNIQUE REFERENCES tasks(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE review_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    review_record_id TEXT NOT NULL REFERENCES review_records(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    base_revision TEXT NOT NULL,
    input_fingerprint TEXT NOT NULL,
    combined_diff_path TEXT NOT NULL,
    decision TEXT NOT NULL DEFAULT 'pending' CHECK (decision IN ('pending', 'approved', 'sent_back')),
    feedback TEXT,
    created_at TEXT NOT NULL,
    decided_at TEXT,
    UNIQUE (review_record_id, attempt_number)
);

CREATE TABLE conflict_flags (
    id TEXT PRIMARY KEY NOT NULL,
    review_attempt_id TEXT NOT NULL REFERENCES review_attempts(id) ON DELETE CASCADE,
    category TEXT NOT NULL CHECK (category IN ('same_file', 'related_file', 'shared_text_reference')),
    evidence_json TEXT NOT NULL CHECK (json_valid(evidence_json)),
    created_at TEXT NOT NULL
);

CREATE TABLE context_shares (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    source_agent_run_id TEXT REFERENCES agent_runs(id) ON DELETE SET NULL,
    target_agent_run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('file', 'output_excerpt', 'summary')),
    content_reference TEXT,
    content_summary TEXT NOT NULL,
    delivery_status TEXT NOT NULL CHECK (delivery_status IN ('pending', 'delivered', 'failed')),
    created_at TEXT NOT NULL,
    delivered_at TEXT
);

CREATE TABLE app_settings (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX tasks_project_status_idx ON tasks(project_id, status);
CREATE INDEX agent_runs_task_status_idx ON agent_runs(task_id, status);
CREATE INDEX timeline_events_project_sequence_idx ON timeline_events(project_id, sequence);
CREATE INDEX context_shares_target_idx ON context_shares(target_agent_run_id, created_at);

