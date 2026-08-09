ALTER TABLE agent_runs ADD COLUMN role TEXT NOT NULL DEFAULT 'executor';
ALTER TABLE agent_runs ADD COLUMN assignment_title TEXT;

CREATE TABLE task_plans (
    id TEXT PRIMARY KEY NOT NULL,
    task_id TEXT NOT NULL REFERENCES tasks(id) ON DELETE CASCADE,
    planner_run_id TEXT NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL CHECK (attempt_number > 0),
    summary TEXT NOT NULL DEFAULT '',
    status TEXT NOT NULL DEFAULT 'proposed' CHECK (status IN ('proposed', 'launched', 'rejected')),
    created_at TEXT NOT NULL,
    launched_at TEXT,
    UNIQUE (task_id, attempt_number)
);

CREATE TABLE task_plan_assignments (
    id TEXT PRIMARY KEY NOT NULL,
    plan_id TEXT NOT NULL REFERENCES task_plans(id) ON DELETE CASCADE,
    title TEXT NOT NULL,
    instruction TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'executor',
    allowed_paths_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(allowed_paths_json)),
    depends_on_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(depends_on_json)),
    position INTEGER NOT NULL CHECK (position >= 0),
    UNIQUE (plan_id, position)
);

ALTER TABLE review_attempts ADD COLUMN snapshot_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(snapshot_json));
ALTER TABLE review_attempts ADD COLUMN validation_evidence_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(validation_evidence_json));

CREATE TABLE merge_attempts (
    id TEXT PRIMARY KEY NOT NULL,
    review_attempt_id TEXT NOT NULL REFERENCES review_attempts(id) ON DELETE CASCADE,
    target_branch TEXT NOT NULL,
    expected_target_revision TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'running' CHECK (status IN ('running', 'succeeded', 'failed')),
    result_revision TEXT,
    error_code TEXT,
    error_message TEXT,
    created_at TEXT NOT NULL,
    completed_at TEXT
);

CREATE TABLE run_branches (
    agent_run_id TEXT PRIMARY KEY NOT NULL REFERENCES agent_runs(id) ON DELETE CASCADE,
    review_attempt_id TEXT NOT NULL REFERENCES review_attempts(id) ON DELETE CASCADE,
    branch_name TEXT NOT NULL UNIQUE,
    revision TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX task_plans_task_status_idx ON task_plans(task_id, status);
CREATE INDEX merge_attempts_review_idx ON merge_attempts(review_attempt_id, created_at);
