# 发行维护

版本统一维护在 `Cargo.toml`，修改后同步更新 `Cargo.lock`。发行标签在版本前加 `v`，例如 `v0.7.0`。三个主版本分量均不得超过 Windows 版本字段上限 65535。

## 发布流程

1. 完成代码、文档和版本号变更，在干净的提交上创建并推送标签。
2. 等待 **Release packages** 完成全部平台的构建、测试和安装包校验。
3. 检查草稿 Release 的十个附件、校验值及说明后发布。正式版取消 prerelease 标记，并设为最新正式版。
4. 确认发布页可公开访问后，将更新清单同步到 `main`，再同步镜像。

Windows x64、Windows ARM64、Linux x64、macOS Apple Silicon 和 macOS Intel 均为必需平台。缺少任何一个安装包都会阻止创建发行草稿。工作流可以在同一标签上重跑，替换同名附件，并保留已有发布说明和公开状态。

`updates/release.json` 记录正式版的版本、发布时间、发布链接和摘要，`channel` 保持 `stable`。更新清单中的版本必须是不带预发布后缀的三段数字；测试版不写入该清单。

## 发行文件

| 平台 | 安装包 | 校验文件 |
| --- | --- | --- |
| Windows x64 | `SYSUNetAuth_windows_x64_Setup_v<version>.exe` | `SHA256SUMS-windows-x64.txt` |
| Windows ARM64 | `SYSUNetAuth_windows_arm64_Setup_v<version>.exe` | `SHA256SUMS-windows-arm64.txt` |
| Linux x64 | `SYSUNetAuth_linux_x64_v<version>.tar.gz` | `SHA256SUMS-linux-x64.txt` |
| macOS Apple Silicon | `SYSUNetAuth_macos_arm64_v<version>.pkg` | `SHA256SUMS-macos-arm64.txt` |
| macOS Intel | `SYSUNetAuth_macos_x64_v<version>.pkg` | `SHA256SUMS-macos-x64.txt` |

Windows 安装程序包含界面、CLI、后台服务和通知助手。macOS PKG 包含完整应用、CLI、服务、授权 helper 和通知助手的 LaunchAgent。Linux 压缩包包含 CLI、服务、systemd 单元和需要 root 执行的安装脚本。

## 本地打包

使用干净的版本标签、Python 3.11+ 和 Inno Setup 6。在仓库根目录执行：

```powershell
python scripts/release_version.py --write
./scripts/build.ps1 -SkipInstaller
Copy-Item build/release-version.json dist/sysu_netauth/
python scripts/package_windows.py --input-dir dist/sysu_netauth
```

ARM64 构建使用 `-Target aarch64-pc-windows-msvc`，将元数据复制到 `dist/arm64/sysu_netauth/` 后，打包时传入 `--arch arm64 --input-dir dist/arm64/sysu_netauth`。输出位于 `dist/windows-installers/`。

macOS 使用 `sh scripts/build-macos.sh`。PKG 与 `.pkg.metadata.json` 在同一构建中生成，后者包含提交、版本、目标架构和安装包摘要；只对外发布 PKG 和校验清单。Apple Silicon 与 Intel 分别完成构建和安装、升级、卸载测试。

发行工具校验标签与包版本一致、标签准确指向当前提交，并检查各平台元数据和摘要。安装包发布于 [GitHub Releases](https://github.com/LoveElysia1314/sysu-netauth/releases)。

正式版更新内容统一维护在根目录 [CHANGELOG.md](../CHANGELOG.md)。每版使用独立的稳定版本号标题，发布流程通过 `scripts/release_notes.py` 提取对应章节作为 Release 正文；缺少章节、重复版本或混入测试版条目都会阻止发布。客户端更新提醒中的“查看新版说明”打开该版本的发布页。
