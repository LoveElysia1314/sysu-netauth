#!/bin/sh
# Installation checks run only on a disposable CI machine, without credentials.
set -eu
[ "${GITHUB_ACTIONS:-}" = true ] || { echo 'Use an ephemeral CI runner.' >&2; exit 1; }
package=$1
resources=/Applications/SYSUNetAuth.app/Contents/Resources
data='/Library/Application Support/SYSUNetAuth'
[ ! -e /Applications/SYSUNetAuth.app ] || { echo 'Existing application found.' >&2; exit 1; }
cleanup() {
    if [ -f "$resources/uninstall-macos.sh" ]; then sudo sh "$resources/uninstall-macos.sh" --purge; fi
    sudo rm -rf /Applications/SYSUNetAuth.app
}
trap cleanup EXIT
sudo install -d -o root -g wheel -m700 "$data"
printf '%s\n' '{"username":"ci-settings-preserved","auto_auth":false}' | sudo tee "$data/config.json" >/dev/null
sudo chmod 600 "$data/config.json"
for attempt in 1 2; do
    if ! sudo installer -verboseR -dumplog -pkg "$package" -target /; then
        sudo tail -n 100 /var/log/install.log
        exit 1
    fi
    codesign --verify --strict /Applications/SYSUNetAuth.app
    remaining=20
    until sudo /usr/local/bin/sysu_netauth status --json > "$RUNNER_TEMP/pkg-status.json"; do
        remaining=$((remaining - 1)); [ "$remaining" -gt 0 ] || exit 1; sleep 1
    done
    grep -q '"state":"idle"' "$RUNNER_TEMP/pkg-status.json"
    sudo /usr/local/bin/sysu_netauth config | grep -q ci-settings-preserved
done
sudo sh "$resources/uninstall-macos.sh"
sudo test -f "$data/config.json"
if sudo launchctl print system/io.github.peterrobot.sysu-netauth >/dev/null 2>&1; then exit 1; fi
echo 'PASS: macOS package install, upgrade, settings preservation and service removal'
