#!/bin/sh
set -eu
cd "$(dirname "$0")/../../.."
cargo build --locked --release --bin sysu_netauth --bin sysu_netauth_service
build_dir=${CARGO_TARGET_DIR:-target}/release
output=apps/native/dist/sysu-netauth-linux-x64
mkdir -p "$output"
install -m755 "$build_dir/sysu_netauth" "$build_dir/sysu_netauth_service" "$output/"
install -m644 apps/native/packaging/sysu-netauth.service "$output/"
install -m755 apps/native/scripts/install-linux.sh "$output/"
tar -C apps/native/dist -czf apps/native/dist/sysu-netauth-linux-x64.tar.gz sysu-netauth-linux-x64
