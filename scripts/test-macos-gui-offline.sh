#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
check_dir=$(mktemp -d "${TMPDIR:-/tmp}/netauth-gui-check.XXXXXX")
trap 'rm -rf "$check_dir"' EXIT
swiftc -module-cache-path "$check_dir/cache" \
    macos/Sources/NetAuthCore/Client.swift macos/Sources/NetAuthCore/Settings.swift macos/Sources/NetAuthCore/Helper.swift macos/Sources/NetAuthCore/DesktopState.swift macos/Sources/NetAuthCore/StatusMark.swift macos/OfflineChecks/Checks.swift \
    -o "$check_dir/check"
"$check_dir/check"
