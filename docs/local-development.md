# Local development

[English README](../README.md) · [简体中文](../README.zh-CN.md)

Use current Rust stable (edition 2024), Node.js 22.18 or later on the 22.x line, Python 3, and a POSIX shell. Rust 1.98.1 is the locally tested compiler; the minimum supported Rust version has not been established. Node 22.18+ is recommended because the frontend tests use Node's TypeScript transformation support.

Run the following commands from the repository root after `npm ci`. They use an independent, Git-ignored `data/local` directory and do not depend on a developer's existing preview database.

## 1. Initialize configuration

```bash
python3 backend/deploy/initialize.py \
  --config-dir data/local \
  --templates backend/deploy/config

python3 - <<'PY'
import json
from pathlib import Path

folder = Path('data/local').resolve()
for name in ('tools.json', 'pipeline.json'):
    path = folder / name
    config = json.loads(path.read_text())
    if config['database'] == '/var/lib/synquay/synquay.db':
        config['database'] = str(folder / 'synquay.db')
        path.write_text(json.dumps(config, indent=2) + '\n')
path = folder / 'security.json'
config = json.loads(path.read_text())
origins = config['admin'].setdefault('allowed_origins', [])
if 'http://127.0.0.1:5173' not in origins:
    origins.append('http://127.0.0.1:5173')
    path.write_text(json.dumps(config, indent=2) + '\n')
PY
```

The initializer generates random MCP and bootstrap Webhook credentials once. Repeating it preserves existing files. The new workspace starts with no tools, managed message connections, or forwarding routes. Base pipeline endpoints remain available as documented in the [deployment guide](debian.md); they do not send anything without a route.

## 2. Start the backend

In the first terminal:

```bash
set -a
. ./data/local/credentials.env
set +a
export SYNQUAY_BIND=127.0.0.1:8080
export SYNQUAY_TOOLS_CONFIG="$PWD/data/local/tools.json"
export SYNQUAY_TOOLS_SECURITY_CONFIG="$PWD/data/local/security.json"
export SYNQUAY_PIPELINE_CONFIG="$PWD/data/local/pipeline.json"
cargo run --locked -p synquay-server
```

`credentials.env` is generated locally by the initializer. Do not source untrusted environment files. The server does not automatically load `.env`; `.env.example` is only a reference. If you previously set `SYNQUAY_WEB_DIR` for a packaged deployment, unset it in this development terminal.

## 3. Start the frontend

In a second terminal at the repository root:

```bash
npm run dev:frontend
```

Open **http://127.0.0.1:5173/**. Vite proxies API and MCP requests to `127.0.0.1:8080`. The console should show an empty workspace and the default MCP service. Create connections and routes in the UI; no demo upstream service is required.

Health check:

```bash
curl --fail http://127.0.0.1:8080/api/health
```

A successful health response confirms that the process can respond, not that external brokers or chat platforms are reachable. For QQ callbacks use a public HTTPS address; a loopback address is only reachable locally.

If you change the frontend port, update `admin.allowed_origins` in the local security configuration and restart the backend. For a different backend port, set `SYNQUAY_BIND` in the backend terminal and `SYNQUAY_API_TARGET` in the frontend terminal.

## Logs and checks

`RUST_LOG=debug` changes verbosity; `SYNQUAY_LOG_FORMAT=json` enables structured logs. Logs go to stderr. API request IDs correlate responses and request logs. OpenTelemetry export and cross-service tracing are not yet implemented.

```bash
npm run check
npm run test:frontend
python3 -m unittest discover -s scripts/tests -v
```

For a single-process production build:

```bash
npm run build:frontend
cargo build --release --locked -p synquay-server
# Keep the configuration and credential variables from step 2.
SYNQUAY_WEB_DIR="$PWD/frontend/dist" ./target/release/synquay-server
```

Stop the development backend before starting this process on the same port. The console is then served at **http://127.0.0.1:8080/** without Vite.
