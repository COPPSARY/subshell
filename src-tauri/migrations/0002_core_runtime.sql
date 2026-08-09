ALTER TABLE tasks ADD COLUMN acceptance_criteria_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(acceptance_criteria_json));
ALTER TABLE tasks ADD COLUMN allowed_paths_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(allowed_paths_json));
ALTER TABLE tasks ADD COLUMN validation_commands_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(validation_commands_json));
ALTER TABLE tasks ADD COLUMN decisions_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(decisions_json));

ALTER TABLE agent_runs ADD COLUMN context_pack_path TEXT;
ALTER TABLE agent_runs ADD COLUMN context_manifest_json TEXT CHECK (context_manifest_json IS NULL OR json_valid(context_manifest_json));
ALTER TABLE agent_runs ADD COLUMN context_sha256 TEXT;

ALTER TABLE worktrees ADD COLUMN environment_manifest_json TEXT CHECK (environment_manifest_json IS NULL OR json_valid(environment_manifest_json));

CREATE TABLE generic_provider_profiles (
    provider_account_id TEXT PRIMARY KEY NOT NULL REFERENCES provider_accounts(id) ON DELETE CASCADE,
    executable_path TEXT NOT NULL,
    arguments_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(arguments_json)),
    prompt_mode TEXT NOT NULL CHECK (prompt_mode IN ('argument', 'stdin')),
    config_root_env_var TEXT
);
