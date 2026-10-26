#!/usr/bin/env python3
"""Create missing installation configuration; never replace operator files or secrets."""
import argparse
import os
from pathlib import Path
import secrets
import tempfile


def create_once(path, content, mode):
    # Publish a fully written file atomically. A concurrent/existing file wins.
    fd, name = tempfile.mkstemp(prefix=".synquay-init-", dir=path.parent)
    try:
        with os.fdopen(fd, "wb") as stream:
            os.fchmod(stream.fileno(), mode)
            stream.write(content)
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.link(name, path)
        except FileExistsError:
            pass
    finally:
        os.unlink(name)


def initialize(config_dir, templates):
    config_dir.mkdir(parents=True, exist_ok=True, mode=0o750)
    for name in ("tools.json", "security.json", "pipeline.json"):
        create_once(config_dir / name, (templates / name).read_bytes(), 0o640)
    credentials = "".join(f"{key}={secrets.token_hex(32)}\n" for key in
                          ("SYNQUAY_MCP_TOKEN", "SYNQUAY_WEBHOOK_TOKEN"))
    create_once(config_dir / "credentials.env", credentials.encode(), 0o600)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--config-dir", type=Path, default=Path("/etc/synquay"))
    parser.add_argument("--templates", type=Path, default=Path("/usr/share/synquay/config"))
    args = parser.parse_args()
    initialize(args.config_dir, args.templates)
