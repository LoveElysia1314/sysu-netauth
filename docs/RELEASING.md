# Releases

## Tags and CI

Versions come from Git tags. Rust uses `git-version`; Python uses `setuptools-scm`.

| Tag | Rust | Python |
| --- | --- | --- |
| `v0.7.0` | `0.7.0` | `0.7.0` |
| `v0.7.0-alpha.0` | `0.7.0-alpha.0` | `0.7.0a0` |
| `v0.7.0-beta.2` | `0.7.0-beta.2` | `0.7.0b2` |
| `v0.7.0-rc.1` | `0.7.0-rc.1` | `0.7.0rc1` |

Use a lowercase `v` prefix. Pushing a release tag runs **Release packages** and creates a draft GitHub release. Publishing a release also builds and attaches its packages. Prerelease tags produce prereleases. Review installation, upgrade and service operation before publishing the draft.

The workflow can also run manually on a release tag. Existing releases receive the installers without changes to their notes or publication status. Reruns replace assets with matching filenames.

| Application | Installer | Checksums |
| --- | --- | --- |
| Rust, MSVC x64 | `SYSUNetAuth_Rust_msvc_x64_Setup_v<version>.exe` | `SHA256SUMS-rust-msvc-x64.txt` |
| Rust, GNU x64 | `SYSUNetAuth_Rust_gnu_x64_Setup_v<version>.exe` | `SHA256SUMS-rust-gnu-x64.txt` |
| Python, x64 | `SYSUNetAuth_Python_x64_Setup_v<version>.exe` | `SHA256SUMS-python-x64.txt` |

Each Windows variant also provides a ZIP containing the installer and a separate `SYSUNetAuth_<variant>_Portable_v<version>.zip` containing the application binaries. Python's portable archive includes its runtime dependencies.

Linux x64: `SYSUNetAuth_Rust_linux_x64_v<version>.tar.gz`, with `SHA256SUMS-rust-linux-x64.txt`. The archive includes the CLI, service, systemd unit and installation script.

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
python scripts/package_windows.py --variant rust --abi gnu --input-dir apps/rust/dist/gnu/sysu_netauth
python scripts/package_windows.py --variant python --input-dir apps/python/dist/sysu_netauth
```

Output: `dist/windows-installers/`. Options: `--output-dir` and `--compiler` (path to `ISCC.exe`).

Release tooling tests: `python -m unittest discover -s tests -v`.
