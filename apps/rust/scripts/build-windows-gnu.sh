#!/bin/sh
set -eu
cd "$(dirname "$0")/../../.."
if [ "$#" -ne 0 ]; then
    echo 'Usage: sh apps/rust/scripts/build-windows-gnu.sh' >&2
    exit 2
fi
for tool in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-windres zip; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Missing $tool. Install gcc-mingw-w64-x86-64, binutils-mingw-w64-x86-64 and zip." >&2
        exit 1
    fi
done
cargo build --locked --release --target x86_64-pc-windows-gnu --bins
build_dir=${CARGO_TARGET_DIR:-target}/x86_64-pc-windows-gnu/release
host_target=$(rustc -vV | sed -n 's/^host: //p')
cargo run --locked --quiet --target "$host_target" --example verify_windows -- "$build_dir"
output=apps/rust/dist/gnu/sysu_netauth
mkdir -p "$output"
for executable in sysu_netauth sysu_netauth_service sysu_netauth_gui; do
    install -m755 "$build_dir/$executable.exe" "$output/$executable.exe"
done
archive=apps/rust/dist/sysu-netauth-windows-gnu-x64.zip
rm -f "$archive"
(cd apps/rust/dist/gnu && zip -q -X ../sysu-netauth-windows-gnu-x64.zip sysu_netauth/*.exe)
echo "Windows GNU release: $archive"
