#!/usr/bin/env python3
"""Restart the local console at 5174/8082, preserving its existing database."""
import argparse
import json
import os
from pathlib import Path
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request

from seed_console_routes import seed_if_configured

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'data' / 'console-onboarding'
PORTS = (5174, 8082, 5274, 8182, 5275, 8183)


def listeners(port):
    result = subprocess.run(['lsof', '-nP', f'-iTCP:{port}', '-sTCP:LISTEN', '-Fp'], capture_output=True, text=True)
    if result.returncode not in (0, 1):
        raise RuntimeError(f'Unable to inspect port {port}')
    return {int(line[1:]) for line in result.stdout.splitlines() if line.startswith('p')}


def verify_owner(pid):
    result = subprocess.run(['lsof', '-a', '-p', str(pid), '-d', 'cwd', '-Fn'], capture_output=True, text=True)
    directories = {Path(line[1:]).resolve() for line in result.stdout.splitlines() if line.startswith('n')}
    if not directories.intersection({ROOT, ROOT / 'frontend'}):
        raise RuntimeError(f'Process {pid} is not verified as this project; refusing to stop it')


def read_json(url, timeout=2):
    with urllib.request.urlopen(url, timeout=timeout) as response:
        if response.status != 200:
            raise RuntimeError('Unexpected HTTP status')
        return json.load(response)


def wait_for(url, process, log):
    deadline = time.monotonic() + 20
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f'Service exited; inspect {log}')
        try:
            return read_json(url)
        except (OSError, ValueError):
            time.sleep(.25)
    raise RuntimeError(f'Service did not become ready; inspect {log}')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true', help='Validate configuration and show processes without changing anything')
    args = parser.parse_args()
    config_path = DATA / 'tools.json'
    security_path = DATA / 'security.json'
    config = json.loads(config_path.read_text())
    json.loads(security_path.read_text())
    database = Path(config['database'])
    if not database.is_absolute():
        database = ROOT / database
    if not database.is_file():
        raise RuntimeError('Existing console database is missing; refusing to create a replacement')
    token = (DATA / 'mcp-token').read_text().strip()
    if len(token) < 32:
        raise RuntimeError('Existing MCP token is missing or too short')
    node = shutil.which('node')
    cargo = shutil.which('cargo')
    if not node or not cargo:
        raise RuntimeError('node and cargo must be available on PATH')
    vite = ROOT / 'node_modules' / 'vite' / 'bin' / 'vite.js'
    if not vite.is_file():
        raise RuntimeError('Vite is missing; install workspace dependencies first')
    pipeline_path = Path(os.environ['SYNQUAY_PIPELINE_CONFIG']) if os.environ.get('SYNQUAY_PIPELINE_CONFIG') else DATA / 'pipeline.json'
    pipeline_config = json.loads(pipeline_path.read_text()) if pipeline_path.is_file() else None
    pipeline_token = None
    if pipeline_config:
        if pipeline_config['workspace_id'] != config['workspace_id']:
            raise RuntimeError('Pipeline and console must use the same workspace')
        pipeline_token = os.environ.get(pipeline_config['webhook_token_env'])
        if pipeline_token is None and pipeline_path.resolve() == (DATA / 'pipeline.json').resolve():
            pipeline_token = (DATA / 'webhook-token').read_text().strip()
        if not pipeline_token or len(pipeline_token) < 32:
            raise RuntimeError('Pipeline webhook secret is missing or too short')
    elif os.environ.get('SYNQUAY_PIPELINE_CONFIG'):
        raise RuntimeError('Configured pipeline file does not exist')
    processes = {port: listeners(port) for port in PORTS}
    for pids in processes.values():
        for pid in pids:
            verify_owner(pid)
    print('Frontend: http://127.0.0.1:5174/ -> backend: http://127.0.0.1:8082', flush=True)
    print('Existing workspace database will be preserved. Temporary QA ports 5274/8182 and 5275/8183 will be stopped.', flush=True)
    for port, pids in processes.items():
        print(f'Port {port}: ' + (', '.join(map(str, sorted(pids))) or 'not running'), flush=True)
    print('Message pipeline: ' + (str(pipeline_path) if pipeline_config else 'not configured'), flush=True)
    if args.check:
        return
    build_env = os.environ.copy()
    if 'CARGO_HOME' not in build_env and Path('/private/tmp/synquay-perf-cargo').is_dir():
        build_env['CARGO_HOME'] = '/private/tmp/synquay-perf-cargo'
    # Build before stopping the working services, so compilation failure leaves them running.
    subprocess.run([cargo, 'build', '--locked', '-p', 'synquay-server', '--bin', 'synquay-server'], cwd=ROOT, env=build_env, check=True)
    for pid in set().union(*processes.values()):
        verify_owner(pid)
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        except PermissionError:
            raise RuntimeError('The environment denied stopping processes. Run this script directly in your local terminal.') from None
    deadline = time.monotonic() + 20
    while any(listeners(port) for port in PORTS):
        if time.monotonic() > deadline:
            raise RuntimeError('A service is still listening; stopped without force-killing any process')
        time.sleep(.2)
    env = os.environ.copy()
    if pipeline_config:
        env['SYNQUAY_PIPELINE_CONFIG'] = str(pipeline_path.resolve())
        env[pipeline_config['webhook_token_env']] = pipeline_token
    else:
        env.pop('SYNQUAY_PIPELINE_CONFIG', None)
    env.update(SYNQUAY_BIND='127.0.0.1:8082', SYNQUAY_TOOLS_CONFIG=str(config_path),
               SYNQUAY_TOOLS_SECURITY_CONFIG=str(security_path), SYNQUAY_API_TARGET='http://127.0.0.1:8082')
    env[config['token_env']] = token
    started = []
    try:
        for name, command, cwd, probe in (
            ('backend', [str(ROOT / 'target/debug/synquay-server')], ROOT, 'http://127.0.0.1:8082/api/tools/config'),
            ('frontend', [node, str(vite), '--host', '127.0.0.1', '--port', '5174', '--strictPort'], ROOT / 'frontend', 'http://127.0.0.1:5174/api/tools/config'),
        ):
            log_path = DATA / f'{name}.log'
            with log_path.open('ab') as log:
                process = subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL,
                                           stdout=log, stderr=log, start_new_session=True)
            started.append(process)
            (DATA / f'{name}.pid').write_text(str(process.pid))
            view = wait_for(probe, process, log_path)
            print(f'{name}: ready, configuration revision {view.get("revision")}', flush=True)
            if name == 'backend' and pipeline_config and pipeline_path.resolve() == (DATA / 'pipeline.json').resolve():
                seed_if_configured(DATA, 'http://127.0.0.1:8082')
        direct = read_json('http://127.0.0.1:8082/api/tools/config')
        proxied = read_json('http://127.0.0.1:5174/api/tools/config')
        if (direct.get('profile_id'), direct.get('revision')) != (proxied.get('profile_id'), proxied.get('revision')):
            raise RuntimeError('Frontend proxy returned a different configuration')
        if pipeline_config:
            routes = read_json('http://127.0.0.1:5174/api/routes')
            if not routes.get('available') and not pipeline_config.get('agent'):
                raise RuntimeError('Frontend cannot access configured message routes')
        with urllib.request.urlopen('http://127.0.0.1:5174/', timeout=3) as response:
            if response.status != 200 or 'text/html' not in response.headers.get('Content-Type', ''):
                raise RuntimeError('Frontend did not serve its HTML page')
    except Exception:
        for process in started:
            if process.poll() is None:
                process.terminate()
        raise
    print('Restart complete. Frontend and backend are connected. Open http://127.0.0.1:5174/', flush=True)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError) as error:
        print(f'Restart failed: {error}', file=sys.stderr)
        sys.exit(1)
