-- Rebuild to allow accounts which never own a password credential.
CREATE TABLE application_accounts_next (
 id TEXT PRIMARY KEY,
 application_id TEXT NOT NULL REFERENCES applications(id),
 display_name TEXT NOT NULL,
 username TEXT NOT NULL,
 login_method TEXT NOT NULL DEFAULT 'password' CHECK(login_method IN ('password','sms','qr')),
 phone TEXT,
 password_credential_ref TEXT,
 auto_submit_enabled INTEGER NOT NULL CHECK(auto_submit_enabled IN (0,1)),
 last_login_status TEXT, last_login_at TEXT, created_at TEXT NOT NULL, updated_at TEXT NOT NULL,
 CHECK ((login_method='password' AND password_credential_ref IS NOT NULL AND length(trim(username))>0 AND phone IS NULL)
 OR (login_method='sms' AND password_credential_ref IS NULL AND phone IS NOT NULL AND length(trim(phone))>0 AND username='')
 OR (login_method='qr' AND password_credential_ref IS NULL AND phone IS NULL)),
 CHECK(login_method='password' OR auto_submit_enabled=0)
);
INSERT INTO application_accounts_next (id,application_id,display_name,username,password_credential_ref,auto_submit_enabled,last_login_status,last_login_at,created_at,updated_at)
 SELECT id,application_id,display_name,username,password_credential_ref,auto_submit_enabled,last_login_status,last_login_at,created_at,updated_at FROM application_accounts;
DROP TABLE application_accounts;
ALTER TABLE application_accounts_next RENAME TO application_accounts;
