"""Exercise the compiled server, real HTTP forwarding, SIGTERM and restart dedupe.
Run cargo build -p synquay-server first. Uses only loopback and a temporary database.
"""
import json
import os
from pathlib import Path
import queue
import secrets
import subprocess
import tempfile
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.request import Request, urlopen

ROOT = Path(__file__).resolve().parents[1]


def main():
    delivered = queue.Queue()

    class Receiver(BaseHTTPRequestHandler):
        def do_POST(self):
            body = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
            assert self.headers["Idempotency-Key"] == body["delivery_id"]
            delivered.put(body)
            self.send_response(202)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def log_message(self, *_):
            pass

    with tempfile.TemporaryDirectory(prefix="synquay-smoke-") as temporary:
        with ThreadingHTTPServer(("127.0.0.1", 0), Receiver) as receiver:
            threading.Thread(target=receiver.serve_forever, daemon=True).start()
            config = json.loads((ROOT / "examples/pipeline.example.json").read_text())
            config["database"] = str(Path(temporary) / "events.db")
            config["destination_url"] = f"http://127.0.0.1:{receiver.server_port}/messages"
            path = Path(temporary) / "pipeline.json"
            path.write_text(json.dumps(config))
            token = secrets.token_hex(32)
            env = dict(os.environ, SYNQUAY_PIPELINE_CONFIG=str(path),
                       SYNQUAY_BIND="127.0.0.1:0", SYNQUAY_LOG_FORMAT="json",
                       RUST_LOG="info", SYNQUAY_WEBHOOK_TOKEN=token)
            process = None
            errors = []

            def start():
                nonlocal process
                started = queue.Queue()
                process = subprocess.Popen([str(ROOT / "target/debug/synquay-server")],
                                           env=env, stdout=subprocess.DEVNULL,
                                           stderr=subprocess.PIPE, text=True)

                def logs(pipe):
                    for line in pipe:
                        try:
                            event = json.loads(line)
                        except ValueError:
                            errors.append("non-JSON server output")
                            continue
                        if event.get("level") == "ERROR":
                            errors.append(event.get("fields", {}).get("event", "server_error"))
                        if event.get("fields", {}).get("event") == "server.started":
                            started.put(event["fields"]["address"])

                threading.Thread(target=logs, args=(process.stderr,), daemon=True).start()
                return f"http://{started.get(timeout=10)}/api/webhooks/{config['source_connection_id']}"

            def stop():
                nonlocal process
                process.terminate()
                assert process.wait(timeout=10) == 0, "graceful shutdown failed"
                process.stderr.close()
                process = None

            def send(url):
                body = {"source_event_id": "smoke-event-1", "event_type": "chat.message",
                        "payload": {"text": "local smoke test"}}
                request = Request(url, data=json.dumps(body).encode(),
                                  headers={"Authorization": f"Bearer {token}",
                                           "Content-Type": "application/json"})
                with urlopen(request, timeout=5) as response:
                    assert response.status == 202
                    return json.load(response)

            try:
                first = send(start())
                message = delivered.get(timeout=5)
                assert message["parts"][0]["value"]["payload"]["text"] == "local smoke test"
                stop()
                repeated = send(start())
                assert repeated["event_id"] == first["event_id"]
                assert repeated["disposition"] == "duplicate"
                try:
                    delivered.get(timeout=1.5)
                    raise AssertionError("duplicate was sent after restart")
                except queue.Empty:
                    pass
                stop()
                assert not errors, errors
                print("PASS: real process forwarding, graceful shutdown, restart and deduplication")
            finally:
                if process is not None:
                    process.kill()
                    process.wait(timeout=5)
                receiver.shutdown()


if __name__ == "__main__":
    main()
