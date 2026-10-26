CREATE TABLE workspaces (
 id TEXT PRIMARY KEY,
 name TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('active','suspended')),
 revision INTEGER NOT NULL CHECK(revision > 0)
);
CREATE TABLE workspace_members (
 workspace TEXT NOT NULL REFERENCES workspaces(id),
 principal TEXT NOT NULL,
 role TEXT NOT NULL CHECK(role IN ('owner','administrator','operator','viewer')),
 state TEXT NOT NULL CHECK(state IN ('active','revoked')),
 PRIMARY KEY(workspace,principal)
);
-- Last-owner check is a bounded existence query, independent of member count.
CREATE INDEX workspace_active_owners ON workspace_members(workspace,principal)
 WHERE role='owner' AND state='active';
CREATE TABLE workspace_changes (
 workspace TEXT NOT NULL REFERENCES workspaces(id),
 revision INTEGER NOT NULL,
 actor TEXT NOT NULL,
 request_id TEXT NOT NULL,
 occurred_ms INTEGER NOT NULL,
 change_record TEXT NOT NULL,
 PRIMARY KEY(workspace,revision)
);
PRAGMA user_version=8;
