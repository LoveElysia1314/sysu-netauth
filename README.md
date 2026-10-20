# SYSU NetAuth

中山大学有线校园网 802.1X / EAP-MD5 自动认证客户端，使用 Rust 实现。

- Windows 10/11 x64：后台 Windows 服务 + 原生 Win32 设置窗口和系统托盘。
- Linux x64：命令行 + systemd 服务，无桌面依赖。
- 自动选择有线网卡、多网卡故障转移、启动重试、持续响应交换机重认证。
- EAP 成功后验证互联网连通性；验证期间仍持续响应 EAP 请求。
- Windows 服务独立处理睡眠/休眠恢复：重建捕获并自动重新认证，无需打开 GUI；保留手动断开和关闭自动认证的选择。
- 状态通过本地 IPC 推送，无状态文件轮询、心跳写入、Scapy 或 Python/Qt 运行时。

Windows 仍需要 [Npcap](https://npcap.com/#download)。程序支持从官网下载并校验安装程序的数字签名后启动安装向导；安装包不捆绑 Npcap。Linux 使用内核原始套接字，不需要 libpcap。请先退出其他 802.1X 客户端。

## 构建

安装当前稳定版 Rust。仓库提交了 Cargo.lock，构建使用 `--locked`。

```sh
cargo test --locked --workspace
cargo build --locked --release
```

Linux 打包：`sh scripts/build.sh`；产物位于 `dist/sysu-netauth-linux-x64.tar.gz`。

Windows 在安装了 Visual Studio C++ 工具和 Windows SDK 的开发者 PowerShell 中执行：

```powershell
./scripts/build.ps1 -SkipInstaller
# 安装 Inno Setup 6 后可生成安装包：
./scripts/build.ps1
```

输出包含 `sysu_netauth.exe`（CLI）、`sysu_netauth_service.exe`（服务）、`sysu_netauth_gui.exe`（设置/托盘）。打开设置窗口需要管理员权限。GUI 退出后，独立服务模式继续认证；关闭窗口会隐藏到托盘。

### 在 Linux 交叉编译 Windows 发行版

使用 [cargo-xwin](https://github.com/rust-cross/cargo-xwin) 构建相同的 MSVC 目标，保留静态 CRT、LTO、`opt-level=3`、单代码生成单元和符号裁剪。LLVM、SDK 和 cargo-xwin 仅为构建依赖，不随程序分发。

```sh
# Debian / Ubuntu；其他发行版安装对应的 LLVM 工具和 zip。
sudo apt-get install clang llvm lld zip
rustup target add x86_64-pc-windows-msvc
cargo install --locked cargo-xwin --version 0.23.1
sh scripts/build-windows.sh
```

首次构建会下载 Microsoft SDK/CRT，缓存在 `target/xwin`；可通过 `XWIN_CACHE_DIR` 修改。支持 `CARGO_TARGET_DIR`；若 LLVM 工具带版本后缀，可设置 `NETAUTH_RC=/usr/bin/llvm-rc-19`。

产物为 `dist/sysu-netauth-windows-x64.zip`，仅包含三个原生 EXE；GUI 已内嵌图标和管理员权限清单。构建会检查 PE 架构、GUI/控制台子系统、内嵌资源和 DLL 导入，拒绝依赖动态 VC CRT 或 MinGW 运行时的产物。Npcap 仍由 Windows 用户另行安装，并按需加载。

ZIP 中的服务需在 Windows 管理员终端注册并启动；请先解压到固定安装目录：

```powershell
.\sysu_netauth_service.exe --install
sc.exe start SYSUNetAuth
.\sysu_netauth_gui.exe
```

已有 Inno Setup 构建流程可直接使用 `dist/sysu_netauth` 中的 EXE 生成安装包；`build-windows.sh` 本身生成 ZIP，不依赖 Wine 或 Windows 虚拟机。

## Linux 安装

```sh
sh scripts/build.sh
sudo sh scripts/install-linux.sh
sudo sysu_netauth config import /path/to/config.json
sudo sysu_netauth authenticate
sudo sysu_netauth status
```

最小配置：

```json
{"username":"NetID","password":"密码","iface_mode":"auto","auto_auth":true}
```

配置文件包含凭据，请限制其权限。也可以通过 `sudo sysu_netauth config password` 安全输入密码。服务不负责 DHCP 或修改 IP 配置，继续使用系统的网络管理器；需要避免另一认证器同时占用同一网卡。

```sh
sudo systemctl stop sysu-netauth
sudo systemctl disable sysu-netauth
# 删除程序和 unit 前先停止服务；按需保留 /etc/sysu-netauth 和 /var/lib/sysu-netauth。
```

## CLI

```sh
sysu_netauth interfaces
sudo sysu_netauth status --json
sudo sysu_netauth watch --json
sudo sysu_netauth authenticate
sudo sysu_netauth logoff
sudo sysu_netauth config
sudo sysu_netauth config password
sudo sysu_netauth check-update

# 直接访问网卡，需要先停止后台服务；与服务互斥。
sudo sysu_netauth probe --iface eth0
sudo sysu_netauth authenticate --iface eth0 --username NetID --timeout 90
sudo sysu_netauth logoff --iface eth0
```

Windows 在管理员终端使用相同命令，无需 `sudo`。直接认证命令完成认证和连通性检查后退出；持续续期请使用后台服务。`--timeout` 包含连通性验证时间。

新 CLI 使用子命令，旧的 `--list-ifaces` / `--probe-iface` 等顶层参数已移除。配置/状态读取通过服务执行，`interfaces` 直接使用操作系统 API。服务控制仅允许管理员/SYSTEM 或 Linux root，配置输出默认隐藏密码。

## 配置与升级

Windows 配置：`%ProgramData%\SYSUNetAuth\config.json`。首次读取旧 Python 配置时保留 `config.json.python-backup`，迁移凭据、行为设置和已成功网卡缓存。请保留该备份至确认升级成功。

Windows 休眠恢复后，服务先等待约 2 秒，再重新选择网卡并认证；网卡尚未就绪时在 120 秒恢复宽限期内退避重试。无需启动或登录 GUI，稳定连接后没有新增周期轮询。原因分析、日志检查和实机验证见 [休眠恢复说明](docs/RESUME.md)。

Linux 配置：`/etc/sysu-netauth/config.json`；缓存：`/var/lib/sysu-netauth`；IPC：`/run/sysu-netauth/control.sock`。可手动编辑配置后重启服务，或使用 `config import` 即时应用。

| 字段 | 默认值 | 含义 |
| --- | --- | --- |
| `username`, `password` | 空 | GBK 可编码的 NetID 和密码 |
| `iface_mode` | `auto` | 自动选择或 `manual` 固定网卡 |
| `iface` | 空 | 手动模式的网卡名/Windows adapter ID |
| `auto_auth` | `true` | 自动认证及失败重试 |
| `retry_interval` | `60` | 正常重试间隔，15–3600 秒 |
| `service_mode` | `true` | Windows 服务独立运行；关闭时退出 GUI 会停止服务 |
| `launch_gui_on_login` | `false` | Windows 最高权限登录任务 |
| `hide_window_on_login` | `true` | 登录启动时隐藏窗口 |
| `desktop_notify` | `true` | Windows 托盘通知 |

`status.json`、`command.json` 和 `commands/*.json` 不再读写。替代方式为 CLI `status` / `watch` / `authenticate` / `logoff` / `config import`。实时状态不持久化，断开 IPC 表示服务不可用。

互联网验证保持原有语义：使用系统路由向 `119.29.29.29` / `114.114.114.114` 发起 ICMP 检查。这可以通过任意可用网络成功，不代表测试流量一定经过认证网卡；禁用 ICMP 的网络也可能导致验证失败。

## 验证与性能

```sh
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo test --locked --workspace
# Linux 需要 iproute2，并允许创建 user/network namespace：
cargo test --locked --test linux_network -- --ignored --nocapture
# 对已运行的服务进程采样（PID 后的可选参数为秒数）：
cargo run --locked --release --example measure_linux -- SERVICE_PID 600
```

Windows：`./scripts/measure-windows.ps1 -ProcessName sysu_netauth_service -Seconds 600`，再对 `sysu_netauth_gui` 单独采样。目标为稳定状态 CPU 小于单核 0.1%，服务和托盘总内存相对同机器 Python 基线减少至少 50%。这些是验收目标；Windows 实测和校园网真实握手仍需在对应环境完成。

本地 Linux 30 秒采样：待机约 2.75 MiB RSS；已认证且保持捕获会话约 2.94 MiB RSS，两者采样期间均无可测 CPU 时间增长或磁盘写入。环境、测量限制和复现步骤见 [性能记录](docs/PERFORMANCE.md)。

实现细节、IPC、部署验证见 [架构说明](docs/ARCHITECTURE.md)。
