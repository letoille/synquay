#!/usr/bin/env python3
"""Offline SQLite + content backup/verified restore. Stop Synquay before either operation."""
import argparse
from contextlib import closing
import hashlib
import json
import os
from pathlib import Path
import shutil
import sqlite3
import uuid


def digest(path):
    value = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(64 * 1024), b""):
            value.update(chunk)
    return value.hexdigest()


def inspect(database):
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as c:
        if c.execute("PRAGMA user_version").fetchone()[0] not in (4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18):
            raise ValueError("Expected schema v4 through v18")
        if c.execute("PRAGMA quick_check").fetchall() != [("ok",)] or c.execute("PRAGMA foreign_key_check").fetchone():
            raise ValueError("Database integrity check failed")
        rows = c.execute("SELECT id,size,digest FROM content_objects WHERE state IN ('staged','available')").fetchall()
        directory = c.execute("SELECT directory FROM content_binding WHERE singleton=1").fetchone()
        return rows, Path(directory[0]) if directory else None


def needs_connection_key(database):
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as c:
        if c.execute("PRAGMA user_version").fetchone()[0] < 18:
            return False
        return bool(c.execute("SELECT EXISTS(SELECT 1 FROM connection_secrets) OR EXISTS(SELECT 1 FROM message_connection_operations WHERE request LIKE 'hmac-v1:%')").fetchone()[0])


def check_key(path, expected=None):
    if path.is_symlink() or not path.is_file() or path.stat().st_size != 32:
        raise ValueError("Connection encryption key missing or invalid")
    if os.name == "posix" and path.stat().st_mode & 0o077:
        raise ValueError("Connection encryption key must be owner-only")
    if expected is not None and digest(path) != expected:
        raise ValueError("Connection encryption key checksum mismatch")


def copy_private(source, target):
    if source.is_symlink():
        raise ValueError("Refusing symlink content")
    with source.open("rb") as src, target.open("xb") as dst:
        os.chmod(target, 0o600)
        shutil.copyfileobj(src, dst, 64 * 1024)
        dst.flush()
        os.fsync(dst.fileno())


def verify(bundle):
    if (bundle / "manifest.json").stat().st_size > 16 * 1024 * 1024:
        raise ValueError("Manifest exceeds limit")
    manifest = json.loads((bundle / "manifest.json").read_text())
    if manifest["format"] != 1 or digest(bundle / "database.sqlite3") != manifest["database_sha256"]:
        raise ValueError("Backup database checksum mismatch")
    rows, _ = inspect(bundle / "database.sqlite3")
    key_digest = manifest.get("connection_key_sha256")
    if needs_connection_key(bundle / "database.sqlite3") and not key_digest:
        raise ValueError("Backup is missing the connection encryption key")
    if key_digest:
        check_key(bundle / "connection-secrets.key", key_digest)
    expected = {str(uuid.UUID(row[0])): (row[1], row[2]) for row in rows}
    if set(manifest["content"]) != set(expected):
        raise ValueError("Backup content manifest differs from database")
    for ident, (size, sha) in expected.items():
        path = bundle / "content" / f"{ident}.blob"
        if path.is_symlink() or path.stat().st_size != size or digest(path) != sha or manifest["content"][ident] != sha:
            raise ValueError("Backup content verification failed")
    return manifest


def backup(database, destination):
    # Existing destinations are never overwritten. Incomplete bundles have no valid manifest.
    destination.mkdir(mode=0o700)
    (destination / "content").mkdir(mode=0o700)
    target = destination / "database.sqlite3"
    target.touch(mode=0o600, exist_ok=False)
    with closing(sqlite3.connect(database.as_uri() + "?mode=ro", uri=True)) as src, closing(sqlite3.connect(target)) as dst:
        src.backup(dst, pages=256, sleep=0.01)
    rows, directory = inspect(target)
    content = {}
    for ident, size, sha in rows:
        ident = str(uuid.UUID(ident))
        if directory is None:
            raise ValueError("Content binding missing")
        path = directory / f"{ident}.blob"
        copy_private(path, destination / "content" / path.name)
        if path.stat().st_size != size:
            raise ValueError("Content size changed: ensure Synquay is stopped")
        content[ident] = sha
    manifest = {"format": 1, "database_sha256": digest(target), "content": content}
    key = Path(str(database) + ".connection-secrets.key")
    if needs_connection_key(target) or key.exists():
        check_key(key)
        copy_private(key, destination / "connection-secrets.key")
        manifest["connection_key_sha256"] = digest(destination / "connection-secrets.key")
    (destination / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    os.chmod(destination / "manifest.json", 0o600)
    verify(destination)
    return {"status": "verified", "content_objects": len(content)}


def restore(bundle, destination):
    manifest = verify(bundle)
    destination.mkdir(mode=0o700)
    (destination / "content").mkdir(mode=0o700)
    copy_private(bundle / "database.sqlite3", destination / "database.sqlite3")
    if manifest.get("connection_key_sha256"):
        copy_private(bundle / "connection-secrets.key", destination / "database.sqlite3.connection-secrets.key")
    for ident in manifest["content"]:
        name = f"{uuid.UUID(ident)}.blob"
        copy_private(bundle / "content" / name, destination / "content" / name)
    # Explicit offline relocation; production runtime refuses silently moving its content path.
    with sqlite3.connect(destination / "database.sqlite3") as c:
        c.execute("UPDATE content_binding SET directory=? WHERE singleton=1", (str(destination / "content"),))
    inspect(destination / "database.sqlite3")
    return {"status": "restored_verified", "content_objects": len(manifest["content"])}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["backup", "restore", "verify", "compact"])
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path, nargs="?")
    parser.add_argument("--offline", action="store_true", help="Confirm Synquay and all writers/cleanup workers are stopped")
    args = parser.parse_args()
    if args.operation != "verify" and (not args.offline or (args.operation != "compact" and args.destination is None)):
        parser.error("Stop Synquay and pass --offline plus a new destination directory")
    source = args.source.resolve(strict=True)
    if args.operation == "compact":
        inspect(source)
        if shutil.disk_usage(source.parent).free < source.stat().st_size * 2 + 256 * 1024 * 1024:
            raise ValueError("Insufficient free space for offline compaction")
        with closing(sqlite3.connect(source)) as c:
            busy, _, _ = c.execute("PRAGMA wal_checkpoint(TRUNCATE)").fetchone()
            if busy:
                raise ValueError("Database busy: stop all processes before compacting")
            c.execute("VACUUM")
            c.execute("PRAGMA optimize")
        inspect(source)
        result = {"status": "compacted_verified"}
    elif args.operation == "verify":
        result = {"status": "verified", "content_objects": len(verify(source)["content"])}
    else:
        result = (backup if args.operation == "backup" else restore)(source, args.destination.resolve())
    print(json.dumps(result))


if __name__ == "__main__":
    main()
