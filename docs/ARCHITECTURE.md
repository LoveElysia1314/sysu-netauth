# Repository layout and version switching

| Directory | Contents |
| --- | --- |
| `apps/rust/` | Rust application, tests, resources and build scripts |
| `apps/python/` | Python application, tests, resources and build scripts |
| `scripts/`, `tests/` | Shared release tooling and tests |
| `updates/` | Published update feed; keep its URL stable |

Run Cargo commands from the repository root and Python application commands from `apps/python`. The root `start.bat` launches Rust; `apps/python/start.bat` launches Python. Build outputs are under each application's `dist/` directory.

## Switching on Windows

Both installers use the `SYSUNetAuth` service and `%ProgramData%\SYSUNetAuth` settings directory. Install one version at a time and use its matching GUI and service.

1. Back up `%ProgramData%\SYSUNetAuth`; it contains credentials.
2. Run the selected version's installer.

See [Windows installation and maintenance](WINDOWS.md) for requirements and troubleshooting.

Rust saves the Python configuration as `config.json.python-backup` on first migration. To restore it when returning to Python, stop the GUI and service first, then restore the intended backup; its credentials and settings may be older.

The [sleep/hibernate recovery fix](../apps/rust/docs/RESUME.md) is available in Rust only.
