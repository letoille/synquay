#!/usr/bin/env python3
"""Local echo endpoint for examples/tools.json. Binds loopback only."""
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json

class Handler(BaseHTTPRequestHandler):
    def do_POST(self):
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if self.path != "/echo" or not 0 < length <= 65536:
                self.send_error(400)
                return
            self.connection.settimeout(5)
            value = json.loads(self.rfile.read(length))
            if type(value.get("value")) is not int:
                self.send_error(400)
                return
            body = json.dumps({"value": value["value"]}).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except (ValueError, AttributeError, TimeoutError):
            self.send_error(400)

    def log_message(self, *_args):
        pass

if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", 8091), Handler).serve_forever()
