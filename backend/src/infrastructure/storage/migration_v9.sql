-- Additive admin-managed HTTP connections; PK is also the operation recovery key.
CREATE TABLE managed_tool_connections (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, operation TEXT NOT NULL,
 actor TEXT NOT NULL, expected_revision INTEGER NOT NULL, revision INTEGER NOT NULL,
 request_record TEXT NOT NULL, tool_record TEXT NOT NULL,
 PRIMARY KEY(workspace,profile,operation),
 UNIQUE(workspace,profile,revision),
 FOREIGN KEY(workspace,profile,revision) REFERENCES tool_releases(workspace,profile,revision)
);
PRAGMA user_version=9;
