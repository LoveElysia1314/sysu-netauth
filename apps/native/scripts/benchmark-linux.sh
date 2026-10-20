#!/bin/sh
# Isolate the measurement from real adapters. No installation or host networking changes.
set -eu
cd "$(dirname "$0")/../../.."
if [ "${1:-}" != '--inside' ]; then
    cargo build --locked --release --bin sysu_netauth_service --example measure_linux
    exec unshare --user --map-root-user --net sh apps/native/scripts/benchmark-linux.sh --inside "${1:-600}"
fi
benchmark_dir=$(mktemp -d /tmp/netauth-benchmark.XXXXXX)
printf '%s\n' '{"native_schema":1,"auto_auth":false}' > "$benchmark_dir/config.json"
target/release/sysu_netauth_service --data-dir "$benchmark_dir" &
service_pid=$!
trap 'kill "$service_pid" 2>/dev/null || true; wait "$service_pid" 2>/dev/null || true; rm -rf "$benchmark_dir"' EXIT HUP INT TERM
sleep 2
target/release/examples/measure_linux "$service_pid" "${2:-600}"
