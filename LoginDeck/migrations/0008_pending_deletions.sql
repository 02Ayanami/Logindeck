CREATE TABLE pending_deletions (
    kind TEXT NOT NULL CHECK(kind IN ('website','account','application')),
    owner_id TEXT NOT NULL,
    PRIMARY KEY(kind, owner_id)
);
