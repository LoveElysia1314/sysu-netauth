#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo 'Build on macOS.' >&2; exit 1; }
cargo build --locked --release --bin sysu_netauth --bin sysu_netauth_service
swift build --package-path macos --build-system native -c release "$@"
bin_dir=$(swift build --package-path macos --build-system native -c release "$@" --show-bin-path)
app=dist/SYSUNetAuth.app
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
rust_bin=${CARGO_TARGET_DIR:-target}/release
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
<key>CFBundleIdentifier</key><string>io.github.peterrobot.sysu-netauth.gui.experimental</string>
<key>CFBundleName</key><string>SYSU NetAuth</string>
<key>CFBundleExecutable</key><string>SYSUNetAuth</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleShortVersionString</key><string>0.1.0</string>
<key>LSMinimumSystemVersion</key><string>11.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST
plutil -lint "$app/Contents/Info.plist"
for binary in SYSUNetAuthHelper sysu_netauth sysu_netauth_service; do
    codesign --force --options runtime --sign - "$app/Contents/Resources/$binary"
done
codesign --force --options runtime --sign - "$app"
codesign --verify --strict "$app"
echo "Built ad-hoc signed development app: $app"
