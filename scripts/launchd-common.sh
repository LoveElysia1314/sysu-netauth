#!/bin/sh
# Shared by the macOS installation and removal scripts.
label=io.github.peterrobot.sysu-netauth
plist_path=/Library/LaunchDaemons/$label.plist
service_path=/Library/PrivilegedHelperTools/sysu_netauth_service
data_path='/Library/Application Support/SYSUNetAuth'

require_macos_root() {
    [ "$(uname -s)" = Darwin ] || { echo 'This script requires macOS.' >&2; exit 1; }
    [ "$(id -u)" -eq 0 ] || { echo 'Run this script with sudo.' >&2; exit 1; }
}

managed_process_alive() {
    kill -0 "$1" 2>/dev/null || return 1
    process_command=$(/bin/ps -p "$1" -o command=)
    case "$process_command" in
        "$service_path"|"$service_path "*) return 0 ;;
        *) return 1 ;;
    esac
}

stop_daemon() {
    if /bin/launchctl print "system/$label" >/dev/null 2>&1; then
        /bin/launchctl bootout "system/$label"
    fi
    # Cover a process still exiting after bootout and an orphan from an older install.
    for process_id in $(/usr/bin/pgrep -f '^/Library/PrivilegedHelperTools/sysu_netauth_service([[:space:]]|$)' || true); do
        managed_process_alive "$process_id" || continue
        kill -TERM "$process_id" 2>/dev/null || true
        remaining=15
        while managed_process_alive "$process_id" && [ "$remaining" -gt 0 ]; do
            sleep 1
            remaining=$((remaining - 1))
        done
        if managed_process_alive "$process_id"; then
            kill -KILL "$process_id"
            remaining=5
            while managed_process_alive "$process_id" && [ "$remaining" -gt 0 ]; do
                sleep 1
                remaining=$((remaining - 1))
            done
        fi
        if managed_process_alive "$process_id"; then
            echo 'The authentication service has not stopped.' >&2
            exit 1
        fi
    done
}
