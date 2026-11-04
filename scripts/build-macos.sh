#!/bin/sh
set -eu
cd "$(dirname "$0")/.."
[ "$(uname -s)" = Darwin ] || { echo 'Build on macOS with Xcode Command Line Tools.' >&2; exit 1; }
target=${1:-$(rustc -vV | awk '/^host:/ { print $2 }')}
case "$target" in
    aarch64-apple-darwin) arch=arm64 ;;
    x86_64-apple-darwin) arch=x64 ;;
    *) echo "Unsupported macOS target: $target" >&2; exit 1 ;;
esac
export MACOSX_DEPLOYMENT_TARGET=${MACOSX_DEPLOYMENT_TARGET:-11.0}
export NETAUTH_RUST_TARGET=$target
sh scripts/build-macos-gui.sh
app=dist/SYSUNetAuth.app
stage=$PWD/dist/package-macos-$arch
case "$stage" in "$PWD"/dist/package-macos-*) rm -rf "$stage" ;; *) exit 1 ;; esac
mkdir -p "$stage/Applications"
ditto "$app" "$stage/Applications/SYSUNetAuth.app"
chmod +x packaging/macos/preinstall packaging/macos/postinstall
version=$("$app/Contents/Resources/sysu_netauth" --version | awk '{print $2}')
package=dist/sysu-netauth-macos-$arch.pkg
components=dist/package-macos-$arch.plist
pkgbuild --analyze --root "$stage" "$components"
python3 - "$components" <<'PY'
import plistlib, sys
from pathlib import Path
path = Path(sys.argv[1])
components = plistlib.loads(path.read_bytes())
for component in components:
    component['BundleIsRelocatable'] = False
    component['BundleIsVersionChecked'] = False
path.write_bytes(plistlib.dumps(components))
PY
pkgbuild --root "$stage" --install-location / --identifier io.github.peterrobot.sysu-netauth \
    --version "${version%%-*}" --ownership recommended --component-plist "$components" \
    --scripts packaging/macos "$package"
python3 - "$package" "$arch" "$version" <<'PY'
import hashlib, json, os, subprocess, sys
from pathlib import Path
path, arch, version = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
if os.environ.get('NETAUTH_RELEASE_TAG'):
    release = json.loads(Path('build/release-version.json').read_text())
else:
    release = {'version':version, 'commit':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()}
with path.open('rb') as stream:
    checksum = hashlib.file_digest(stream, 'sha256').hexdigest()
Path(str(path)+'.metadata.json').write_text(json.dumps({'release':release,'arch':arch,'sha256':checksum},indent=2)+'\n')
PY
echo "Built macOS installer: $package"
