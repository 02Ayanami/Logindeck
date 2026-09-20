CREATE TABLE browser_capture_settings (
 id INTEGER PRIMARY KEY CHECK (id = 1),
 enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
 revision INTEGER NOT NULL DEFAULT 0,
 last_connected_at INTEGER
);
INSERT INTO browser_capture_settings (id) VALUES (1);
