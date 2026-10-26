CREATE TABLE IF NOT EXISTS events (
 workspace TEXT NOT NULL, source TEXT NOT NULL, event_id TEXT NOT NULL,
 source_event_id TEXT, received_ms INTEGER NOT NULL, record TEXT NOT NULL,
 PRIMARY KEY(workspace, source, event_id), UNIQUE(workspace, source, source_event_id)
);
CREATE TABLE IF NOT EXISTS dispatches (
 workspace TEXT NOT NULL, dispatch_key TEXT NOT NULL, receipt TEXT NOT NULL,
 reply_fingerprint TEXT, PRIMARY KEY(workspace, dispatch_key)
);
CREATE TABLE IF NOT EXISTS deliveries (
 workspace TEXT NOT NULL, delivery_id TEXT NOT NULL, dispatch_key TEXT NOT NULL,
 record TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('pending','leased','sending','submitted','failed','reconcile')),
 due_ms INTEGER NOT NULL, attempt INTEGER NOT NULL DEFAULT 0,
 lease_id TEXT, lease_until_ms INTEGER, result TEXT,
 PRIMARY KEY(workspace, delivery_id),
 FOREIGN KEY(workspace, dispatch_key) REFERENCES dispatches(workspace, dispatch_key)
);
CREATE INDEX IF NOT EXISTS deliveries_ready ON deliveries(workspace, state, due_ms);
CREATE INDEX IF NOT EXISTS events_order ON events(workspace, received_ms);
CREATE TABLE IF NOT EXISTS audit (
 record_id TEXT PRIMARY KEY, workspace TEXT NOT NULL, occurred_ms INTEGER NOT NULL, record TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS pipeline_binding (
 singleton INTEGER PRIMARY KEY CHECK(singleton=1), record TEXT NOT NULL
);
PRAGMA user_version = 1;
