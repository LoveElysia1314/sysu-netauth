#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$base/launchd-common.sh"
require_macos_root
cleanup_staging() { rm -f /usr/local/bin/sysu_netauth.new "$service_path.new"; }
trap cleanup_staging EXIT
if [ -f "$base/sysu_netauth" ]; then
    binaries=$base
    plist=$base/$label.plist
else
    binaries=${CARGO_TARGET_DIR:-$base/../../../target}/release
    plist=$base/../packaging/$label.plist
fi
for executable in sysu_netauth sysu_netauth_service; do
    [ -s "$binaries/$executable" ] || { echo "Missing executable: $binaries/$executable" >&2; exit 1; }
done
/usr/bin/plutil -lint "$plist" >/dev/null
stop_daemon
if [ ! -d /usr/local/bin ]; then install -d -o root -g wheel -m755 /usr/local/bin; fi
install -d -o root -g wheel -m755 /Library/PrivilegedHelperTools
install -d -o root -g wheel -m700 "$data_path" /var/run/sysu-netauth
install -o root -g wheel -m755 "$binaries/sysu_netauth" /usr/local/bin/sysu_netauth.new
install -o root -g wheel -m755 "$binaries/sysu_netauth_service" "$service_path.new"
mv -f /usr/local/bin/sysu_netauth.new /usr/local/bin/sysu_netauth
mv -f "$service_path.new" "$service_path"
install -o root -g wheel -m644 "$plist" "$plist_path"
/bin/launchctl enable "system/$label"
/bin/launchctl bootstrap system "$plist_path"
echo 'Installed. Import settings with: sudo sysu_netauth config import config.json'
