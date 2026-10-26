ALTER TABLE deliveries ADD COLUMN target_id TEXT;
ALTER TABLE deliveries ADD COLUMN resolution TEXT;
ALTER TABLE deliveries ADD COLUMN recovery_revision INTEGER NOT NULL DEFAULT 0;
ALTER TABLE deliveries ADD COLUMN parent_delivery_id TEXT;
UPDATE deliveries SET target_id=json_extract(record,'$.destination.id');
CREATE INDEX delivery_target_due ON deliveries(workspace,target_id,state,due_ms);
ALTER TABLE routing_tasks ADD COLUMN resolution TEXT;
ALTER TABLE routing_tasks ADD COLUMN recovery_revision INTEGER NOT NULL DEFAULT 0;
CREATE TABLE delivery_attempts(
 workspace TEXT NOT NULL, delivery_id TEXT NOT NULL, attempt INTEGER NOT NULL,
 lease_id TEXT NOT NULL, claimed_ms INTEGER NOT NULL, started_ms INTEGER,
 finished_ms INTEGER, outcome TEXT NOT NULL,
 PRIMARY KEY(workspace,delivery_id,attempt),
 FOREIGN KEY(workspace,delivery_id) REFERENCES deliveries(workspace,delivery_id)
);
CREATE TABLE recovery_actions(
 workspace TEXT NOT NULL, command_id TEXT NOT NULL, principal TEXT NOT NULL,
 occurred_ms INTEGER NOT NULL, command TEXT NOT NULL, receipt TEXT NOT NULL, target_key TEXT NOT NULL,
 PRIMARY KEY(workspace,command_id)
);
CREATE INDEX recovery_by_target ON recovery_actions(workspace,target_key,occurred_ms);
PRAGMA user_version=3;
