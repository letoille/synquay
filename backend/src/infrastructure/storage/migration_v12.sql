CREATE TABLE mcp_servers (
 workspace TEXT NOT NULL,
 id TEXT NOT NULL,
 name TEXT NOT NULL,
 deleted INTEGER NOT NULL DEFAULT 0,
 record TEXT NOT NULL,
 last_operation TEXT,
 last_request TEXT,
 PRIMARY KEY(workspace,id)
);
CREATE UNIQUE INDEX mcp_server_names ON mcp_servers(workspace,lower(name)) WHERE deleted=0;
CREATE INDEX mcp_server_list ON mcp_servers(workspace,deleted,name,id);
CREATE TABLE mcp_server_changes (
 workspace TEXT NOT NULL, server_id TEXT NOT NULL, revision INTEGER NOT NULL,
 actor TEXT NOT NULL, operation TEXT NOT NULL, record TEXT NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,server_id,revision)
);
PRAGMA user_version=12;
