#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo 'Build on macOS.' >&2; exit 1; }
python3 scripts/generate-icons.py --check
target=${NETAUTH_RUST_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}
host=$(rustc -vV | awk '/^host:/ { print $2 }')
[ "$target" = "$host" ] || { echo 'Build the complete GUI on a matching architecture macOS host.' >&2; exit 1; }
cargo build --locked --release --target "$target" --bin sysu_netauth --bin sysu_netauth_service
swift build --package-path macos --build-system native -c release "$@"
bin_dir=$(swift build --package-path macos --build-system native -c release "$@" --show-bin-path)
app=dist/SYSUNetAuth.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
icon_dir=$(mktemp -d "${TMPDIR:-/tmp}/netauth-icon.XXXXXX")
trap 'rm -rf "$icon_dir"' EXIT
"$bin_dir/NetAuthIconPreview" --export-macos "$icon_dir/AppIcon.iconset"
iconutil -c icns "$icon_dir/AppIcon.iconset" -o "$app/Contents/Resources/AppIcon.icns"
rust_bin=${CARGO_TARGET_DIR:-target}/$target/release
version=$("$rust_bin/sysu_netauth" --version | awk '{print $2}')
core_version=${version%%-*}
install -m755 "$rust_bin/sysu_netauth" "$rust_bin/sysu_netauth_service" "$app/Contents/Resources/"
install -m755 scripts/install-macos.sh scripts/uninstall-macos.sh "$app/Contents/Resources/"
install -m644 scripts/launchd-common.sh packaging/io.github.peterrobot.sysu-netauth.plist "$app/Contents/Resources/"
install -m755 "$bin_dir/SYSUNetAuthHelper" "$app/Contents/Resources/SYSUNetAuthHelper"
install -m644 packaging/io.github.peterrobot.sysu-netauth.gui-helper.plist "$app/Contents/Resources/"
install -m755 "$bin_dir/SYSUNetAuth" "$app/Contents/MacOS/SYSUNetAuth"
cat > "$app/Contents/Info.plist" <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>io.github.peterrobot.sysu-netauth.gui</string>
<key>CFBundleName</key><string>SYSU NetAuth</string>
<key>CFBundleExecutable</key><string>SYSUNetAuth</string>
<key>CFBundleIconFile</key><string>AppIcon</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $core_version" "$app/Contents/Info.plist"
/usr/libexec/PlistBuddy -c "Set :CFBundleVersion $core_version" "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist"
if [ -n "${NETAUTH_RELEASE_TAG:-}" ]; then
    install -m644 build/release-version.json "$app/Contents/Resources/"
else
    rm -f "$app/Contents/Resources/release-version.json"
fi
for binary in SYSUNetAuthHelper sysu_netauth sysu_netauth_service; do
    codesign --force --options runtime --sign - "$app/Contents/Resources/$binary"
done
codesign --force --options runtime --sign - "$app"
codesign --verify --strict "$app"
echo "Built ad-hoc signed development app: $app"
