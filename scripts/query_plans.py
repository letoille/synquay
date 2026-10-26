#!/usr/bin/env python3
"""Reproducible query-plan fixture (temporary DB only, no production modifications)."""
import json
from pathlib import Path
import sqlite3
import tempfile

ROOT = Path(__file__).resolve().parents[1]
QUERIES = {
    "target_claim": "SELECT delivery_id FROM deliveries WHERE workspace='w' AND target_id='t1' AND state='pending' AND resolution IS NULL AND due_ms<=10000 ORDER BY due_ms,rowid LIMIT 1",
    "strict_predecessor": "SELECT 1 FROM deliveries WHERE workspace='w' AND target_id='t1' AND rowid<100 AND state<>'submitted' AND resolution IS NULL LIMIT 1",
    "expired_lease": "SELECT delivery_id FROM deliveries WHERE workspace='w' AND state='sending' AND lease_until_ms<=100",
    "route_ready": "SELECT source,event_id FROM routing_tasks WHERE workspace='w' AND state IN ('ready','retry_wait') AND due_ms<=10000 ORDER BY due_ms,event_id LIMIT 100",
    "audit_retention": "SELECT rowid FROM audit WHERE workspace='w' AND occurred_ms<=10000 ORDER BY occurred_ms LIMIT 100",
    "payload_retention": "SELECT rowid FROM retired_work WHERE workspace='w' AND payload_purged=0 AND retired_ms<=100 LIMIT 100",
    "content_retry_delete": "SELECT id FROM content_objects WHERE state='deleting' ORDER BY created_ms LIMIT 100",
    "content_expired_staging": "SELECT id FROM content_objects WHERE state IN ('uploading','staged') AND created_ms<100 ORDER BY created_ms LIMIT 100",
    "content_reverse_reference": "SELECT event_id FROM event_content WHERE content_id='c1'",
    "checkpoint": "SELECT revision,cursor FROM source_checkpoints WHERE workspace='w' AND source='s' AND partition='p'",
}

def main():
    with tempfile.TemporaryDirectory(prefix="synquay-query-plan-") as directory:
        with sqlite3.connect(Path(directory) / "fixture.db") as c:
            for name in ["schema.sql", "migration_v2.sql", "migration_v3.sql", "migration_v4.sql"]:
                c.executescript((ROOT / "backend/src/infrastructure/storage" / name).read_text())
            c.execute("INSERT INTO dispatches(workspace,dispatch_key,receipt) VALUES('w','test','[]')")
            for i in range(10000):
                state = "pending" if i % 7 == 0 else "submitted"
                c.execute("INSERT INTO deliveries(workspace,delivery_id,dispatch_key,record,state,due_ms,target_id) VALUES('w',?,'test','{}',?,?,?)", (str(i), state, i, f"t{i % 32}"))
                c.execute("INSERT INTO audit VALUES(?,'w',?,'{}')", (str(i), i))
                c.execute("INSERT INTO events(workspace,source,event_id,received_ms,record) VALUES('w','s',?,?,'{}')", (str(i), i))
                c.execute("INSERT INTO routing_tasks(workspace,source,event_id,state,due_ms) VALUES('w','s',?,?,?)", (str(i), "ready" if i % 7 == 0 else "completed", i))
            c.execute("ANALYZE")
            plans = {name: {"sql": query, "plan": [r[3] for r in c.execute("EXPLAIN QUERY PLAN " + query)]} for name, query in QUERIES.items()}
            print(json.dumps({"sqlite_version": sqlite3.sqlite_version, "engine": "Python sqlite3; bundled Rust engine separately covered by integration test", "fixture_deliveries": 10000, "fixture_events": 10000, "fixture_audit": 10000, "plans": plans}, ensure_ascii=False, indent=2))

if __name__ == "__main__":
    main()
