UPDATE provider_accounts AS candidate
SET removed_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now'),
    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
WHERE candidate.removed_at IS NULL
  AND EXISTS (
    SELECT 1
    FROM generic_provider_profiles AS profile
    WHERE profile.provider_account_id = candidate.id
      AND profile.inherit_user_home = 1
  )
  AND NOT EXISTS (
    SELECT 1
    FROM agent_runs AS run
    WHERE run.provider_account_id = candidate.id
  )
  AND candidate.id <> COALESCE(
    (
      SELECT setting.value
      FROM app_settings AS setting
      JOIN provider_accounts AS preferred ON preferred.id = setting.value
      JOIN generic_provider_profiles AS preferred_profile
        ON preferred_profile.provider_account_id = preferred.id
      WHERE setting.key = 'default_provider_account_id'
        AND preferred.removed_at IS NULL
        AND preferred.provider_type = candidate.provider_type
        AND preferred_profile.inherit_user_home = 1
    ),
    (
      SELECT referenced.id
      FROM provider_accounts AS referenced
      JOIN generic_provider_profiles AS referenced_profile
        ON referenced_profile.provider_account_id = referenced.id
      WHERE referenced.removed_at IS NULL
        AND referenced.provider_type = candidate.provider_type
        AND referenced_profile.inherit_user_home = 1
        AND EXISTS (
          SELECT 1 FROM agent_runs AS run WHERE run.provider_account_id = referenced.id
        )
      ORDER BY referenced.created_at, referenced.id
      LIMIT 1
    ),
    (
      SELECT earliest.id
      FROM provider_accounts AS earliest
      JOIN generic_provider_profiles AS earliest_profile
        ON earliest_profile.provider_account_id = earliest.id
      WHERE earliest.removed_at IS NULL
        AND earliest.provider_type = candidate.provider_type
        AND earliest_profile.inherit_user_home = 1
      ORDER BY earliest.created_at, earliest.id
      LIMIT 1
    )
  );
