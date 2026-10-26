CREATE TABLE message_connections (
 workspace TEXT NOT NULL, source TEXT NOT NULL, revision INTEGER NOT NULL, record TEXT NOT NULL,
 PRIMARY KEY(workspace,source)
);
CREATE TABLE message_connection_operations (
 workspace TEXT NOT NULL, source TEXT NOT NULL, operation_id TEXT NOT NULL, actor TEXT NOT NULL,
 request TEXT NOT NULL, revision INTEGER NOT NULL, created_ms INTEGER NOT NULL,
 PRIMARY KEY(workspace,source,operation_id)
);
CREATE INDEX routing_unresolved_source ON routing_tasks(workspace,source) WHERE state<>'completed';
PRAGMA user_version=16;
