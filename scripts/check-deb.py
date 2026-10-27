#!/usr/bin/env python3
"""Inspect the actual Debian archive, not only its source manifest."""
from pathlib import Path
import subprocess
import sys
import tempfile

package = Path(sys.argv[1]).resolve()
assert subprocess.check_output(["dpkg-deb", "-f", str(package), "Package"], text=True).strip() == "synquay"
with tempfile.TemporaryDirectory() as temp:
    root = Path(temp)
    subprocess.run(["dpkg-deb", "-x", str(package), str(root)], check=True)
    subprocess.run(["dpkg-deb", "-e", str(package), str(root / "DEBIAN")], check=True)
    for file in ("usr/bin/synquay-server", "usr/share/synquay/web/index.html", "usr/share/synquay/web/brand/flow-icon.svg", "usr/share/synquay/initialize.py", "etc/synquay/synquay.env"):
        assert (root / file).is_file(), file
    assert list((root / "usr/share/synquay/web/assets").glob("*.js"))
    units = list(root.glob("**/systemd/system/synquay.service"))
    assert len(units) == 1
    assert "User=synquay" in units[0].read_text()
    assert "StandardOutput=append:/var/log/synquay/synquay.log" in units[0].read_text()
    assert "LogsDirectory=synquay" in units[0].read_text()
    assert (root / "etc/logrotate.d/synquay").is_file()
    assert "/etc/logrotate.d/synquay" in (root / "DEBIAN/conffiles").read_text()
    binary = (root / "usr/bin/synquay-server").read_bytes()
    assert binary[:4] == b"\x7fELF", "Not a Linux ELF binary"
    assert (root / "usr/bin/synquay-server").stat().st_mode & 0o111
    assert not list(root.rglob("*.db")), "Runtime database must not be packaged"
    assert not (root / "etc/synquay/credentials.env").exists(), "Do not ship shared secrets"
    assert "/etc/synquay/synquay.env" in (root / "DEBIAN/conffiles").read_text()
    for script in ("postinst", "prerm", "postrm"):
        path = root / "DEBIAN" / script
        assert path.is_file(), script
        assert "#DEBHELPER#" not in path.read_text(), "Unexpanded systemd scripts"
        subprocess.run(["sh", "-n", str(path)], check=True)
print("Debian archive content and maintainer scripts verified")
