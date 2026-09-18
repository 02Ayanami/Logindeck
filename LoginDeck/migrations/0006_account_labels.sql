ALTER TABLE websites ADD COLUMN account_name TEXT NOT NULL DEFAULT '';
CREATE TABLE account_sequences (
    scope TEXT PRIMARY KEY NOT NULL,
    last_number INTEGER NOT NULL DEFAULT 0
);
UPDATE websites SET account_name = '账号 ' || (
    SELECT COUNT(*) FROM websites AS sibling
    WHERE sibling.normalized_origin = websites.normalized_origin
      AND (sibling.created_at < websites.created_at OR
          (sibling.created_at = websites.created_at AND sibling.id <= websites.id))
);
INSERT INTO account_sequences(scope, last_number)
SELECT 'website:' || normalized_origin, COUNT(*) FROM websites GROUP BY normalized_origin;
INSERT INTO account_sequences(scope, last_number)
SELECT 'application:' || application_id, COUNT(*) FROM application_accounts GROUP BY application_id;
