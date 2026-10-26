-- Separate schema version from bounded event/delivery storage encoding versions.
CREATE TABLE config_releases (
 revision INTEGER PRIMARY KEY AUTOINCREMENT,
 record TEXT NOT NULL UNIQUE,
 created_ms INTEGER NOT NULL
);
INSERT INTO config_releases(record,created_ms) SELECT record,CAST(unixepoch('subsec')*1000 AS INTEGER) FROM pipeline_binding;
CREATE TABLE active_release(singleton INTEGER PRIMARY KEY CHECK(singleton=1), revision INTEGER NOT NULL REFERENCES config_releases(revision));
INSERT INTO active_release SELECT 1,revision FROM config_releases;
ALTER TABLE events ADD COLUMN config_revision INTEGER REFERENCES config_releases(revision);
ALTER TABLE deliveries ADD COLUMN config_revision INTEGER REFERENCES config_releases(revision);
UPDATE events SET config_revision=(SELECT MAX(revision) FROM config_releases);
UPDATE deliveries SET config_revision=(SELECT MAX(revision) FROM config_releases);
CREATE TABLE source_checkpoints (
 workspace TEXT NOT NULL, source TEXT NOT NULL, partition TEXT NOT NULL,
 generation INTEGER NOT NULL, revision INTEGER NOT NULL, cursor TEXT, event_id TEXT,
 PRIMARY KEY(workspace,source,partition)
);
CREATE TABLE content_binding(singleton INTEGER PRIMARY KEY CHECK(singleton=1),directory TEXT NOT NULL);
CREATE TABLE content_objects (
 id TEXT PRIMARY KEY, workspace TEXT NOT NULL, source TEXT NOT NULL,
 size INTEGER NOT NULL CHECK(size>=0), digest TEXT, media_type TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('uploading','staged','available','deleting')),
 created_ms INTEGER NOT NULL
);
CREATE TABLE content_usage(singleton INTEGER PRIMARY KEY CHECK(singleton=1),bytes INTEGER NOT NULL DEFAULT 0,items INTEGER NOT NULL DEFAULT 0);
INSERT INTO content_usage(singleton) VALUES(1);
CREATE TRIGGER content_charge AFTER INSERT ON content_objects BEGIN
 UPDATE content_usage SET bytes=bytes+NEW.size,items=items+1 WHERE singleton=1;
END;
CREATE TRIGGER content_release AFTER DELETE ON content_objects BEGIN
 UPDATE content_usage SET bytes=bytes-OLD.size,items=items-1 WHERE singleton=1;
END;
CREATE INDEX content_age ON content_objects(state,created_ms);
CREATE TABLE event_content (
 workspace TEXT NOT NULL, source TEXT NOT NULL, event_id TEXT NOT NULL,
 content_id TEXT NOT NULL REFERENCES content_objects(id),
 PRIMARY KEY(workspace,source,event_id,content_id),
 FOREIGN KEY(workspace,source,event_id) REFERENCES events(workspace,source,event_id)
);
CREATE INDEX event_content_reverse ON event_content(content_id);
CREATE TRIGGER content_unlink AFTER DELETE ON event_content BEGIN
 UPDATE content_objects SET state='staged' WHERE id=OLD.content_id AND state='available'
 AND NOT EXISTS(SELECT 1 FROM event_content WHERE content_id=OLD.content_id);
END;
CREATE TABLE retired_work (
 workspace TEXT NOT NULL, work_key TEXT NOT NULL, retired_ms INTEGER NOT NULL,
 payload_purged INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(workspace,work_key)
);
CREATE INDEX retired_age ON retired_work(workspace,retired_ms);
PRAGMA user_version=4;
-- One-time conservative retirement of old terminal work. Age starts at upgrade.
INSERT OR IGNORE INTO retired_work(workspace,work_key,retired_ms)
SELECT workspace,dispatch_key,CAST(unixepoch('subsec')*1000 AS INTEGER) FROM dispatches d
WHERE NOT EXISTS(SELECT 1 FROM pending_work p WHERE p.workspace=d.workspace AND p.work_key=d.dispatch_key)
AND NOT EXISTS(SELECT 1 FROM deliveries j WHERE j.workspace=d.workspace AND j.dispatch_key=d.dispatch_key AND j.state<>'submitted' AND j.resolution IS NULL);
INSERT OR IGNORE INTO retired_work(workspace,work_key,retired_ms)
SELECT workspace,'event:'||source||':'||event_id,CAST(unixepoch('subsec')*1000 AS INTEGER) FROM routing_tasks WHERE resolution='abandoned';
CREATE INDEX audit_retention ON audit(workspace,occurred_ms);
CREATE INDEX retired_payload_due ON retired_work(workspace,retired_ms) WHERE payload_purged=0;
-- Match due-time ordering within a target, avoiding the previous optional-target OR.
CREATE INDEX delivery_target_pending ON deliveries(workspace,target_id,due_ms) WHERE state='pending' AND resolution IS NULL;
CREATE INDEX delivery_unresolved_target ON deliveries(workspace,target_id) WHERE state<>'submitted' AND resolution IS NULL;
CREATE INDEX delivery_expired ON deliveries(workspace,state,lease_until_ms) WHERE lease_until_ms IS NOT NULL;
CREATE INDEX attempt_open ON delivery_attempts(workspace,delivery_id,lease_id) WHERE finished_ms IS NULL;

CREATE INDEX routing_ready_order ON routing_tasks(workspace,due_ms,event_id) WHERE state IN ('ready','retry_wait');
-- Superseded by smaller pending/unresolved indexes for the actual target-lane queries.
DROP INDEX delivery_target_due;
CREATE INDEX content_staged_age ON content_objects(created_ms) WHERE state IN ('uploading','staged');
