#!/usr/bin/env python3
"""Generate an isolated development realm; never import into a production realm."""
import json
import os
from pathlib import Path
import secrets
import uuid

VERSION = "26.8.1"
REALM = "synquay-lab"
AGENT_CLIENT = "synquay-agent"
SCOPE = "synquay:tools"


def realm_config(resource, redirect, subject, password, agent_secret, resource_secret):
    return {
        "realm": REALM, "enabled": True, "sslRequired": "external",
        "registrationAllowed": False, "resetPasswordAllowed": False,
        "revokeRefreshToken": True, "refreshTokenMaxReuse": 0,
        "accessTokenLifespan": 120, "ssoSessionIdleTimeout": 600,
        "clientScopes": [{
            "name": SCOPE, "protocol": "openid-connect",
            "attributes": {"include.in.token.scope": "true", "display.on.consent.screen": "true",
                           "consent.screen.text": "Allow Synquay tool access"},
            "protocolMappers": [{
                "name": "synquay-audience", "protocol": "openid-connect",
                "protocolMapper": "oidc-audience-mapper",
                "config": {"included.client.audience": resource, "access.token.claim": "true",
                           "id.token.claim": "false", "introspection.token.claim": "true"}},
                {"name": "subject", "protocol": "openid-connect", "protocolMapper": "oidc-sub-mapper",
                 "config": {"access.token.claim": "true", "introspection.token.claim": "true"}}]}],
        "clients": [{
            "clientId": resource, "enabled": True, "protocol": "openid-connect",
            "publicClient": False, "secret": resource_secret, "clientAuthenticatorType": "client-secret",
            "standardFlowEnabled": False, "directAccessGrantsEnabled": False,
            "serviceAccountsEnabled": False, "attributes": {"resource_url": resource}}, {
            "clientId": AGENT_CLIENT, "enabled": True, "protocol": "openid-connect",
            "publicClient": False, "secret": agent_secret, "clientAuthenticatorType": "client-secret",
            "standardFlowEnabled": True, "implicitFlowEnabled": False,
            "directAccessGrantsEnabled": False, "serviceAccountsEnabled": False,
            "consentRequired": True, "fullScopeAllowed": False,
            "redirectUris": [redirect], "webOrigins": [], "defaultClientScopes": [],
            "optionalClientScopes": [SCOPE],
            "attributes": {"pkce.code.challenge.method": "S256",
                           "exclude.issuer.from.auth.response": "false"}}],
        "users": [{"id": subject, "username": "synquay-test", "enabled": True,
                   "email": "synquay-test@example.invalid", "emailVerified": True,
                   "firstName": "Synquay", "lastName": "Test",
                   "credentials": [{"type": "password", "value": password, "temporary": False}]}],
    }


def write_private(path, value):
    with open(path, "x", encoding="utf-8") as stream:
        os.chmod(path, 0o600)
        json.dump(value, stream, indent=2)
        stream.write("\n")


def prepare(directory, issuer_base, resource, redirect, upstream, project_root):
    directory = Path(directory)
    directory.mkdir(mode=0o700, parents=True, exist_ok=False)
    subject = str(uuid.uuid4())
    password, agent_secret, resource_secret, admin_secret = [secrets.token_urlsafe(36) for _ in range(4)]
    credentials = {"username": "synquay-test", "password": password,
                   "agent_secret": agent_secret, "resource_secret": resource_secret, "admin_secret": admin_secret}
    write_private(directory / "credentials.json", credentials)
    write_private(directory / "realm.json", realm_config(resource, redirect, subject, password, agent_secret, resource_secret))
    tools = json.loads((Path(project_root) / "examples/tools.json").read_text())
    tools["database"] = str(directory / "tools.db")
    tools["tools"][0]["url"] = upstream
    write_private(directory / "tools.json", tools)
    write_private(directory / "security.json", {
        "admin": {"principal_id": str(uuid.uuid4()), "token_env": "SYNQUAY_LAB_ADMIN"},
        "oauth": {"issuer": issuer_base + "/realms/" + REALM, "resource": resource,
                  "introspection_endpoint": issuer_base + "/realms/" + REALM + "/protocol/openid-connect/token/introspect",
                  "introspection_client_id": resource, "introspection_secret_env": "SYNQUAY_LAB_INTROSPECTION",
                  "required_scope": SCOPE, "allow_loopback_http": True,
                  "grants": [{"subject": subject, "oauth_client_id": AGENT_CLIENT,
                              "principal_id": str(uuid.uuid4()), "client_id": str(uuid.uuid4()), "tools": ["echo"]}]}})
    return credentials
