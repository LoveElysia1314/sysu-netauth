# SYSU NetAuth Python 版

Windows 有线校园网认证客户端，提供 GUI、CLI 和后台服务。需要 [Npcap](https://npcap.com/#download)（WinPcap API 兼容模式），程序提供安装引导。

## 安装与使用

从 [Releases](https://github.com/peterrobot/sysu-netauth/releases) 下载 `Python_x64` Setup 安装程序，安装后填写 NetID 和密码。

启用「服务独立运行」和自动认证后，退出 GUI 仍会保持认证。请先退出其他 802.1X 客户端。版本切换与故障排查见 [Windows 安装说明](../../docs/WINDOWS.md)。Python 版尚未包含 Rust 版的休眠恢复修复。

配置与日志位于 `%ProgramData%\SYSUNetAuth\`。连接异常时检查 `service.log`；服务无法启动时检查 `service_bootstrap.log`。

## 从源码运行

在仓库根目录生成版本：

```powershell
python -m pip install -r requirements-build.txt
python scripts/release_version.py --generate-python
```

进入应用目录，以下命令均在 `apps/python` 中执行：

```powershell
cd apps/python
python -m pip install -r requirements.lock
python run.py
```

也可运行本目录的 `start.bat`，通过 uv 管理 Python 环境。

## CLI

在管理员终端执行：

```powershell
python run.py --list-ifaces
python run.py -i "以太网 2" --probe-iface
python run.py -i "以太网 2" -u "NetID"
python run.py -i "以太网 2" --logoff
python run.py --check-npcap
```

省略 `-p/--password` 时会交互式输入密码。其他参数见 `python run.py --help`。

## 配置

配置文件：`%ProgramData%\SYSUNetAuth\config.json`，包含明文凭据，目录仅允许 SYSTEM 和管理员访问。

| 字段 | 默认值 | 含义 |
| --- | --- | --- |
| `username`, `password` | 空 | NetID 和密码 |
| `iface_mode` | `auto` | `auto` 自动选择，`manual` 固定网卡 |
| `iface` | 空 | 手动模式的网卡名 |
| `auto_auth` | `true` | 自动认证 |
| `retry_interval` | `60` | 重试间隔，15–3600 秒 |
| `service_mode` | `true` | 服务独立运行；关闭后退出 GUI 会停止服务 |
| `launch_gui_on_login` | `false` | 登录时启动 GUI |
| `hide_window_on_login` | `true` | 登录启动时隐藏窗口 |
| `desktop_notify` | `true` | 托盘通知 |

## 构建与测试

构建前按上述步骤生成版本。安装 Inno Setup 6 可生成 Setup 安装程序。

```powershell
python -m pip install -r requirements-dev.lock
python -m unittest discover -s tests -v
python -m pip install PyInstaller
python scripts/build.py --skip-installer
python scripts/build.py
```

EXE 输出：`dist/sysu_netauth/`。完整构建的安装包输出到本目录；统一发行打包见 [发行说明](../../docs/RELEASING.md)。

[架构说明](docs/ARCHITECTURE.md)
