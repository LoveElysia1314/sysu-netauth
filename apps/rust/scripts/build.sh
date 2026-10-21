#!/bin/sh
set -eu
cd "$(dirname "$0")/../../.."
cargo build --locked --release --bin sysu_netauth --bin sysu_netauth_service
build_dir=${CARGO_TARGET_DIR:-target}/release
output=apps/rust/dist/sysu-netauth-linux-x64
mkdir -p "$output"
install -m755 "$build_dir/sysu_netauth" "$build_dir/sysu_netauth_service" "$output/"
install -m644 apps/rust/packaging/sysu-netauth.service "$output/"
install -m755 apps/rust/scripts/install-linux.sh "$output/"
if [ -n "${NETAUTH_RELEASE_TAG:-}" ]; then
    install -m644 build/release-version.json "$output/"
else
    rm -f "$output/release-version.json"
fi
tar -C apps/rust/dist -czf apps/rust/dist/sysu-netauth-linux-x64.tar.gz sysu-netauth-linux-x64
