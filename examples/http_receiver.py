"""Local demonstration receiver. Stores nothing; displays only delivery identifiers."""
import json
from http.server import BaseHTTPRequestHandler, HTTPServer


class Receiver(BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/messages":
            self.send_error(404)
            return
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 256 * 1024:
                raise ValueError()
            payload = json.loads(self.rfile.read(length))
            delivery_id = payload["delivery_id"]
        except (ValueError, KeyError, TypeError):
            self.send_error(400)
            return
        self.send_response(202)
        self.send_header("Content-Length", "0")
        self.end_headers()
        print(f"Accepted delivery {delivery_id}", flush=True)

    def log_message(self, *_):
        pass


if __name__ == "__main__":
    print("Local receiver: http://127.0.0.1:9091/messages", flush=True)
    with HTTPServer(("127.0.0.1", 9091), Receiver) as server:
        try:
            server.serve_forever()
        except KeyboardInterrupt:
            pass
