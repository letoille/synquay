CREATE TABLE capacity_policy(singleton INTEGER PRIMARY KEY CHECK(singleton=1), record TEXT NOT NULL);
CREATE TABLE capacity_state(
 workspace TEXT PRIMARY KEY, items INTEGER NOT NULL DEFAULT 0 CHECK(items>=0),
 bytes INTEGER NOT NULL DEFAULT 0 CHECK(bytes>=0), paused INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE pending_work(
 workspace TEXT NOT NULL, work_key TEXT NOT NULL, bytes INTEGER NOT NULL CHECK(bytes>=0),
 admitted_ms INTEGER NOT NULL, PRIMARY KEY(workspace,work_key)
);
CREATE INDEX pending_work_age ON pending_work(workspace,admitted_ms);
CREATE TRIGGER pending_work_insert AFTER INSERT ON pending_work BEGIN
 INSERT INTO capacity_state(workspace,items,bytes) VALUES(NEW.workspace,1,NEW.bytes)
 ON CONFLICT(workspace) DO UPDATE SET items=items+1,bytes=bytes+NEW.bytes;
END;
CREATE TRIGGER pending_work_delete AFTER DELETE ON pending_work BEGIN
 UPDATE capacity_state SET items=items-1,bytes=bytes-OLD.bytes WHERE workspace=OLD.workspace;
END;
CREATE TABLE routing_tasks(
 workspace TEXT NOT NULL, source TEXT NOT NULL, event_id TEXT NOT NULL,
 state TEXT NOT NULL CHECK(state IN ('ready','retry_wait','blocked','completed')),
 attempt INTEGER NOT NULL DEFAULT 0, due_ms INTEGER NOT NULL, error_code TEXT,
 PRIMARY KEY(workspace,source,event_id),
 FOREIGN KEY(workspace,source,event_id) REFERENCES events(workspace,source,event_id)
);
CREATE INDEX routing_due ON routing_tasks(workspace,state,due_ms,event_id);
CREATE INDEX deliveries_dispatch ON deliveries(workspace,dispatch_key,state);
INSERT INTO routing_tasks(workspace,source,event_id,state,due_ms)
 SELECT e.workspace,e.source,e.event_id,
 CASE WHEN EXISTS(SELECT 1 FROM dispatches d WHERE d.workspace=e.workspace AND d.dispatch_key='event:'||e.source||':'||e.event_id)
 THEN 'completed' ELSE 'ready' END,e.received_ms FROM events e;
INSERT INTO pending_work(workspace,work_key,bytes,admitted_ms)
 SELECT e.workspace,'event:'||e.source||':'||e.event_id,length(CAST(e.record AS BLOB)),e.received_ms FROM events e
 WHERE NOT EXISTS(SELECT 1 FROM dispatches d WHERE d.workspace=e.workspace AND d.dispatch_key='event:'||e.source||':'||e.event_id)
 OR EXISTS(SELECT 1 FROM deliveries j WHERE j.workspace=e.workspace AND j.dispatch_key='event:'||e.source||':'||e.event_id AND j.state<>'submitted');
INSERT INTO pending_work(workspace,work_key,bytes,admitted_ms)
 SELECT d.workspace,d.dispatch_key,SUM(length(CAST(d.record AS BLOB))),MIN(d.due_ms)
 FROM deliveries d WHERE d.dispatch_key LIKE 'reply:%'
 AND EXISTS(SELECT 1 FROM deliveries j WHERE j.workspace=d.workspace AND j.dispatch_key=d.dispatch_key AND j.state<>'submitted')
 GROUP BY d.workspace,d.dispatch_key;
PRAGMA user_version=2;
