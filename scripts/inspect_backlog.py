#!/usr/bin/env python3
"""Inspect v2/v3/v4 backlog metadata without reading message bodies or changing SQL state."""
import argparse
import json
from pathlib import Path
import shutil
import sqlite3
import time
import uuid


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("database", type=Path)
    parser.add_argument("--workspace", type=uuid.UUID)
    args = parser.parse_args()
    path = args.database.resolve(strict=True)
    connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True, timeout=5)
    connection.row_factory = sqlite3.Row
    try:
        connection.execute("BEGIN")
        version = connection.execute("PRAGMA user_version").fetchone()[0]
        if version not in (2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18):
            parser.error(f"Expected schema 2, 3, 4, 5, 6, 7, 8 or 9, found {version}; do not edit the database manually.")
        policy = json.loads(connection.execute("SELECT record FROM capacity_policy WHERE singleton=1").fetchone()[0])
        query = "SELECT workspace,items,bytes,paused FROM capacity_state"
        params = ()
        if args.workspace:
            query += " WHERE workspace=?"
            params = (str(args.workspace),)
        summaries = []
        for row in connection.execute(query, params):
            workspace = row["workspace"]
            oldest = connection.execute("SELECT MIN(admitted_ms) FROM pending_work WHERE workspace=?", (workspace,)).fetchone()[0]
            states = {}
            for table in ("routing_tasks", "deliveries"):
                unresolved = " AND resolution IS NULL" if version >= 3 else ""
                states[table] = dict(connection.execute(f"SELECT state,COUNT(*) FROM {table} WHERE workspace=?{unresolved} GROUP BY state", (workspace,)))
            summaries.append({"workspace": workspace, "pending_items": row["items"],
                              "pending_bytes": row["bytes"], "paused": bool(row["paused"]),
                              "oldest_pending_seconds": max(0, int(time.time() - oldest / 1000)) if oldest is not None else None,
                              **states})
        print(json.dumps({"schema_version": version, "capacity": policy,
                          "disk_free_bytes": shutil.disk_usage(path.parent).free,
                          "workspaces": summaries}, ensure_ascii=False, indent=2))
    finally:
        connection.close()


if __name__ == "__main__":
    main()
