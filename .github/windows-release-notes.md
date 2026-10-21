Choose one Windows x64 setup installer:

- **Rust_msvc_x64**: Rust version, including sleep/hibernate recovery.
- **Rust_gnu_x64**: Rust version built with the GNU toolchain.
- **Python_x64**: Python version.

Windows requires [Npcap](https://npcap.com/#download). Setup ZIPs contain the installer; Portable ZIPs contain application binaries. `SHA256SUMS` files cover all downloads.

**Linux x64**: the Rust `.tar.gz` includes CLI, service, systemd unit and installation script.

Install one version at a time. Before switching, exit the GUI, stop the `SYSUNetAuth` service and back up `%ProgramData%\SYSUNetAuth`.
