# Releases

## Tags and CI

Versions come from Git tags. Rust uses `git-version`; Python uses `setuptools-scm`.

| Tag | Rust | Python |
| --- | --- | --- |
| `v0.7.0` | `0.7.0` | `0.7.0` |
| `v0.7.0-alpha.0` | `0.7.0-alpha.0` | `0.7.0a0` |
| `v0.7.0-beta.2` | `0.7.0-beta.2` | `0.7.0b2` |
| `v0.7.0-rc.1` | `0.7.0-rc.1` | `0.7.0rc1` |

Commit the release changes, create a tag in one of these formats, and push it. **Release packages** builds both applications and creates a draft GitHub release. Prerelease tags produce prereleases. Review installation, upgrade and service operation before publishing the draft.

Tier 1 (Windows AMD64) and Tier 2 (Windows AARCH64, Linux AMD64) builds and tests must pass. Tier 3 (macOS) is advisory; successful macOS packages are added independently without blocking Windows or Linux releases.

The workflow can also run manually on a release tag. Existing releases receive the installers without changes to their notes or publication status. Reruns replace assets with matching filenames.

| Application | Installer | Checksums |
| --- | --- | --- |
| Rust, MSVC x64 | `SYSUNetAuth_Rust_msvc_x64_Setup_v<version>.exe` | `SHA256SUMS-rust-msvc-x64.txt` |
| Rust, MSVC ARM64 | `SYSUNetAuth_Rust_msvc_arm64_Setup_v<version>.exe` | `SHA256SUMS-rust-msvc-arm64.txt` |
| Rust, GNU x64 | `SYSUNetAuth_Rust_gnu_x64_Setup_v<version>.exe` | `SHA256SUMS-rust-gnu-x64.txt` |
| Python, x64 | `SYSUNetAuth_Python_x64_Setup_v<version>.exe` | `SHA256SUMS-python-x64.txt` |

Each Windows variant also provides a `SYSUNetAuth_<variant>_Portable_v<version>.zip` containing the application binaries. Python's portable archive includes its runtime dependencies.

Linux x64: `SYSUNetAuth_Rust_linux_x64_v<version>.tar.gz`, with `SHA256SUMS-rust-linux-x64.txt`. The archive includes the CLI, service, systemd unit and installation script.

macOS: `SYSUNetAuth_Rust_macos_arm64_v<version>.tar.gz` (Apple Silicon) and `SYSUNetAuth_Rust_macos_x64_v<version>.tar.gz` (Intel), with `SHA256SUMS-rust-macos-<arch>.txt`. Both include the CLI, daemon and launchd installation/removal scripts. macOS support is experimental.

## Local packaging

Use a clean release tag and install Inno Setup 6. From the repository root:

```powershell
python -m pip install -r requirements-build.txt
python scripts/release_version.py --write
```

Build the applications using their [Rust](../apps/rust/README.md) or [Python](../apps/python/README.md) instructions, then package:

```powershell
Copy-Item build/release-version.json apps/rust/dist/sysu_netauth/
Copy-Item build/release-version.json apps/rust/dist/gnu/sysu_netauth/
Copy-Item build/release-version.json apps/python/dist/sysu_netauth/
python scripts/package_windows.py --variant rust --input-dir apps/rust/dist/sysu_netauth
Copy-Item build/release-version.json apps/rust/dist/arm64/sysu_netauth/
python scripts/package_windows.py --variant rust --arch arm64 --input-dir apps/rust/dist/arm64/sysu_netauth
python scripts/package_windows.py --variant rust --abi gnu --input-dir apps/rust/dist/gnu/sysu_netauth
python scripts/package_windows.py --variant python --input-dir apps/python/dist/sysu_netauth
```

Output: `dist/windows-installers/`. Options: `--output-dir` and `--compiler` (path to `ISCC.exe`).

Release tooling tests: `python -m unittest discover -s tests -v`.
