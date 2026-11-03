#!/bin/sh
set -eu
base=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "$base/launchd-common.sh"
require_macos_root
case "${1:-}" in
    ''|--purge) ;;
    *) echo 'Usage: sudo sh uninstall-macos.sh [--purge]' >&2; exit 1 ;;
esac
stop_daemon
rm -f "$plist_path" "$service_path" "$service_path.new" /usr/local/bin/sysu_netauth /usr/local/bin/sysu_netauth.new
rm -rf /var/run/sysu-netauth
if [ "${1:-}" = --purge ]; then rm -rf "$data_path"; fi
