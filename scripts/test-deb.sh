#!/usr/bin/env bash
# Destructive to the test installation only. Run on an isolated Linux CI runner.
set -euo pipefail
[[ $EUID == 0 ]] || { echo 'Run with sudo on an isolated test runner.' >&2; exit 1; }
[[ -d /run/systemd/system ]] || { echo 'A running systemd is required.' >&2; exit 1; }
[[ ! -e /var/lib/synquay && ! -e /etc/synquay ]] || { echo 'Refusing to touch an existing installation.' >&2; exit 1; }
ROOT=$(cd "$(dirname "$0")/.." && pwd)
PACKAGE=$(realpath "$1")
apt-get install -y "$PACKAGE"
python3 "$ROOT/scripts/smoke-installed.py"
systemctl is-active --quiet synquay
PID=$(systemctl show synquay --property MainPID --value)
[[ $(ps -o user= -p "$PID" | xargs) == synquay ]]
[[ $(stat -c '%a' /etc/synquay/credentials.env) == 600 ]]
echo '# upgrade preservation check' >> /etc/synquay/synquay.env
touch /var/lib/synquay/package-test-marker
SUMS=$(mktemp)
trap 'rm -f "$SUMS"' EXIT
sha256sum /etc/synquay/{credentials.env,tools.json,pipeline.json,security.json,synquay.env} > "$SUMS"
dpkg --force-confold -i "$PACKAGE"
python3 "$ROOT/scripts/smoke-installed.py"
sha256sum --check "$SUMS"
systemctl is-active --quiet synquay
apt-get remove -y synquay
! systemctl is-active --quiet synquay
[[ -f /var/lib/synquay/package-test-marker && -f /var/lib/synquay/synquay.db ]]
sha256sum --check "$SUMS"
apt-get purge -y synquay
[[ -f /var/lib/synquay/package-test-marker && -f /var/lib/synquay/synquay.db ]]
[[ -f /etc/synquay/credentials.env && -f /etc/synquay/tools.json ]]
echo 'Install, same-version upgrade, removal and retained data verified'
