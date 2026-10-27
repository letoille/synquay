-- Cumulative counters survive retention cleanup. Primary keys cover scoped lookups.
CREATE TABLE message_totals (
 workspace TEXT NOT NULL, kind TEXT NOT NULL, id TEXT NOT NULL,
 total INTEGER NOT NULL DEFAULT 0,
 PRIMARY KEY(workspace,kind,id)
) WITHOUT ROWID;
INSERT INTO message_totals SELECT workspace,'received',source,COUNT(*) FROM events GROUP BY workspace,source;
INSERT INTO message_totals SELECT workspace,'sent',target_id,COUNT(*) FROM deliveries WHERE state='submitted' GROUP BY workspace,target_id;
-- Seed route counts from retained history, once per input dispatch rather than destination.
INSERT INTO message_totals
 SELECT m.workspace,'matched',json_extract(r.value,'$.id'),COUNT(DISTINCT d.dispatch_key)
 FROM managed_routes m, json_each(m.record,'$.routes') r, json_each(r.value,'$.targets') t
 JOIN deliveries d ON d.workspace=m.workspace AND d.route_rule_id=json_extract(t.value,'$.rule_id')
 GROUP BY m.workspace,json_extract(r.value,'$.id');
ALTER TABLE deliveries ADD COLUMN counted_submitted INTEGER NOT NULL DEFAULT 0;
UPDATE deliveries SET counted_submitted=1 WHERE state='submitted';
CREATE TRIGGER count_received AFTER INSERT ON events BEGIN
 INSERT INTO message_totals(workspace,kind,id,total) VALUES(NEW.workspace,'received',NEW.source,1)
 ON CONFLICT(workspace,kind,id) DO UPDATE SET total=total+1;
END;
CREATE TRIGGER count_submitted AFTER UPDATE OF state ON deliveries
 WHEN NEW.state='submitted' AND NEW.counted_submitted=0 BEGIN
 INSERT INTO message_totals(workspace,kind,id,total) VALUES(NEW.workspace,'sent',NEW.target_id,1)
 ON CONFLICT(workspace,kind,id) DO UPDATE SET total=total+1;
 UPDATE deliveries SET counted_submitted=1 WHERE workspace=NEW.workspace AND delivery_id=NEW.delivery_id;
END;
PRAGMA user_version=19;
