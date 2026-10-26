-- Exact scoped lookups and deletion use the primary key; no redundant index.
CREATE TABLE connection_secrets (
 workspace TEXT NOT NULL, source TEXT NOT NULL, connection TEXT NOT NULL,
 encrypted BLOB NOT NULL,
 PRIMARY KEY(workspace, source, connection)
) WITHOUT ROWID;
PRAGMA user_version=18;
