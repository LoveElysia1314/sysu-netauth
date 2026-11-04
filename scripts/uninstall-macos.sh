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
notification_label=io.github.peterrobot.sysu-netauth.notifications
console_uid=$(/usr/bin/stat -f %u /dev/console)
/bin/launchctl bootout "gui/$console_uid/$notification_label" 2>/dev/null || true
rm -f "/Library/LaunchAgents/$notification_label.plist"
for process in $(/usr/bin/pgrep -f '^/Applications/SYSUNetAuth.app/Contents/MacOS/SYSUNetAuth --notifications([[:space:]]|$)' || true); do
    kill -TERM "$process" 2>/dev/null || true
done
/bin/launchctl bootout "system/$helper_label" 2>/dev/null || true
rm -f "/Library/LaunchDaemons/$helper_label.plist" /Library/PrivilegedHelperTools/sysu_netauth_gui_helper /Library/PrivilegedHelperTools/sysu_netauth_gui_policy.json
stop_daemon
rm -f "$plist_path" "$service_path" "$service_path.new" /usr/local/bin/sysu_netauth /usr/local/bin/sysu_netauth.new
rm -rf /var/run/sysu-netauth
if [ "${1:-}" = --purge ]; then rm -rf "$data_path"; fi
