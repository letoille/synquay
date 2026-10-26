"""Inspect runtime/cases or submit a reviewed recovery JSON command using an operator token."""
import argparse
import ipaddress
import json
import os
from pathlib import Path
from urllib.error import HTTPError
from urllib.parse import urlsplit
from urllib.request import Request, build_opener, ProxyHandler, HTTPRedirectHandler
import uuid

class NoRedirect(HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--url', default='http://127.0.0.1:8080')
    p.add_argument('--token-env', default='SYNQUAY_ADMIN_TOKEN')
    mode = p.add_mutually_exclusive_group()
    mode.add_argument('--command', type=Path, help='POST this reviewed recovery command; reuse it after an unknown response')
    mode.add_argument('--case', nargs=2, metavar=('KIND','UUID'), help='inspect delivery or route history')
    args = p.parse_args()
    u = urlsplit(args.url)
    try:
        local = ipaddress.ip_address(u.hostname or '').is_loopback
    except ValueError:
        local = False
    if u.username or u.password or u.query or u.fragment or not u.hostname or not (u.scheme == 'https' or (u.scheme == 'http' and local)):
        p.error('use HTTPS or numeric loopback HTTP; credentials/query/fragment in URLs are disallowed')
    token = os.environ.get(args.token_env)
    if not token:
        p.error('operator token environment variable is missing')
    data = None
    path = '/api/ops'
    if args.command:
        with args.command.open('rb') as f:
            data = f.read(8193)
        if len(data)>8192:
            p.error('command exceeds 8 KiB')
        json.loads(data)
        path += '/recover'
    elif args.case:
        kind, identifier = args.case
        if kind not in ('delivery', 'route'):
            p.error('case kind must be delivery or route')
        path += '/recovery/'+kind+'/'+str(uuid.UUID(identifier))
    request = Request(args.url.rstrip('/')+path, data=data,
                      headers={'Authorization':'Bearer '+token,'Content-Type':'application/json'})
    opener = build_opener(ProxyHandler({}), NoRedirect())
    try:
        with opener.open(request, timeout=15) as response:
            print(json.dumps(json.load(response),ensure_ascii=False,indent=2))
    except HTTPError as e:
        print(json.dumps({'status':e.code,'response':e.read(8192).decode('utf-8',errors='replace')},ensure_ascii=False))
        raise SystemExit(1)

if __name__ == '__main__':
    main()
