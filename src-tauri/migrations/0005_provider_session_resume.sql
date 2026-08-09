ALTER TABLE generic_provider_profiles
ADD COLUMN resume_arguments_json TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(resume_arguments_json));

ALTER TABLE agent_runs ADD COLUMN provider_session_id TEXT;
ALTER TABLE agent_runs ADD COLUMN resume_count INTEGER NOT NULL DEFAULT 0 CHECK (resume_count >= 0);

UPDATE generic_provider_profiles
SET arguments_json = '["--session-id","{sessionId}","{prompt}"]',
    resume_arguments_json = '["--resume","{sessionId}"]'
WHERE provider_account_id IN (
    SELECT id FROM provider_accounts WHERE display_name = 'Claude Code'
);

UPDATE generic_provider_profiles
SET resume_arguments_json = '["resume","--last"]'
WHERE provider_account_id IN (
    SELECT id FROM provider_accounts WHERE display_name = 'Codex'
);
