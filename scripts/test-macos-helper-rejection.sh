#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
# A missing daemon must never count as a successful rejection test.
launchctl print system/io.github.peterrobot.sysu-netauth.gui-helper >/dev/null
test_dir=$(mktemp -d "${TMPDIR:-/tmp}/netauth-helper-rejection.XXXXXX")
trap 'rm -rf "$test_dir"' EXIT
swiftc -parse-as-library -module-cache-path "$test_dir/cache" macos/OfflineChecks/RejectClient.swift -o "$test_dir/probe"
codesign --force --options runtime --sign - "$test_dir/probe"
"$test_dir/probe"
