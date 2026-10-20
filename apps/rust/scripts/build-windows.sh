#!/bin/sh
# Cross-build the static-CRT MSVC release from Linux.
set -eu
cd "$(dirname "$0")/../../.."
if [ "$#" -ne 0 ]; then
    echo 'Usage: sh apps/rust/scripts/build-windows.sh' >&2
    exit 2
fi
if ! command -v cargo-xwin >/dev/null 2>&1; then
    echo 'Missing cargo-xwin. Install: cargo install --locked cargo-xwin --version 0.23.1' >&2
    exit 1
fi
if [ -z "${NETAUTH_RC:-}" ]; then
    for resource_compiler in llvm-rc llvm-rc-22 llvm-rc-21 llvm-rc-20 llvm-rc-19 llvm-rc-18 llvm-rc-17; do
        if command -v "$resource_compiler" >/dev/null 2>&1; then
            NETAUTH_RC=$(command -v "$resource_compiler")
            export NETAUTH_RC
            break
        fi
    done
fi
if [ -z "${NETAUTH_RC:-}" ] || ! command -v "$NETAUTH_RC" >/dev/null 2>&1; then
    echo 'Missing LLVM resource compiler. Install llvm, or set NETAUTH_RC to llvm-rc.' >&2
    exit 1
fi
if ! command -v zip >/dev/null 2>&1; then
    echo 'Missing zip. Install it before packaging the Windows release.' >&2
    exit 1
fi
# Keep downloaded SDK/CRT files in an ignored build cache. No build tools ship.
: "${XWIN_CACHE_DIR:=$(pwd)/target/xwin}"
export XWIN_CACHE_DIR
# This distribution only targets x64; avoid downloading unused ARM CRT libraries.
XWIN_ARCH=x86_64
export XWIN_ARCH
cargo xwin build --locked --release --target x86_64-pc-windows-msvc --bins
build_dir=${CARGO_TARGET_DIR:-target}/x86_64-pc-windows-msvc/release
# Verify actual PE imports and embedded resources before copying distributables.
host_target=$(rustc -vV | sed -n 's/^host: //p')
cargo run --locked --quiet --target "$host_target" --example verify_windows -- "$build_dir"
rust_dist=apps/rust/dist
mkdir -p "$rust_dist/sysu_netauth"
for executable in sysu_netauth sysu_netauth_service sysu_netauth_gui; do
    install -m755 "$build_dir/$executable.exe" "$rust_dist/sysu_netauth/$executable.exe"
done
# zip only the three binaries; icons and the GUI manifest are embedded.
rm -f "$rust_dist/sysu-netauth-windows-x64.zip"
(cd "$rust_dist" && zip -q -X sysu-netauth-windows-x64.zip sysu_netauth/sysu_netauth.exe sysu_netauth/sysu_netauth_service.exe sysu_netauth/sysu_netauth_gui.exe)
echo "Windows release: $rust_dist/sysu-netauth-windows-x64.zip"
