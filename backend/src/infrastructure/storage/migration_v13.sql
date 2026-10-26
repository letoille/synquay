ALTER TABLE mcp_servers ADD COLUMN route_path TEXT;
UPDATE mcp_servers SET route_path='/mcp/' || id;
CREATE UNIQUE INDEX mcp_server_routes ON mcp_servers(workspace,route_path) WHERE deleted=0;
PRAGMA user_version=13;
