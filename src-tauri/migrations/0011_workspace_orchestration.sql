ALTER TABLE projects ADD COLUMN unit_limit INTEGER CHECK (unit_limit IS NULL OR unit_limit > 0);

ALTER TABLE tasks ADD COLUMN unit_limit INTEGER CHECK (unit_limit IS NULL OR unit_limit > 0);

ALTER TABLE agent_runs ADD COLUMN depends_on_run_ids_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(depends_on_run_ids_json));
ALTER TABLE agent_runs ADD COLUMN retry_of_run_id TEXT REFERENCES agent_runs(id) ON DELETE SET NULL;
ALTER TABLE agent_runs ADD COLUMN unit_limit INTEGER CHECK (unit_limit IS NULL OR unit_limit > 0);

CREATE TABLE agent_templates (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    provider_account_id TEXT REFERENCES provider_accounts(id) ON DELETE SET NULL,
    role TEXT NOT NULL,
    instruction TEXT NOT NULL DEFAULT '',
    environment_files_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(environment_files_json)),
    unit_limit INTEGER CHECK (unit_limit IS NULL OR unit_limit > 0),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (project_id, name)
);

CREATE TABLE environment_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    environment_files_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(environment_files_json)),
    validation_commands_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(validation_commands_json)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE (project_id, name)
);

CREATE TABLE bookmarks (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(id) ON DELETE CASCADE,
    timeline_event_id TEXT REFERENCES timeline_events(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL,
    CHECK (task_id IS NOT NULL OR agent_run_id IS NOT NULL OR timeline_event_id IS NOT NULL)
);

CREATE TABLE workspace_snapshots (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id TEXT REFERENCES tasks(id) ON DELETE CASCADE,
    agent_run_id TEXT REFERENCES agent_runs(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('checkpoint', 'snapshot')),
    label TEXT NOT NULL,
    base_revision TEXT NOT NULL,
    patch_path TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE merge_queue (
    id TEXT PRIMARY KEY NOT NULL,
    project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    review_attempt_id TEXT NOT NULL REFERENCES review_attempts(id) ON DELETE CASCADE,
    review_fingerprint TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'queued' CHECK (status IN ('queued', 'running', 'succeeded', 'failed', 'cancelled')),
    result_revision TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    completed_at TEXT,
    UNIQUE (review_attempt_id)
);

CREATE INDEX agent_templates_project_idx ON agent_templates(project_id, name);
CREATE INDEX bookmarks_project_idx ON bookmarks(project_id, created_at);
CREATE INDEX workspace_snapshots_project_idx ON workspace_snapshots(project_id, created_at);
CREATE INDEX merge_queue_project_status_idx ON merge_queue(project_id, status, created_at);
