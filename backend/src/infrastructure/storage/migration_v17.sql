-- Adapter-owned discovery, isolated by workspace, connection and upstream account.
CREATE TABLE connection_conversations (
 workspace TEXT NOT NULL, connection TEXT NOT NULL, account TEXT NOT NULL,
 kind TEXT NOT NULL CHECK(kind IN ('group','direct')), id TEXT NOT NULL,
 last_seen_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,connection,account,kind,id)
) WITHOUT ROWID;
CREATE INDEX conversations_recent ON connection_conversations(workspace,connection,account,kind,last_seen_ms DESC,id);
PRAGMA user_version=17;
