SYSU NetAuth v0.7.0 正式版。

- 采用 Rust 认证核心；Windows 使用 Win32 原生界面，macOS 使用 AppKit 原生界面，不再依赖 Python、Qt 或 PySide6。
- Windows 使用系统自带的 NDISUIO 驱动完成有线认证，Npcap 后端及 EAPHost 验证方案退役，无需额外安装网络驱动。
- macOS 提供完整 PKG，包含原生界面、菜单栏、后台服务和授权 helper；修复菜单栏图标反复重绘造成的高 CPU 占用。
- 后台服务独立运行，统一恢复和重试策略；连接通知采用 5 秒防抖，短暂中断恢复时保持静默。
- 支持日志查看和诊断导出；日志过长时保留最新记录，遮盖当前账号和密码，不导出配置文件。Windows 安装和升级会修复配置及日志权限。
- 发行文件按平台和架构命名，移除 Rust、编译器和便携版标识。五个平台包与 SHA-256 校验文件齐全后统一发布。

Windows x64、ARM64 选择对应的 `windows_x64`、`windows_arm64` Setup；Apple Silicon 和 Intel Mac 分别选择 `macos_arm64`、`macos_x64` PKG。Linux x64 压缩包包含系统服务安装脚本。

Windows 已有设置在升级时保留。若同一网卡启用了 Windows 系统有线认证，请先停用有线自动配置服务，避免两个认证客户端同时占用网卡。旧 Python／Qt 客户端停止维护，建议使用本版。

各平台自动构建和安装包检查均为发布必需项。Windows 已完成校园网实连和恢复验证；Mac 已完成两种架构的离线、服务及安装升级测试，校园网实连和电源恢复仍需设备验证。macOS 应用采用 ad-hoc 签名，尚无 Developer ID 签名或公证。

[安装与使用文档](https://github.com/LoveElysia1314/sysu-netauth#readme)
