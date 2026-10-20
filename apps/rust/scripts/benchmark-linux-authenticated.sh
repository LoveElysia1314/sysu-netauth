#!/bin/sh
# Measure the actual authenticated service against a simulated switch, with isolated routing.
set -eu
cd "$(dirname "$0")/../../.."
cargo build --locked --release --example measure_linux
NETAUTH_MEASURE_SECONDS="${1:-600}" cargo test --locked --release --test linux_network -- --ignored --nocapture
