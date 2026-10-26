#!/usr/bin/env python3
"""Test compiled server + official SDK example client with temp DB and loopback HTTP."""
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = Path(__file__).resolve().parents[1]

def main():
    calls = []
    logs = []
    class Receiver(BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            calls.append(json.loads(body))
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        def log_message(self, *_args):
            pass
    with tempfile.TemporaryDirectory(prefix="synquay-mcp-smoke-") as tmp:
        with ThreadingHTTPServer(("127.0.0.1", 0), Receiver) as upstream:
            threading.Thread(target=upstream.serve_forever, daemon=True).start()
            cfg = json.loads((ROOT / "examples/tools.json").read_text())
            cfg["database"] = str(Path(tmp) / "tools.db")
            cfg["tools"][0]["url"] = f"http://127.0.0.1:{upstream.server_port}/echo"
            config_path = Path(tmp) / "tools.json"
            token = secrets.token_hex(32)
            env = dict(os.environ, SYNQUAY_MCP_TOKEN=token, SYNQUAY_TOOLS_CONFIG=str(config_path),
                       SYNQUAY_BIND="127.0.0.1:0", SYNQUAY_LOG_FORMAT="json", RUST_LOG="info")
            env.pop("SYNQUAY_PIPELINE_CONFIG", None)
            process = None
            def start():
                nonlocal process
                config_path.write_text(json.dumps(cfg))
                ready = queue.Queue()
                process = subprocess.Popen([str(ROOT / "target/debug/synquay-server")], env=env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
                def drain(pipe):
                    for line in pipe:
                        logs.append(line)
                        event = json.loads(line)
                        if event.get("fields", {}).get("event") == "server.started":
                            ready.put(event["fields"]["address"])
                threading.Thread(target=drain, args=(process.stderr,), daemon=True).start()
                env["SYNQUAY_MCP_URL"] = f"http://{ready.get(timeout=10)}/mcp"
            def stop():
                nonlocal process
                process.terminate()
                assert process.wait(timeout=10) == 0
                process.stderr.close()
                process = None
            def client(*args):
                result = subprocess.run([str(ROOT / "target/debug/examples/mcp_client"), *args],
                                        env=env, capture_output=True, text=True, timeout=15, check=True)
                return json.loads(result.stdout)
            try:
                start()
                assert client()[0]["name"] == "echo__r1"
                args = ("echo__r1", '{"value":7}', "00000000-0000-0000-0000-000000000007")
                assert client(*args)["structuredContent"] == {"value":7}
                assert client(*args)["isError"] is False
                stop()
                start()
                assert client(*args)["isError"] is False
                assert len(calls) == 1
                stop()
                cfg["revision"] = 2
                cfg["tools"][0]["exposed"] = False
                start()
                assert client() == []
                assert client(*args)["isError"] is True
                stop()
                assert token not in "".join(logs)
                status = subprocess.run(["python3", str(ROOT / "scripts/inspect_tools.py"), cfg["database"]], capture_output=True, text=True, check=True)
                status = json.loads(status.stdout)
                assert status["profiles"][0]["revision"] == 2
                assert status["ledger_usage"]["items"] == 1
                assert status["unfinished_or_unknown"] == 0
                print("MCP process smoke passed: SDK discovery/call, restart dedupe, publication, revocation, private logs, inspection")
            finally:
                if process is not None:
                    process.kill()
                    process.wait(timeout=5)
                upstream.shutdown()

if __name__ == "__main__":
    main()
