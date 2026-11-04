SYSU NetAuth v0.7.0-beta.3 测试版。

- Windows 正式切换到系统自带 NDISUIO 原生认证，不再依赖 Npcap，也不包含旧认证后端或 EAPHost 验证分支。
- macOS 原生界面随 PKG 安装包提供，包含菜单栏、后台服务和授权 helper；修复菜单栏图标反复重绘造成的高 CPU 占用。
- 统一两端的界面启动、隐藏窗口和通知选项；后台服务独立运行，退出界面不停止认证。
- 增加日志查看与诊断导出，遮盖当前账号和密码，不导出配置文件。
- Windows 仅提供 x64、ARM64 安装程序；macOS 提供 Apple Silicon、Intel 安装包；Linux 提供系统服务安装包。移除便携版及重复的 Windows GNU 发行资源，各平台均附带 SHA-256 校验清单。

Windows 用户选择 `Rust_msvc_x64` 或 `Rust_msvc_arm64` 的 Setup。若此前启用了 Windows 系统有线认证，需要先停用有线自动配置，避免占用同一网卡；普通升级会保留 SYSU NetAuth 的账号和设置。

macOS 应用使用 ad-hoc 签名，安装包尚无 Developer ID 签名或公证。校园网实连、物理扩展坞拔插和睡眠恢复仍需在 macOS 实机继续验证。此版本保持 beta，程序内更新检查仍只提示正式版。

[安装与使用文档](https://github.com/LoveElysia1314/sysu-netauth#readme)
