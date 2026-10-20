# SYSU NetAuth

中山大学有线校园网 802.1X / EAP-MD5 自动认证客户端。本仓库同时保留原 Python 桌面版和 Rust 原生版，分别维护源码、测试、资源、构建脚本和使用文档。

| 版本 | 平台 | 实现与特点 | 文档 |
| --- | --- | --- | --- |
| 原生版，v0.7.0 开发中 | Windows 10/11 x64、Linux x64 | Rust；原生 Win32 GUI / CLI / 后台服务；低运行开销；Windows 休眠恢复自动重认证 | [原生版说明](apps/native/README.md) |
| Python 版，v0.6.2 | Windows | Python、PySide6、Scapy、pywin32；保留原 GUI、CLI、服务和 PyInstaller 构建流程 | [Python 版说明](apps/python/README.md) |

两个版本均保留 GBK/EAP-MD5 支持，Windows 均需安装 Npcap。Linux 原生版使用内核原始套接字，无需 Python、Qt 或 Npcap。

## 仓库结构

```text
apps/
  native/         Rust crate、Win32 GUI、CLI、服务、资源、脚本和文档
  python/         原 Python 应用、依赖锁定、GUI、服务、测试和打包脚本
.cargo/           Rust 目标配置（Windows 静态 CRT）
Cargo.toml        Rust workspace 和统一 release 优化配置
Cargo.lock        Rust 依赖锁定
docs/             两个版本的关系和切换说明
updates/          已发布版本使用的兼容更新清单 URL
```

以下原生版命令从仓库根目录执行。Rust 中间产物仍在 `target/`，两个应用的发行产物分别在 `apps/native/dist/` 和 `apps/python/dist/`，不会覆盖。

## 原生版

```sh
cargo test --locked --workspace
sh apps/native/scripts/build.sh
```

Windows 开发者 PowerShell：

```powershell
./apps/native/scripts/build.ps1 -SkipInstaller
./start.bat
```

在 Linux 交叉编译 Windows MSVC 发行版：

```sh
# 首次安装所需的 LLVM、Windows Rust target 和 cargo-xwin，详见原生版说明。
sh apps/native/scripts/build-windows.sh
```

交叉编译保留静态 CRT、LTO、`opt-level=3` 和单代码生成单元；构建工具不随程序分发。产物：`apps/native/dist/sysu-netauth-windows-x64.zip`。

## Python 版

Python 目录保留自 Rust 重写前的提交 `bc1f8cb`，运行时代码和依赖锁定保持原样。先进入应用目录，再执行其原有命令：

```powershell
cd apps/python
python -m pip install -r requirements.lock
python run.py
# 或使用 uv 和固定 Python 版本：
./start.bat
```

```powershell
python -m pip install -r requirements-dev.lock
python -m unittest discover -s tests -v
python -m pip install PyInstaller
python scripts/build.py --skip-installer
```

Python 版仍使用原有状态/命令文件接口；原生版使用本地 IPC，CLI 参数也不同。Windows 休眠自动恢复修复目前位于原生版，尚未回移到保留的 Python 版。

## 版本切换

两个 Windows 版本沿用同一个 `SYSUNetAuth` 服务、安装器 AppId、登录任务和 `%ProgramData%\SYSUNetAuth` 设置目录。它们是可替换的实现，不能同时注册运行，也不能混用两个版本的 GUI 和服务。

切换前备份设置目录并停止当前服务和 GUI，再安装或注册所选版本。原生版首次读取 Python 配置会保留 `config.json.python-backup`。回退 Python 前请参考 [版本与切换说明](docs/ARCHITECTURE.md)，特别是凭据备份和两个控制接口的区别。

原生版和 Python 版各有 CI 构建、测试及独立命名的产物。Windows 实机安装、Npcap 和校园网验证仍需在对应环境完成。
