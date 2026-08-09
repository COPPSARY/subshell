ALTER TABLE generic_provider_profiles
ADD COLUMN inherit_user_home INTEGER NOT NULL DEFAULT 0 CHECK (inherit_user_home IN (0, 1));
