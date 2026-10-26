#!/usr/bin/env python3
"""Fetch pinned upstream development runtimes with SHA-256 verification.

No global Java installation or shell-profile changes. Requires Python 3.12+ for
safe tar extraction. On other OS/architectures, use --skip-java and supply Java 21+.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import tarfile
import tempfile
from urllib.request import urlopen

ROOT = Path(__file__).resolve().parents[1]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--directory", type=Path, required=True)
    ap.add_argument("--skip-java", action="store_true")
    args = ap.parse_args()
    if not args.skip_java and (platform.system(), platform.machine()) != ("Darwin", "arm64"):
        ap.error("bundled Java lock is macOS arm64 only; use --skip-java and an installed Java 21+ runtime")
    lock = json.loads((ROOT / "examples/keycloak/runtime-lock.json").read_text())
    dest = args.directory.resolve()
    dest.mkdir(parents=True, exist_ok=True)
    for name in (["keycloak"] if args.skip_java else ["keycloak", "java_macos_arm64"]):
        entry = lock[name]
        output = dest / entry["directory"]
        if output.exists():
            raise SystemExit(f"Refusing to overwrite existing runtime: {output}")
        with tempfile.TemporaryFile() as archive:
            digest = hashlib.sha256()
            total = 0
            with urlopen(entry["url"], timeout=30) as response:
                while chunk := response.read(1024 * 1024):
                    total += len(chunk)
                    if total > 512 * 1024 * 1024:
                        raise SystemExit("Runtime download exceeded 512 MiB")
                    digest.update(chunk); archive.write(chunk)
            if digest.hexdigest() != entry["sha256"]:
                raise SystemExit("Runtime checksum mismatch; nothing extracted")
            archive.seek(0)
            with tarfile.open(fileobj=archive, mode="r:gz") as tar:
                tar.extractall(dest, filter="data")
        print(f"Verified {name} {entry['version']}: {output}")


if __name__ == "__main__":
    main()
