# SYSU NetAuth

中山大学有线校园网 802.1X / EAP-MD5 自动认证客户端。

| 版本 | 平台 | 文档 |
| --- | --- | --- |
| Rust | Windows 10/11 x64 / ARM64、Linux x64、macOS Apple Silicon / Intel | [安装、构建与 CLI](apps/rust/README.md) |
| Python | Windows | [运行、构建与 CLI](apps/python/README.md) |

## Windows 安装

从 [Releases](https://github.com/peterrobot/sysu-netauth/releases) 下载对应的 Setup 安装程序：

- `SYSUNetAuth_Rust_msvc_x64_Setup_v<版本>.exe`
- `SYSUNetAuth_Rust_msvc_arm64_Setup_v<版本>.exe`（Windows on ARM）
- `SYSUNetAuth_Rust_gnu_x64_Setup_v<版本>.exe`
- `SYSUNetAuth_Python_x64_Setup_v<版本>.exe`

两个版本均需 [Npcap](https://npcap.com/#download)。安装后打开设置窗口，填写 NetID 和密码并认证。启用「服务独立运行」后，退出 GUI 仍会保持认证。Rust 版支持睡眠、休眠后自动重认证。

Rust 的 MSVC 与 GNU 安装包功能相同，一般选择 MSVC。Rust 和 Python 共用服务与配置，不能同时运行。版本切换与故障排查见 [Windows 安装说明](docs/WINDOWS.md)。

Windows ARM64 与 macOS 支持仍处于实验阶段。

发行页同时提供 Windows 便携 ZIP、Linux x64 与 macOS 程序包，以及 SHA-256 校验文件。macOS 使用方法见 [macOS 文档](apps/rust/docs/MACOS.md)。

## 从源码构建

Rust（仓库根目录）：

```sh
cargo build --locked --release
```

Windows 打包、Linux 安装及 Linux → Windows 交叉编译见 [Rust 文档](apps/rust/README.md)；Python 开发环境见 [Python 文档](apps/python/README.md)。

版本由 Git 标签生成，标签格式与发行步骤见 [发行说明](docs/RELEASING.md)。
