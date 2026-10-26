-- One bounded routing snapshot per ingress. Empty snapshots prevent deleted routes reseeding.
CREATE TABLE managed_routes (
    workspace TEXT NOT NULL,
    source TEXT NOT NULL,
    revision INTEGER NOT NULL,
    record TEXT NOT NULL,
    PRIMARY KEY(workspace, source)
);
-- Immutable receipts double as an operator audit. Scope and actor are server-owned.
CREATE TABLE route_operations (
    workspace TEXT NOT NULL,
    source TEXT NOT NULL,
    operation_id TEXT NOT NULL,
    actor TEXT NOT NULL,
    request TEXT NOT NULL,
    revision INTEGER NOT NULL,
    created_ms INTEGER NOT NULL,
    PRIMARY KEY(workspace, source, operation_id)
);
ALTER TABLE deliveries ADD COLUMN route_rule_id TEXT;
UPDATE deliveries SET route_rule_id=COALESCE(json_extract(record,'$.data.route_rule[0]'),json_extract(record,'$.route_rule[0]'));
CREATE INDEX deliveries_by_route_state ON deliveries(workspace,route_rule_id,state) WHERE route_rule_id IS NOT NULL;
PRAGMA user_version=15;
