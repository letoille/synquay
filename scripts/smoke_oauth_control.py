#!/usr/bin/env python3
"""Real server + SDK + mock introspection, temporary storage and loopback only."""
import base64
import json
import os
from pathlib import Path
import queue
import secrets
import socket
import subprocess
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, quote_plus
from urllib.request import Request, urlopen
from urllib.error import HTTPError

ROOT = Path(__file__).resolve().parents[1]


def main():
    token, secret = [secrets.token_hex(32) for _ in range(2)]
    calls, logs = [], []
    state = {"active": True}
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        address = f"127.0.0.1:{sock.getsockname()[1]}"
    base = f"http://{address}"

    class Mock(BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            if self.path == "/introspect":
                expected = base64.b64encode(f"client:{quote_plus(secret)}".encode()).decode()
                assert self.headers["Authorization"] == f"Basic {expected}"
                form = parse_qs(body.decode())
                body = json.dumps({"active": state["active"] and form["token"] == [token],
                                   "iss": state["issuer"], "aud": base + "/mcp",
                                   "sub": "alice", "client_id": "agent",
                                   "exp": int(time.time()) + 300, "scope": "synquay:tools"}).encode()
            else:
                assert self.path == "/echo"
                assert self.headers.get("Authorization") is None
                calls.append(json.loads(body))
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *_args):
            pass

    with tempfile.TemporaryDirectory(prefix="synquay-oauth-smoke-") as tmp:
        with ThreadingHTTPServer(("127.0.0.1", 0), Mock) as upstream:
            threading.Thread(target=upstream.serve_forever, daemon=True).start()
            authbase = f"http://127.0.0.1:{upstream.server_port}"
            state["issuer"] = authbase + "/issuer"
            cfg = json.loads((ROOT / "examples/tools.json").read_text())
            cfg["database"] = str(Path(tmp) / "tools.db")
            cfg["tools"][0]["url"] = authbase + "/echo"
            config_path, security_path = Path(tmp) / "tools.json", Path(tmp) / "security.json"
            config_path.write_text(json.dumps(cfg))
            security_path.write_text(json.dumps({
                "admin": {"principal_id": "00000000-0000-0000-0000-000000000010", "token_env": "SMOKE_ADMIN"},
                "oauth": {"issuer": state["issuer"], "resource": base + "/mcp",
                          "introspection_endpoint": authbase + "/introspect", "introspection_client_id": "client",
                          "introspection_secret_env": "SMOKE_SECRET", "required_scope": "synquay:tools",
                          "allow_loopback_http": True, "grants": [{
                              "subject": "alice", "oauth_client_id": "agent", "tools": ["echo"],
                              "principal_id": "00000000-0000-0000-0000-000000000011",
                              "client_id": "00000000-0000-0000-0000-000000000012"}]}}))
            env = dict(os.environ, SYNQUAY_MCP_TOKEN=token, SYNQUAY_MCP_URL=base + "/mcp",
                       SMOKE_SECRET=secret, SYNQUAY_TOOLS_CONFIG=str(config_path),
                       SYNQUAY_TOOLS_SECURITY_CONFIG=str(security_path), SYNQUAY_BIND=address,
                       SYNQUAY_LOG_FORMAT="json", RUST_LOG="info")
            env.pop("SYNQUAY_PIPELINE_CONFIG", None)
            env.pop("SMOKE_ADMIN", None)  # Legacy token_env is ignored; no console secret is supplied.
            process = None

            def start():
                nonlocal process
                ready = queue.Queue()
                process = subprocess.Popen([str(ROOT / "target/debug/synquay-server")], env=env,
                                           stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)

                def drain(pipe):
                    for line in pipe:
                        logs.append(line)
                        event = json.loads(line)
                        if event.get("fields", {}).get("event") == "server.started":
                            ready.put(True)
                threading.Thread(target=drain, args=(process.stderr,), daemon=True).start()
                ready.get(timeout=10)

            def stop():
                nonlocal process
                process.terminate()
                assert process.wait(timeout=10) == 0
                process.stderr.close()
                process = None

            def request(path, method="GET", body=None, credential=None):
                headers = {"Content-Type": "application/json"}
                if credential is not None:
                    headers["Authorization"] = f"Bearer {credential}"
                req = Request(base + path, data=None if body is None else json.dumps(body).encode(),
                              headers=headers, method=method)
                try:
                    with urlopen(req, timeout=10) as response:
                        return response.status, json.load(response)
                except HTTPError as error:
                    return error.code, None

            def client(*args):
                result = subprocess.run([str(ROOT / "target/debug/examples/mcp_client"), *args],
                                        env=env, capture_output=True, text=True, timeout=15, check=True)
                return json.loads(result.stdout)

            try:
                start()
                status, metadata = request("/.well-known/oauth-protected-resource/mcp")
                assert status == 200 and metadata["resource"] == base + "/mcp"
                assert request("/api/tools/config")[0] == 200
                assert client()[0]["name"] == "echo__r1"
                args = ("echo__r1", '{"value":7}', "00000000-0000-0000-0000-000000000007")
                assert client(*args)["structuredContent"] == {"value": 7}
                assert request("/api/tools/draft", "PUT", {"expected_generation": 0, "base_revision": 1, "exposed_tools": []})[0] == 200
                operation = {"operation_id": "00000000-0000-0000-0000-000000000008", "expected_revision": 1, "draft_generation": 1}
                assert request("/api/tools/publish", "POST", operation)[1]["revision"] == 2
                assert request("/api/tools/publish", "POST", operation)[1]["replayed"] is True
                assert client() == []
                assert client(*args)["isError"] is True
                stop()
                start()
                assert client() == []
                assert request("/api/tools/config")[1]["revision"] == 2
                state["active"] = False
                assert request("/mcp", credential=token)[0] == 401
                stop()
                assert len(calls) == 1
                assert not any(value in "".join(logs) for value in (token, secret))
                print("OAuth/control process smoke passed: discovery, SDK call, credential-free console, online hide, idempotent publish, restart, revocation, private logs")
            finally:
                if process is not None:
                    process.kill()
                    process.wait(timeout=5)
                upstream.shutdown()


if __name__ == "__main__":
    main()
