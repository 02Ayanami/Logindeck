CREATE TABLE IF NOT EXISTS pending_secret_cleanup(
  secret_ref TEXT PRIMARY KEY,
  kind TEXT NOT NULL CHECK(kind IN ('website','application_account')),
  owner_id TEXT,
  context TEXT NOT NULL,
  attempts INTEGER NOT NULL DEFAULT 0 CHECK(attempts >= 0),
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
