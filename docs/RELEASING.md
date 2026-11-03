# 发行维护

版本统一维护在 `Cargo.toml`，修改后同步更新 `Cargo.lock`。发行标签在版本前加 `v`，例如 `v0.7.0` 或 `v0.7.0-beta.2`；允许 `alpha.N`、`beta.N`、`rc.N` 预发布后缀。三个主版本分量均不得超过 Windows 版本字段的上限 65535。

## 发布流程

1. 提交代码、文档和版本号变更，合入 `main`。
2. 在合并后的提交上创建并推送标签。
3. 等待 **Release packages** 完成必需平台的构建、测试和安装包校验。
4. 检查草稿 Release 的文件、校验值及说明后发布。预发布标签始终保留 prerelease 标记。

Windows x64、Windows ARM64 和 Linux x64 为必需平台。macOS 为实验性支持，成功的程序包独立上传。工作流可在标签上手动重跑，重跑时替换同名附件，保留已有发布说明和公开状态。

测试版只出现在 Releases 中，不写入正式版更新清单。正式版公开后，再更新 `updates/release.json` 中的版本、发布时间、链接和摘要；`channel` 保持 `stable`。发布链接必须已可公开访问，清单中的版本应为不带预发布后缀的三段数字。

## 发行文件

| 平台 | 安装包 | 校验文件 |
| --- | --- | --- |
| Windows MSVC x64 | `SYSUNetAuth_Rust_msvc_x64_Setup_v<version>.exe` | `SHA256SUMS-rust-msvc-x64.txt` |
| Windows MSVC ARM64 | `SYSUNetAuth_Rust_msvc_arm64_Setup_v<version>.exe` | `SHA256SUMS-rust-msvc-arm64.txt` |
| Windows GNU x64 | `SYSUNetAuth_Rust_gnu_x64_Setup_v<version>.exe` | `SHA256SUMS-rust-gnu-x64.txt` |

Windows 同时提供包含 CLI、GUI 和服务三个 EXE 的便携 ZIP。Linux x64 压缩包包含 CLI、服务、systemd 单元和安装脚本；macOS 的 Apple Silicon、Intel 压缩包包含 CLI、服务及 launchd 安装/卸载脚本。每个平台附带 SHA-256 校验文件。

## 本地打包

使用干净的版本标签、Python 3.11+ 和 Inno Setup 6。在仓库根目录执行：

```powershell
python scripts/release_version.py --write
./scripts/build.ps1 -SkipInstaller
Copy-Item build/release-version.json dist/sysu_netauth/
python scripts/package_windows.py --input-dir dist/sysu_netauth
```

ARM64 构建使用 `-Target aarch64-pc-windows-msvc`，将元数据复制到 `dist/arm64/sysu_netauth/` 后，打包时传入 `--arch arm64 --input-dir dist/arm64/sysu_netauth`。GNU 构建使用 `scripts/build-windows-gnu.sh`，元数据放入 `dist/gnu/sysu_netauth/`，打包时传入 `--abi gnu --input-dir dist/gnu/sysu_netauth`。

输出位于 `dist/windows-installers/`，也可用 `--output-dir` 和 `--compiler` 指定目录或 Inno Setup 编译器。

开发构建与发行构建均显示包版本。发行工具还会校验标签与包版本一致，且标签准确指向当前提交。版本说明统一维护在 [GitHub Releases](https://github.com/LoveElysia1314/sysu-netauth/releases)。
