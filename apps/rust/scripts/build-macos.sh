#!/bin/sh
set -eu
cd "$(dirname "$0")/../../.."
[ "$(uname -s)" = Darwin ] || { echo 'Build on macOS with Xcode Command Line Tools.' >&2; exit 1; }
target=${1:-$(rustc -vV | awk '/^host:/ { print $2 }')}
case "$target" in
    aarch64-apple-darwin) arch=arm64 ;;
    x86_64-apple-darwin) arch=x64 ;;
    *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
cargo build --locked --release --target "$target" --bin sysu_netauth --bin sysu_netauth_service
build_dir=${CARGO_TARGET_DIR:-target}/$target/release
output=apps/rust/dist/sysu-netauth-macos-$arch
mkdir -p "$output"
install -m755 "$build_dir/sysu_netauth" "$build_dir/sysu_netauth_service" "$output/"
install -m644 apps/rust/packaging/io.github.peterrobot.sysu-netauth.plist "$output/"
install -m755 apps/rust/scripts/install-macos.sh apps/rust/scripts/uninstall-macos.sh "$output/"
install -m644 apps/rust/scripts/launchd-common.sh "$output/"
if [ -n "${NETAUTH_RELEASE_TAG:-}" ]; then
    install -m644 build/release-version.json "$output/"
else
    rm -f "$output/release-version.json"
fi
COPYFILE_DISABLE=1 tar -C apps/rust/dist -czf "apps/rust/dist/sysu-netauth-macos-$arch.tar.gz" "sysu-netauth-macos-$arch"
