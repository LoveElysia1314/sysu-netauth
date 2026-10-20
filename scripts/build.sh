#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
cargo build --locked --release --bin sysu_netauth --bin sysu_netauth_service
mkdir -p dist/sysu-netauth-linux-x64
install -m755 target/release/sysu_netauth target/release/sysu_netauth_service dist/sysu-netauth-linux-x64/
install -m644 packaging/sysu-netauth.service dist/sysu-netauth-linux-x64/
install -m755 scripts/install-linux.sh dist/sysu-netauth-linux-x64/
tar -C dist -czf dist/sysu-netauth-linux-x64.tar.gz sysu-netauth-linux-x64
