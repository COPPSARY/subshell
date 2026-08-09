ALTER TABLE agent_runs ADD COLUMN full_access INTEGER NOT NULL DEFAULT 0 CHECK (full_access IN (0, 1));
