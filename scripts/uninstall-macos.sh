#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$base/launchd-common.sh"
require_macos_root
case "${1:-}" in
    ''|--purge) ;;
    *) echo 'Usage: sudo sh uninstall-macos.sh [--purge]' >&2; exit 1 ;;
esac
helper_label=io.github.peterrobot.sysu-netauth.gui-helper
/bin/launchctl bootout "system/$helper_label" 2>/dev/null || true
rm -f "/Library/LaunchDaemons/$helper_label.plist" /Library/PrivilegedHelperTools/sysu_netauth_gui_helper /Library/PrivilegedHelperTools/sysu_netauth_gui_policy.json
stop_daemon
rm -f "$plist_path" "$service_path" "$service_path.new" /usr/local/bin/sysu_netauth /usr/local/bin/sysu_netauth.new
rm -rf /var/run/sysu-netauth
if [ "${1:-}" = --purge ]; then rm -rf "$data_path"; fi
