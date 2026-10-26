-- Durable current overrides (including tombstones) and immutable operation receipts.
CREATE TABLE tool_connection_overrides (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, connection TEXT NOT NULL,
 tool_record TEXT, PRIMARY KEY(workspace,profile,connection)
);
INSERT INTO tool_connection_overrides(workspace,profile,connection,tool_record)
 SELECT workspace,profile,json_extract(tool_record,'$.connection_id'),tool_record FROM managed_tool_connections;
CREATE INDEX active_tool_connection_overrides ON tool_connection_overrides(workspace,profile,connection) WHERE tool_record IS NOT NULL;
CREATE TABLE tool_connection_changes (
 workspace TEXT NOT NULL, profile TEXT NOT NULL, operation TEXT NOT NULL,
 actor TEXT NOT NULL, request_record TEXT NOT NULL, revision INTEGER NOT NULL,
 PRIMARY KEY(workspace,profile,operation),
 FOREIGN KEY(workspace,profile,revision) REFERENCES tool_releases(workspace,profile,revision)
);
PRAGMA user_version=14;
