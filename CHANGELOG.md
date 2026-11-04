# 更新日志

## [0.7.1](https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.1) - 2026-11-04

- Windows 增加独立通知助手，登录后在后台运行；退出状态程序后，仍可接收已启用的联网状态通知和正式版更新提醒。
- macOS 13+ 增加无窗口通知助手，由 LaunchAgent 在登录后启动。助手与界面按用户去重，保持连接通知的 5 秒防抖。
- macOS 增加“软件更新”菜单，可手动检查、查看新版说明或忽略版本。Windows 和 Mac 对每个新版本提醒一次，点击通知打开对应发布说明。
- Linux 在命令行状态中显示正式版更新及发布链接；服务首次发现每个新版本时记入日志。
- Windows 从依赖 Npcap 的旧客户端升级，且系统仍装有 Npcap 时，提供可选的手动清理入口。首次安装和原生认证版本之间的升级保持原有流程。
- 发布说明统一从本文件提取对应版本的内容，更新入口与发布页使用同一份说明。

通知仍受系统权限、专注模式等设置影响。Mac 后台通知助手需要 macOS 13+；macOS 11/12 保留界面内的单次授权操作。

## [0.7.0](https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0) - 2026-11-04

- 采用 Rust 认证核心，Windows 使用 Win32 界面，macOS 使用 AppKit 界面；Python、Qt 和 PySide6 客户端停止维护。
- Windows 使用系统自带的 NDISUIO 驱动完成有线认证，Npcap 后端及 EAPHost 验证方案退役。
- macOS 提供包含完整应用、CLI、后台服务和授权 helper 的 PKG，修复菜单栏图标重复绘制造成的高 CPU 占用。
- 后台服务独立运行，统一断线恢复和重试策略；连接通知采用 5 秒防抖，短暂中断恢复时保持静默。
- 增加日志查看和诊断导出，修复长日志遗漏最新记录的问题；导出时遮盖当前账号和密码。Windows 安装、升级时修复配置和日志权限。
- Windows x64、ARM64，macOS Apple Silicon、Intel，以及 Linux x64 均提供安装包和 SHA-256 校验清单。发行文件按平台和架构命名。

Windows 已完成校园网实连和恢复验证。Mac 两种架构已完成离线、服务和安装升级测试，校园网实连与电源恢复仍需设备验证；应用采用 ad-hoc 签名，尚无 Developer ID 签名或公证。
