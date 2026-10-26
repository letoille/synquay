#!/usr/bin/env python3
"""Real Keycloak authorization-code/PKCE -> Synquay MCP integration.

Requires a verified Keycloak 26.8.1 distribution, Java 21+, and compiled Rust examples.
Uses generated development identities, random loopback ports and temporary databases.
"""
import argparse
import base64
import hashlib
from html.parser import HTMLParser
import http.cookiejar
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import secrets
import socket
import subprocess
import tempfile
import threading
import time
from urllib.error import HTTPError, URLError
from urllib.parse import parse_qs, quote_plus, urlencode, urljoin, urlsplit
from urllib.request import Request, build_opener, HTTPRedirectHandler, HTTPCookieProcessor, ProxyHandler

from keycloak_realm import prepare, REALM, AGENT_CLIENT, SCOPE, VERSION
ROOT = Path(__file__).resolve().parents[1]


class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


class Forms(HTMLParser):
    def __init__(self):
        super().__init__()
        self.forms = []
        self.current = None

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag == "form":
            self.current = {"id": attrs.get("id"), "action": attrs.get("action"), "fields": {}}
            self.forms.append(self.current)
        elif tag == "input" and self.current is not None and attrs.get("name"):
            if attrs.get("type") == "hidden":
                self.current["fields"][attrs["name"]] = attrs.get("value", "")

    def handle_endtag(self, tag):
        if tag == "form":
            self.current = None


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--keycloak-home", type=Path, required=True)
    ap.add_argument("--java-home", type=Path, required=True)
    args = ap.parse_args()
    kc = args.keycloak_home.resolve() / "bin/kc.sh"
    assert kc.is_file() and (args.java_home / "bin/java").is_file()
    authbase = f"http://127.0.0.1:{free_port()}"
    mcpbase = f"http://127.0.0.1:{free_port()}"
    callback = f"http://127.0.0.1:{free_port()}/callback"
    issuer = authbase + "/realms/" + REALM
    resource = mcpbase + "/mcp"
    # Keycloak treats loopback as a secure context, like browsers. Python's cookie
    # jar does not; this test-only policy permits its Secure cookies over HTTP.
    # request() confines every request (including form actions) to loopback ports.
    cookies = http.cookiejar.CookieJar(http.cookiejar.DefaultCookiePolicy(secure_protocols=("https", "wss", "http")))
    opener = build_opener(ProxyHandler({}), NoRedirect(), HTTPCookieProcessor(cookies))
    processes, streams, issued = [], [], []
    calls = []

    def request(url, form=None, basic=None, bearer=None, method=None):
        # Test-only harness never sends generated credentials outside these loopback services.
        assert urlsplit(url).netloc in {urlsplit(authbase).netloc, urlsplit(mcpbase).netloc}
        headers = {}
        if basic:
            headers["Authorization"] = "Basic " + base64.b64encode((':'.join(quote_plus(part) for part in basic)).encode()).decode()
        if bearer:
            headers["Authorization"] = "Bearer " + bearer
        if form is not None:
            headers["Content-Type"] = "application/x-www-form-urlencoded"
        req = Request(url, data=None if form is None else urlencode(form).encode(), headers=headers, method=method)
        try:
            response = opener.open(req, timeout=5)
        except HTTPError as error:
            response = error
        with response:
            return response.status, response.headers, response.read(1024 * 1024)

    def data(url, **kwargs):
        status, _, body = request(url, **kwargs)
        try:
            value = json.loads(body)
        except ValueError:
            value = {}
        return status, value

    def wait_ready(url, process):
        deadline = time.monotonic() + 90
        while time.monotonic() < deadline:
            assert process.poll() is None, "service stopped before readiness"
            try:
                status, _, _ = request(url)
                if status == 200:
                    return
            except (URLError, TimeoutError):
                pass
            time.sleep(0.25)
        raise AssertionError("service readiness timed out")

    def authorize(verifier, requested_resource=resource, challenge=True):
        state = secrets.token_urlsafe(24)
        query = {"client_id": AGENT_CLIENT, "response_type": "code", "scope": "openid " + SCOPE,
                 "redirect_uri": callback, "state": state, "resource": requested_resource}
        if challenge:
            query.update(code_challenge=base64.urlsafe_b64encode(hashlib.sha256(verifier.encode()).digest()).decode().rstrip("="), code_challenge_method="S256")
        url = metadata["authorization_endpoint"] + "?" + urlencode(query)
        for _ in range(10):
            status, headers, body = request(url)
            if status in (302, 303):
                location = urljoin(url, headers["Location"])
            else:
                parser = Forms(); parser.feed(body.decode())
                if not parser.forms:
                    return {"error": ["authorization_rejected"]}
                login = next((f for f in parser.forms if f["id"] == "kc-form-login"), None)
                form = login or parser.forms[0]
                values = form["fields"]
                if login:
                    values.update(username=credentials["username"], password=credentials["password"])
                else:
                    values["accept"] = "Yes"
                action = urljoin(url, form["action"])
                status, headers, body = request(action, form=values)
                assert status in (302, 303), f"login/consent failed: status={status}"
                location = urljoin(action, headers["Location"])
            if location.startswith(callback + "?"):
                result = parse_qs(urlsplit(location).query)
                assert result.get("state") == [state], "OAuth state mismatch"
                if "code" in result:
                    assert result.get("iss") == [issuer], "OAuth issuer mismatch"
                return result
            assert urlsplit(location).netloc == urlsplit(authbase).netloc
            url = location
        raise AssertionError("authorization redirect limit")

    class Echo(BaseHTTPRequestHandler):
        def do_POST(self):
            assert self.headers.get("Authorization") is None
            body = self.rfile.read(int(self.headers["Content-Length"]))
            calls.append(json.loads(body))
            self.send_response(200); self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body))); self.end_headers(); self.wfile.write(body)

        def log_message(self, *_args):
            pass

    with tempfile.TemporaryDirectory(prefix="synquay-keycloak-") as temporary:
        tmp = Path(temporary)
        with ThreadingHTTPServer(("127.0.0.1", 0), Echo) as echo:
            threading.Thread(target=echo.serve_forever, daemon=True).start()
            lab = tmp / "lab"
            credentials = prepare(lab, authbase, resource, callback, f"http://127.0.0.1:{echo.server_port}/echo", ROOT)
            env = dict(os.environ, JAVA_HOME=str(args.java_home.resolve()))
            env.pop("SYNQUAY_PIPELINE_CONFIG", None)
            db_url = "jdbc:h2:file:" + str(tmp / "keycloak-db") + ";NON_KEYWORDS=VALUE;DB_CLOSE_ON_EXIT=FALSE"
            log_path = tmp / "keycloak.log"
            log = open(log_path, "w+"); streams.append(log)
            try:
                subprocess.run([str(kc), "import", "--file", str(lab / "realm.json"), "--db", "dev-file", "--db-url", db_url, "--features", "resource-indicators"],
                               env=env, stdout=log, stderr=log, check=True, timeout=90)
                authority = subprocess.Popen([str(kc), "start-dev", "--http-host", "127.0.0.1",
                    "--http-port", str(urlsplit(authbase).port), "--http-management-port", str(free_port()),
                    "--hostname", authbase, "--features", "resource-indicators", "--db-url", db_url],
                    env=env, stdout=log, stderr=log)
                processes.append(authority)
                wait_ready(issuer + "/.well-known/openid-configuration", authority)
                status, metadata = data(issuer + "/.well-known/openid-configuration")
                assert status == 200 and metadata["issuer"] == issuer
                assert "S256" in metadata["code_challenge_methods_supported"]
                assert metadata["authorization_response_iss_parameter_supported"] is True
                print("PASS: real Keycloak discovery and issuer identification", flush=True)
                verifier = secrets.token_urlsafe(48)
                assert "code" not in authorize(verifier, challenge=False)
                rejected = authorize(verifier, requested_resource="https://wrong.example/mcp")
                basic = (AGENT_CLIENT, credentials["agent_secret"])
                if "code" in rejected:
                    status, denial = data(metadata["token_endpoint"], basic=basic, form={
                        "grant_type": "authorization_code", "code": rejected["code"][0],
                        "redirect_uri": callback, "resource": "https://wrong.example/mcp", "code_verifier": verifier})
                    assert status == 400 and denial.get("error") == "invalid_target", "unknown resource was not rejected at token exchange"
                grant = authorize(verifier)
                assert "code" in grant, "authorization did not return a code"
                basic = (AGENT_CLIENT, credentials["agent_secret"])
                exchange = {"grant_type": "authorization_code", "code": grant["code"][0], "redirect_uri": callback,
                            "resource": resource, "code_verifier": verifier}
                status, tokens = data(metadata["token_endpoint"], form=exchange, basic=basic)
                assert status == 200, f"token exchange failed: {tokens.get('error')}"
                issued.extend([tokens["access_token"], tokens["refresh_token"]])
                assert data(metadata["token_endpoint"], form=exchange, basic=basic)[0] == 400
                # A separate authorization proves verifier checking independently of code replay.
                bad = authorize(verifier)
                wrong = dict(exchange, code=bad["code"][0], code_verifier=secrets.token_urlsafe(48))
                assert data(metadata["token_endpoint"], form=wrong, basic=basic)[0] == 400
                print("PASS: login, consent, PKCE, code replay rejection, invalid resource rejection", flush=True)
                inspect_basic = (resource, credentials["resource_secret"])
                status, claims = data(metadata["introspection_endpoint"], form={"token": tokens["access_token"]}, basic=inspect_basic)
                assert status == 200 and claims.get("active") is True, "resource client introspection rejected"
                assert claims["iss"] == issuer and claims["client_id"] == AGENT_CLIENT
                audience = claims["aud"] if isinstance(claims["aud"], list) else [claims["aud"]]
                assert resource in audience and SCOPE in claims["scope"].split()
                security = json.loads((lab / "security.json").read_text())
                assert claims["sub"] == security["oauth"]["grants"][0]["subject"]
                service_log = open(tmp / "synquay.log", "w+"); streams.append(service_log)
                env.update(SYNQUAY_BIND=urlsplit(mcpbase).netloc, SYNQUAY_TOOLS_CONFIG=str(lab / "tools.json"),
                           SYNQUAY_TOOLS_SECURITY_CONFIG=str(lab / "security.json"), SYNQUAY_LAB_ADMIN=credentials["admin_secret"],
                           SYNQUAY_LAB_INTROSPECTION=credentials["resource_secret"], RUST_LOG="info")
                server = subprocess.Popen([str(ROOT / "target/debug/synquay-server")], env=env, stdout=service_log, stderr=service_log)
                processes.append(server)
                wait_ready(mcpbase + "/.well-known/oauth-protected-resource/mcp", server)

                def client(token, *argv):
                    client_env = dict(env, SYNQUAY_MCP_URL=resource, SYNQUAY_MCP_TOKEN=token)
                    result = subprocess.run([str(ROOT / "target/debug/examples/mcp_client"), *argv], env=client_env,
                                            capture_output=True, text=True, timeout=15)
                    assert result.returncode == 0, "SDK request failed"
                    return json.loads(result.stdout)

                assert client(tokens["access_token"])[0]["name"] == "echo__r1"
                assert client(tokens["access_token"], "echo__r1", '{"value":7}', "00000000-0000-0000-0000-000000000007")["structuredContent"] == {"value": 7}
                status, refreshed = data(metadata["token_endpoint"], form={"grant_type": "refresh_token",
                    "refresh_token": tokens["refresh_token"], "resource": resource}, basic=basic)
                assert status == 200
                issued.extend([refreshed["access_token"], refreshed["refresh_token"]])
                assert client(refreshed["access_token"])[0]["name"] == "echo__r1"
                print("PASS: issuer/audience/subject/client/scope mapping, MCP SDK call, refresh", flush=True)
                status, _, _ = request(metadata["revocation_endpoint"], form={"token": refreshed["refresh_token"], "token_type_hint": "refresh_token"}, basic=basic)
                assert status == 200
                assert data(metadata["token_endpoint"], form={"grant_type": "refresh_token", "refresh_token": refreshed["refresh_token"], "resource": resource}, basic=basic)[0] == 400
                assert request(resource, bearer=refreshed["access_token"])[0] == 401
                assert len(calls) == 1
                for stream in streams:
                    stream.flush(); stream.seek(0)
                    contents = stream.read()
                    assert not any(secret in contents for secret in [*credentials.values(), *issued] if secret != credentials["username"])
                print(f"PASS: revocation rejected at AS and MCP, no forwarded tokens; Keycloak {VERSION}", flush=True)
            except Exception:
                # Keep diagnostics useful without leaking credentials, codes, tokens or URLs containing them.
                log.flush(); log.seek(0)
                errors = [line for line in log if 'ERROR' in line or 'Unrecognized' in line]
                for line in errors[-5:]:
                    for value in [*credentials.values(), *issued]:
                        line = line.replace(value, "[redacted]")
                    print(line.strip())
                raise
            finally:
                for process in reversed(processes):
                    process.terminate()
                    try:
                        process.wait(timeout=15)
                    except subprocess.TimeoutExpired:
                        process.kill(); process.wait(timeout=5)
                for stream in streams:
                    stream.close()
                echo.shutdown()


if __name__ == "__main__":
    main()
