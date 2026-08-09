UPDATE generic_provider_profiles
SET arguments_json = '["{prompt}"]'
WHERE provider_account_id IN (
    SELECT id FROM provider_accounts WHERE display_name = 'Claude Code'
)
AND json_array_length(arguments_json) = 2
AND json_extract(arguments_json, '$[0]') = '-p';

UPDATE generic_provider_profiles
SET arguments_json = '["{prompt}"]'
WHERE provider_account_id IN (
    SELECT id FROM provider_accounts WHERE display_name = 'Codex'
)
AND json_array_length(arguments_json) = 2
AND json_extract(arguments_json, '$[0]') = 'exec';

UPDATE generic_provider_profiles
SET arguments_json = '["-i","{prompt}"]'
WHERE provider_account_id IN (
    SELECT id FROM provider_accounts WHERE display_name = 'Gemini CLI'
)
AND json_array_length(arguments_json) = 2
AND json_extract(arguments_json, '$[0]') = '-p';
