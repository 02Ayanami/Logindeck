-- Preserve existing migration checksums and repair counters for legacy labels.
UPDATE account_sequences SET last_number = MAX(last_number, COALESCE((
    SELECT MAX(CAST(CASE
        WHEN display_name LIKE '账号 %' THEN SUBSTR(display_name, 4)
        WHEN display_name LIKE 'Account %' THEN SUBSTR(display_name, 9)
    END AS INTEGER)) FROM application_accounts
    WHERE 'application:' || application_id = account_sequences.scope
), 0));
