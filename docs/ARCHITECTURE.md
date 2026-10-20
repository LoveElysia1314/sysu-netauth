# Applications and repository layout

This repository contains two runnable implementations of SYSU NetAuth. The Python application is restored from `bc1f8cb`, the last revision before the native rewrite. The Rust application includes subsequent cross-compilation and Windows resume recovery changes. Preserving Python does not introduce a Python dependency into the native binaries.

## Ownership and build boundaries

| Area | Native application | Python application |
| --- | --- | --- |
| Source | `apps/native/src/` | `apps/python/sysu_netauth/` |
| Dependencies | Root `Cargo.lock`, `apps/native/Cargo.toml` | `apps/python/requirements*.txt` and `requirements*.lock` |
| Tests | `apps/native/tests/` and Rust unit tests | `apps/python/tests/` |
| Resources | `apps/native/assets/`, `apps/native/packaging/` | `apps/python/sysu_netauth/assets/` |
| Build scripts | `apps/native/scripts/` | `apps/python/scripts/` |
| Application docs | [Native README](../apps/native/README.md), [architecture](../apps/native/docs/ARCHITECTURE.md) | [Python README](../apps/python/README.md), [architecture](../apps/python/docs/ARCHITECTURE.md) |
| Intermediate outputs | Root `target/` | `apps/python/build/` |
| Release outputs | `apps/native/dist/` | `apps/python/dist/`; Python installer archives retain their original app-directory location |
| CI | `.github/workflows/native.yml` | `.github/workflows/python.yml` |

Cargo commands can run from the repository root or `apps/native`. Release optimization and static MSVC CRT flags remain at the workspace root. Native shell/PowerShell build scripts resolve the repository root from their own location, so calling them from another working directory is supported. Their Inno Setup resources and output remain local to the native app.

Python commands and tests run from `apps/python`, which has its own interpreter version pin, dependency locks, launchers and PyInstaller/Inno Setup scripts. The runtime modules, assets, tests and build scripts are restored unchanged; this reorganization only adds context to its README. No common runtime or cross-language bridge is introduced.

The root `start.bat` continues to launch the native client. Use `apps/python/start.bat` for the Python client. The root `updates/release.json` stays at its published URL, which is hard-coded in both clients; relocating it would break update checks in installed releases. Python also retains its original `apps/python/updates/release.json` as v0.6.2 build metadata: its installer generator checks that local manifest against the Python version. This keeps Python builds independent of future native release metadata. This rearrangement does not publish a new update or change the stable feed version.

## Switching the installed Windows application

Both applications deliberately retain the existing service name (`SYSUNetAuth`), installer AppId, login task and ProgramData directory for compatibility. Keeping both source trees does not enable two simultaneous authentication services. Stop the currently installed service and fully exit its GUI before replacing binaries or installing the other version. Do not connect a Python GUI to a native service or vice versa: their control protocols are different.

Back up the whole `%ProgramData%\SYSUNetAuth` directory before switching; it contains credentials and must stay protected. Native migration backs up the original Python configuration as `config.json.python-backup`, adds `native_schema`, and moves adapter hints into its service cache. It does not migrate Python status/command files into an IPC queue. Retain backups until the selected application works.

When rolling back, install/register the Python service and use its matching GUI. If restoring the pre-migration configuration is necessary, restore it while both GUI and service are stopped. A pre-migration backup may have older credentials or settings, so choose the intended backup explicitly. Do not assume both implementations can safely write the shared directory together.

## Behavior and validation

Python retains v0.6.2 behavior, including its original GUI, file-based status/commands and service retry loop. The native application uses persistent capture, OS network notifications and local IPC. The Windows suspend/resume recovery fix is currently native-only; see [diagnosis and hardware acceptance](../apps/native/docs/RESUME.md). Restoring Python does not imply that fix has been backported.

Native CI retains Linux/Windows checks and Linux-to-Windows MSVC release cross-compilation. Python CI installs its locked Windows dependencies, runs the original tests, builds with PyInstaller, and uploads a separately named artifact. Linux-only Python checks cannot validate pywin32 COM/service integration or a Windows GUI. Real Windows hibernate/Npcap behavior and actual campus network operation still need hardware validation for the chosen implementation.
