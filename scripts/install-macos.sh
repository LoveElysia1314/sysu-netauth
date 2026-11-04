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
    binaries=${CARGO_TARGET_DIR:-$base/../target}/release
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

# GUI helper is optional for the standalone CLI distribution.
if [ -f "$base/SYSUNetAuthHelper" ] && [ -n "${1:-}" ]; then
    gui_uid=$1
    case "$gui_uid" in ''|*[!0-9]*|0) echo 'Invalid GUI user ID' >&2; exit 1 ;; esac
    gui_binary=$base/../MacOS/SYSUNetAuth
    /usr/bin/codesign --verify --strict "$gui_binary"
    /usr/bin/codesign --verify --strict "$base/SYSUNetAuthHelper"
    gui_hash=$(/usr/bin/codesign -d --verbose=4 "$gui_binary" 2>&1 | sed -n 's/^CDHash=//p')
    case "$gui_hash" in ''|*[!0-9a-f]*) echo 'Invalid GUI code hash' >&2; exit 1 ;; esac
    [ "${#gui_hash}" -eq 40 ] || exit 1
    helper_label=io.github.peterrobot.sysu-netauth.gui-helper
    /bin/launchctl bootout "system/$helper_label" 2>/dev/null || true
    install -o root -g wheel -m755 "$base/SYSUNetAuthHelper" /Library/PrivilegedHelperTools/sysu_netauth_gui_helper
    policy=/Library/PrivilegedHelperTools/sysu_netauth_gui_policy.json
    # Atomically replace, rather than follow a pre-existing destination symlink.
    policy_tmp=$(mktemp /Library/PrivilegedHelperTools/netauth-policy.XXXXXX)
    printf '{"uid":%s,"cdhash":"%s"}\n' "$gui_uid" "$gui_hash" > "$policy_tmp"
    chown root:wheel "$policy_tmp"
    chmod 600 "$policy_tmp"
    mv -f "$policy_tmp" "$policy"
    install -o root -g wheel -m644 "$base/$helper_label.plist" "/Library/LaunchDaemons/$helper_label.plist"
    /bin/launchctl enable "system/$helper_label"
    /bin/launchctl bootstrap system "/Library/LaunchDaemons/$helper_label.plist"
fi
