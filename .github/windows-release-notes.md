Choose the Windows setup installer for your machine:

- **Rust_msvc_x64**: Rust version, including sleep/hibernate recovery.
- **Rust_msvc_arm64**: ARM64 Rust version for Windows on ARM (experimental).
- **Rust_gnu_x64**: Rust version built with the GNU toolchain.
- **Python_x64**: Python version.

Windows requires [Npcap](https://npcap.com/#download). Portable ZIPs contain application binaries. `SHA256SUMS` files cover all downloads.

**Linux x64**: the Rust `.tar.gz` includes CLI, service, systemd unit and installation script.

**macOS Apple Silicon / Intel (experimental)**: the Rust `.tar.gz` includes CLI, daemon and launchd installation/removal scripts.
