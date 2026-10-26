-- PKs support profile lookup, release history and idempotency lookup without extra indexes.
CREATE TABLE tool_releases (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, revision INTEGER NOT NULL CHECK(revision>0),
 record TEXT NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile,revision)
);
CREATE TABLE tool_heads (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, revision INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile),
 FOREIGN KEY(workspace,profile,revision) REFERENCES tool_releases(workspace,profile,revision)
);
CREATE TABLE tool_publications (
 id TEXT PRIMARY KEY, workspace TEXT NOT NULL, profile TEXT NOT NULL,
 revision INTEGER NOT NULL, principal TEXT NOT NULL, occurred_ms INTEGER NOT NULL,
 FOREIGN KEY(workspace,profile,revision) REFERENCES tool_releases(workspace,profile,revision)
);
CREATE TABLE tool_invocations (
 workspace TEXT NOT NULL, principal TEXT NOT NULL, invocation TEXT NOT NULL,
 fingerprint TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('running','completed')),
 outcome TEXT, created_ms INTEGER NOT NULL, finished_ms INTEGER,
 budget_bytes INTEGER NOT NULL CHECK(budget_bytes>=0),
 PRIMARY KEY(workspace,principal,invocation)
);
CREATE TABLE tool_invocation_usage(singleton INTEGER PRIMARY KEY CHECK(singleton=1),items INTEGER NOT NULL,bytes INTEGER NOT NULL);
INSERT INTO tool_invocation_usage VALUES(1,0,0);
CREATE TRIGGER tool_invocation_insert AFTER INSERT ON tool_invocations BEGIN
 UPDATE tool_invocation_usage SET items=items+1,bytes=bytes+NEW.budget_bytes WHERE singleton=1;
END;
CREATE TRIGGER tool_invocation_update AFTER UPDATE OF budget_bytes ON tool_invocations BEGIN
 UPDATE tool_invocation_usage SET bytes=bytes+NEW.budget_bytes-OLD.budget_bytes WHERE singleton=1;
END;
CREATE TRIGGER tool_invocation_delete AFTER DELETE ON tool_invocations BEGIN
 UPDATE tool_invocation_usage SET items=items-1,bytes=bytes-OLD.budget_bytes WHERE singleton=1;
END;
PRAGMA user_version=5;
