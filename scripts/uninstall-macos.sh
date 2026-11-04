#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$base/launchd-common.sh"
require_macos_root
purge=false
remove_app=false
for argument in "$@"; do
    case "$argument" in
        --purge) purge=true ;;
        --remove-app) remove_app=true ;;
        *) echo 'Usage: sudo sh uninstall-macos.sh [--purge] [--remove-app]' >&2; exit 1 ;;
    esac
done
app=/Applications/SYSUNetAuth.app
if [ "$remove_app" = true ] && [ -e "$app" ]; then
    [ ! -L "$app" ] || { echo 'Refusing to remove a symlinked application.' >&2; exit 1; }
    bundle_id=$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$app/Contents/Info.plist")
    [ "$bundle_id" = io.github.peterrobot.sysu-netauth.gui ] || { echo 'Unexpected application identifier.' >&2; exit 1; }
fi
helper_label=io.github.peterrobot.sysu-netauth.gui-helper
notification_label=io.github.peterrobot.sysu-netauth.notifications
# Unload every active user's notification job before terminating its process.
# Removing only the foreground user's job leaves fast-user-switching sessions running.
rm -f "/Library/LaunchAgents/$notification_label.plist"
for user_id in $(/bin/ps -axo uid= | /usr/bin/sort -un); do
    /bin/launchctl bootout "gui/$user_id/$notification_label" 2>/dev/null || true
done
for process in $(/usr/bin/pgrep -f '^/Applications/SYSUNetAuth\.app/Contents/MacOS/SYSUNetAuth --notifications([[:space:]]|$)' || true); do
    kill -TERM "$process" 2>/dev/null || true
done
/bin/launchctl bootout "system/$helper_label" 2>/dev/null || true
rm -f "/Library/LaunchDaemons/$helper_label.plist" /Library/PrivilegedHelperTools/sysu_netauth_gui_helper /Library/PrivilegedHelperTools/sysu_netauth_gui_policy.json
stop_daemon
rm -f "$plist_path" "$service_path" "$service_path.new" /usr/local/bin/sysu_netauth /usr/local/bin/sysu_netauth.new
rm -rf /var/run/sysu-netauth
if [ "$purge" = true ]; then rm -rf "$data_path"; fi
# The GUI waits for this script, then exits. Its executable can be removed while running.
if [ "$remove_app" = true ]; then
    rm -rf "$app"
    /usr/sbin/pkgutil --forget io.github.peterrobot.sysu-netauth >/dev/null 2>&1 || true
fi
echo 'SYSU NetAuth components removed.'
