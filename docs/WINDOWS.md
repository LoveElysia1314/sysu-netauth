# Windows 安装与维护

支持 Windows 10/11 x64，安装和管理后台服务需要管理员权限。

## 安装

从 [Releases](https://github.com/peterrobot/sysu-netauth/releases) 下载 Setup EXE。Rust 版一般选择 `Rust_msvc_x64`；`Rust_gnu_x64` 提供相同功能。`Python_x64` 为原 Python 版。

1. 运行 Setup，按提示安装 [Npcap](https://npcap.com/#download)。Python 版需开启 Npcap 的 WinPcap API 兼容模式。
2. 打开 SYSU NetAuth，填写 NetID 和密码，选择有线网卡并认证。
3. 启用自动认证和「服务独立运行」，即可退出 GUI 并由服务保持认证。

请先退出其他 802.1X 客户端。Rust 版支持睡眠和休眠恢复后自动重认证。

## 版本切换

Rust 和 Python 共用 `%ProgramData%\SYSUNetAuth` 配置与 `SYSUNetAuth` 服务，每次只能使用一种版本的 GUI 和服务。切换前备份配置目录，其中包含账号和密码。

Python → Rust 首次迁移时，原配置会备份为 `config.json.python-backup`。返回 Python 时，如需恢复此备份，应先停止服务，再将其复制为 `config.json`；备份可能不包含迁移后修改的密码或设置。

## 故障排查

- **无法认证：** 检查 Npcap、有线网卡、NetID 与密码，并退出其他认证客户端。
- **服务未运行：** 在管理员 PowerShell 中执行 `sc.exe query SYSUNetAuth`；需要时执行 `sc.exe start SYSUNetAuth`。
- **连接中断：** 查看 `%ProgramData%\SYSUNetAuth\service.log`；Rust 版休眠问题另见 [恢复排查](../apps/rust/docs/RESUME.md)。
- **安装或卸载失败：** 在 `%TEMP%` 查找 `Setup Log` 或 `Uninstall Log`。也可从管理员终端运行 `Setup.exe /LOG="C:\path\setup.log"`，或安装目录中的 `unins000.exe /LOG="C:\path\uninstall.log"`，指定一个可写路径。

便携 ZIP 不包含安装程序，不会自动管理服务或升级；普通用户建议使用 Setup。
