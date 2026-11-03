#!/bin/sh
# Run only on an ephemeral macOS CI runner. Authentication stays disabled.
set -eu
package=$1
label=io.github.peterrobot.sysu-netauth
data='/Library/Application Support/SYSUNetAuth'
[ "${GITHUB_ACTIONS:-}" = true ] || { echo 'Run this test on an ephemeral GitHub Actions runner.' >&2; exit 1; }
for path in "$data" /Library/LaunchDaemons/$label.plist /Library/PrivilegedHelperTools/sysu_netauth_service /usr/local/bin/sysu_netauth; do
    if sudo test -e "$path" || sudo test -L "$path"; then
        echo "Refusing to replace an existing installation during tests: $path" >&2
        exit 1
    fi
done
cleanup() { sudo sh "$package/uninstall-macos.sh" --purge; }
trap cleanup EXIT
sudo install -d -o root -g wheel -m700 "$data"
printf '%s\n' '{"username":"ci-settings-preserved","auto_auth":false}' | sudo tee "$data/config.json" >/dev/null
sudo chmod 600 "$data/config.json"
check_service() {
    remaining=20
    while ! sudo /usr/local/bin/sysu_netauth status --json > /tmp/netauth-ci-status.json; do
        remaining=$((remaining - 1))
        [ "$remaining" -gt 0 ] || return 1
        sleep 1
    done
    grep -q '"state":"idle"' /tmp/netauth-ci-status.json
    sudo /usr/local/bin/sysu_netauth config | grep -q ci-settings-preserved
}
sudo sh "$package/install-macos.sh"
check_service
sudo sh "$package/install-macos.sh"
check_service
sudo sh "$package/uninstall-macos.sh"
if sudo launchctl print "system/$label" >/dev/null 2>&1; then
    echo 'launchd job survived uninstallation' >&2
    exit 1
fi
[ ! -e /Library/PrivilegedHelperTools/sysu_netauth_service ]
sudo test -f "$data/config.json"
sudo sh "$package/uninstall-macos.sh"
