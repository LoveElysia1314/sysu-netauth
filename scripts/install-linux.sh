#!/bin/sh
set -eu
if [ "$(id -u)" -ne 0 ]; then
    echo 'Run this script as root.' >&2
    exit 1
fi
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
if [ -f "$base/sysu_netauth" ]; then
    binaries=$base
    unit=$base/sysu-netauth.service
else
    binaries=$base/../target/release
    unit=$base/../packaging/sysu-netauth.service
fi
systemctl stop sysu-netauth.service 2>/dev/null || true
install -m755 "$binaries/sysu_netauth" "$binaries/sysu_netauth_service" /usr/local/bin/
install -d -m700 /etc/sysu-netauth /var/lib/sysu-netauth /run/sysu-netauth
install -m644 "$unit" /etc/systemd/system/sysu-netauth.service
systemctl daemon-reload
systemctl enable --now sysu-netauth.service
echo 'Installed. Import settings with: sudo sysu_netauth config import config.json'
