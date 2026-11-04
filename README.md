# SYSU NetAuth

中山大学有线校园网自动认证客户端，支持 802.1X / EAP-MD5。Windows 和 macOS 提供图形界面，Linux 提供命令行；后台服务负责持续认证和断线重连。

当前正式版为 **v0.7.0**，从 [GitHub](https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0) 或 [Gitee](https://gitee.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0) 下载。

## 下载与安装

| 平台 | 下载文件标识 | 使用方式 |
| --- | --- | --- |
| Windows 10/11 x64 | `windows_x64` | 原生界面、命令行、后台服务 |
| Windows 10/11 ARM64 | `windows_arm64` | 原生界面、命令行、后台服务 |
| Linux x64 | `linux_x64` | 命令行、systemd 服务 |
| macOS 11+ Apple Silicon / Intel | `macos_arm64` / `macos_x64` | PKG 安装包，含原生界面、菜单栏和后台服务 |

Windows 下载对应架构的 `Setup.exe`，macOS 下载 `.pkg` 安装包。安装后填写 NetID 和密码，点击“认证”。后台服务独立运行，退出界面后仍可自动认证。

macOS 首次打开时若提示“Apple 无法验证是否包含恶意软件”，请按[首次打开说明](docs/MACOS.md#首次打开时的安全提示)为该安装包单独授权。

Linux 压缩包包含需要 root 权限执行的系统服务安装脚本。

- [Windows 安装与排障](docs/WINDOWS.md)
- [Linux 安装与服务管理](docs/LINUX.md)
- [macOS 安装与服务管理](docs/MACOS.md)

## 功能

- 自动选择有线网卡，支持按设备固定选择和失败后切换候选网卡。
- 持续响应交换机的重新认证请求，自动处理断线和 Windows 睡眠、休眠恢复。
- 网卡插拔时更新选择列表，保留尚未保存的选择。
- Windows 界面跟随系统浅色或深色主题，提供托盘状态、登录启动和通知选项。
- 后台检查正式版更新；Windows 和 macOS 可手动检查、查看发布说明或忽略版本，Linux 在命令行状态中提示更新。
- Windows 和 macOS 13+ 提供独立通知助手，退出状态程序后仍可接收联网状态及正式版更新提醒。

程序负责有线认证，IP 地址和 DHCP 由系统网络管理器处理。使用前请退出其他 802.1X 客户端。

## 文档与开发

[命令行与配置](docs/CLI.md) · [源码构建](docs/BUILDING.md) · [macOS 界面开发](macos/README.md) · [架构](docs/ARCHITECTURE.md) · [测试](docs/VALIDATION.md) · [性能测量](docs/PERFORMANCE.md) · [发行维护](docs/RELEASING.md)

从仓库根目录构建：

```sh
cargo build --locked --release
```

Windows 构建需要 Visual Studio C++ 工具和 Windows SDK；制作安装包还需要 Inno Setup 6。各平台的依赖、输出目录和交叉编译方法见[构建文档](docs/BUILDING.md)。

版本说明见[更新日志](CHANGELOG.md)，安装包见 [GitHub Releases](https://github.com/LoveElysia1314/sysu-netauth/releases)。
