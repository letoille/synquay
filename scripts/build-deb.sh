#!/usr/bin/env bash
# Single build entry point shared by local Linux builds and GitHub Actions.
set -euo pipefail
ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"
if [[ $(uname -s) != Linux ]]; then
  echo "Build the .deb on Debian/Ubuntu Linux or in a Linux container." >&2
  exit 1
fi
for tool in cargo npm python3 dpkg-deb dpkg-shlibdeps readelf; do
  command -v "$tool" >/dev/null || { echo "Missing build tool: $tool" >&2; exit 1; }
done
if ! command -v cargo-deb >/dev/null; then
  cargo install cargo-deb --version 3.6.0 --locked
fi
if [[ $(cargo deb --version) != 'cargo-deb 3.6.0' ]]; then
  echo 'Install the packaging version: cargo install cargo-deb --version 3.6.0 --locked --force' >&2
  exit 1
fi
npm ci
npm run test:frontend
npm run build:frontend
# cargo-deb assets are relative to the backend crate.
rm -rf backend/web-dist
cp -R frontend/dist backend/web-dist
cargo build --release --locked -p synquay-server --bin synquay-server
readelf -h target/release/synquay-server >/dev/null
VERSION=$(cargo metadata --no-deps --format-version 1 --locked | python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"] == "synquay-server"))')
DEB_VERSION="${VERSION/-/~}-1"
ARCH=$(dpkg --print-architecture)
mkdir -p target/debian
PACKAGE="synquay_${DEB_VERSION}_${ARCH}.deb"
cargo deb -p synquay-server --no-build --deb-version "$DEB_VERSION" --output "target/debian/$PACKAGE"
python3 scripts/check-deb.py "target/debian/$PACKAGE"
(cd target/debian && sha256sum "$PACKAGE" > SHA256SUMS)
echo "Package: target/debian/$PACKAGE"
