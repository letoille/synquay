#!/usr/bin/env python3
"""Read-only tool publication/ledger inspection. Never prints tokens, arguments or results."""
import argparse
import json
import sqlite3
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("database", type=Path)
args = parser.parse_args()
c = sqlite3.connect(args.database.resolve().as_uri() + "?mode=ro", uri=True)
c.row_factory = sqlite3.Row
try:
    if c.execute("PRAGMA user_version").fetchone()[0] not in (5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21):
        parser.error("Expected schema v5 through v21")
    c.execute("BEGIN")
    heads = [dict(r) for r in c.execute("SELECT workspace,profile,revision FROM tool_heads")]
    usage = dict(c.execute("SELECT items,bytes FROM tool_invocation_usage WHERE singleton=1").fetchone())
    # Grouped counts are an explicit operator inspection, not a per-request query.
    states = [dict(r) for r in c.execute("SELECT state,COUNT(*) AS count FROM tool_invocations GROUP BY state")]
    unknown = c.execute("SELECT COUNT(*) FROM tool_invocations WHERE state='running' OR outcome='\"Unknown\"'").fetchone()[0]
    print(json.dumps({"profiles": heads, "ledger_usage": usage, "states": states, "unfinished_or_unknown": unknown}, indent=2))
finally:
    c.close()
