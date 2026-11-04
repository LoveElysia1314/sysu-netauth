# 更新日志

## [0.7.0](https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0) - 2026-11-04

### 平台与安装

- 新增 macOS 和 Linux 支持。Windows 提供 Win32 界面，macOS 提供 AppKit 界面，Linux 提供命令行和 systemd 服务。
- Windows 改用系统自带的 NDISUIO 驱动。由 v0.6.3 升级且仍装有 Npcap 时，安装向导提供可选的清理入口。
- macOS 应用菜单提供卸载入口，统一移除应用、自动启动项和后台组件，可选择清理账户、设置和日志。
- 提供 Windows x64、ARM64，macOS Apple Silicon、Intel 和 Linux x64 安装包。macOS PKG 包含应用及后台组件，升级时保留已保存的配置。

### 认证与日常使用

- 使用 Rust 重写认证核心，统一管理认证、续期、网卡切换和失败重试。后台服务持续运行，状态程序按需打开。
- 应用图标采用白底圆角与单色网口盾牌，菜单栏和托盘继续显示认证状态。
- Windows 和 macOS 采用四格布局，集中展示账户与认证、网络状态、常规设置和应用信息。账户区提供认证、断开连接和退出程序；应用信息区提供检查更新、运行日志和更新日志。账户信息在点击认证时保存，常规选项修改后立即保存。
- 网卡列表随插拔刷新，按设备 ID 保留选择。服务处理断线恢复和 Windows 睡眠、休眠后的重新认证；手动断开后等待用户重新连接。
- 增加独立通知助手。启用通知后，退出状态程序仍可收到连接变化和正式版更新提醒。短于 5 秒的连接中断保持静默，持续中断及恢复分别提醒一次。
- 更新检查同时查询 GitHub 和 Gitee，采用最新可用的正式版信息。自动检查与新版通知分别设置，可手动检查、查看说明和忽略指定版本；更新日志优先展示未忽略的新版说明。
- 运行日志优先保留最新记录，遮盖当前账号与密码；安装、升级时设置配置和日志的访问权限。

### 安装提示与验证

macOS 应用采用 ad-hoc 签名，安装包尚无 Developer ID 签名或 Apple 公证。首次打开时请按[安装说明](docs/MACOS.md#首次打开时的安全提示)完成系统授权。通知助手需要 macOS 13+。

Windows 已完成校园网实连验证；macOS 两种架构已完成离线、服务及安装升级测试，校园网实连和电源恢复待设备验证。详细范围见[测试文档](docs/VALIDATION.md)。

### 致谢

感谢 [@peterrobot](https://github.com/peterrobot) 在 [PR #5](https://github.com/LoveElysia1314/sysu-netauth/pull/5) 中提供 Rust 重构版本，以及 Windows 原生界面、多平台构建与安装支持、连接恢复等改进。

[与 v0.6.3 比较](https://github.com/LoveElysia1314/sysu-netauth/compare/v0.6.3...v0.7.0)
