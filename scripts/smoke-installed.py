#!/usr/bin/env python3
"""Check the installed application without creating connections or sending messages."""
import json
import time
import urllib.request

opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
base = "http://127.0.0.1:8080"
for attempt in range(80):
    try:
        with opener.open(base + "/api/health", timeout=2) as response:
            assert json.load(response)["status"] == "ok"
        break
    except (OSError, AssertionError):
        if attempt == 79:
            raise
        time.sleep(.25)
with opener.open(base + "/") as response:
    assert "text/html" in response.headers["Content-Type"]
    assert b'id="root"' in response.read()
for path in ("/api/tools/config", "/api/message-connections", "/api/routes"):
    with opener.open(base + path) as response:
        data = json.load(response)
        if path == "/api/message-connections":
            assert data["available"] and data["snapshot"]["connections"] == []
        elif path == "/api/routes":
            assert data["available"] and data["snapshot"]["routes"] == []
print("Installed console, MCP management, connections and empty routing verified")
