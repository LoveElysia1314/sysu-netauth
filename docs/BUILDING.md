# 源码构建

使用稳定版 Rust 工具链，从仓库根目录执行：

```sh
cargo build --locked --release
```

程序版本统一读取 `Cargo.toml`。发行标签必须与包版本一致，并指向当前提交。依赖锁定在 `Cargo.lock`；发行工具使用 Python 3.11+ 标准库。

## Windows

安装 Visual Studio C++ 工具和 Windows SDK，在开发者 PowerShell 中构建：

```powershell
./scripts/build.ps1 -SkipInstaller
```

输出为 `dist/sysu_netauth/` 中的 CLI、GUI 和服务三个 EXE。安装 Inno Setup 6 后，省略 `-SkipInstaller` 可同时生成 Setup 安装包。服务注册和启动方式见 [Windows 文档](WINDOWS.md)。

脚本默认使用 Rust 主机架构。交叉构建 ARM64 还需安装 VS 的 ARM64 C++ 工具与对应 Rust 目标：

```powershell
rustup target add aarch64-pc-windows-msvc
./scripts/build.ps1 -Target aarch64-pc-windows-msvc -SkipInstaller
```

ARM64 输出位于 `dist/arm64/sysu_netauth/`。

## Linux 与 macOS

Linux 构建并生成包含 systemd 单元及安装脚本的压缩包：

```sh
sh scripts/build.sh
```

输出为 `dist/sysu-netauth-linux-x64.tar.gz`。安装方式见 [Linux 文档](LINUX.md)。

macOS 构建需要 Xcode Command Line Tools：

```sh
sh scripts/build-macos.sh
```

脚本按主机架构生成 Apple Silicon 或 Intel 程序包，也可显式传入 `aarch64-apple-darwin` 或 `x86_64-apple-darwin`。安装方式见 [macOS 文档](MACOS.md)。

## Linux 交叉构建 Windows

MSVC 构建使用静态 CRT，需要 LLVM、zip 和 cargo-xwin：

```sh
sudo apt-get install clang llvm lld zip
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin --version 0.23.1
sh scripts/build-windows.sh
```

ARM64 构建：

```sh
rustup target add aarch64-pc-windows-msvc
sh scripts/build-windows.sh aarch64-pc-windows-msvc
```

这些 ZIP 仅用于 CI 构建传递与开发验证，不作为公开发行附件。输出为 `dist/sysu-netauth-windows-x64.zip` 或 `dist/arm64/sysu-netauth-windows-arm64.zip`。SDK 缓存默认位于 `target/xwin`，可用 `XWIN_CACHE_DIR` 修改；资源编译器可用 `NETAUTH_RC` 指定。

GNU 构建使用 MinGW：

```sh
sudo apt-get install gcc-mingw-w64-x86-64 binutils-mingw-w64-x86-64 zip
rustup target add x86_64-pc-windows-gnu
sh scripts/build-windows-gnu.sh
```

输出位于 `dist/gnu/sysu_netauth/`，压缩包为 `dist/sysu-netauth-windows-gnu-x64.zip`。
