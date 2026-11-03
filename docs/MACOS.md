# macOS

macOS 11+，支持 Apple Silicon 与 Intel，提供 CLI 和 launchd 后台服务。需要有线以太网，无需 Npcap。当前为实验性支持，校园网认证和睡眠恢复仍需实机验证。

## 安装与配置

从 [Releases](https://github.com/LoveElysia1314/sysu-netauth/releases) 下载 `macos_arm64`（Apple Silicon）或 `macos_x64`（Intel）压缩包。Apple Silicon 示例：

```sh
tar -xzf SYSUNetAuth_Rust_macos_arm64_v*.tar.gz
cd sysu-netauth-macos-arm64
sudo sh install-macos.sh
sysu_netauth interfaces
```

将账号写入 `config.json`：

```json
{"username":"NetID","password":"密码","iface_mode":"auto","auto_auth":true}
```

```sh
chmod 600 config.json
sudo sysu_netauth config import config.json
sudo sysu_netauth status --json
sudo sysu_netauth watch --json
```

手动选卡可设置 `"iface_mode":"manual","iface":"00:11:22:33:44:55"`。这里的 MAC 地址仅为示例，请先运行 `sysu_netauth interfaces --json`，将所选网卡的 `id` 原样填入 `iface`，不要填写 `en5` 等网卡名称。旧配置若使用名称，需要重新选择网卡并保存。其他命令见 [命令行与配置](CLI.md)。

配置与日志位于 `/Library/Application Support/SYSUNetAuth/`，其中 `config.json` 包含凭据。连接问题查看 `service.log`，启动问题查看 `service_bootstrap.log`。请避免在同一网卡上同时运行其他 802.1X 客户端。

## 管理与构建

macOS 原生 GUI 已提供[开发预览及离线测试说明](../macos/README.md)，尚未随正式发行包分发；包含包内 CLI，并通过系统管理员授权控制后台服务；服务安装及真实认证尚待实机验证。

查看服务：`sudo launchctl print system/io.github.peterrobot.sysu-netauth`。

在解压目录卸载：`sudo sh uninstall-macos.sh`；同时删除保存的账号和设置：`sudo sh uninstall-macos.sh --purge`。

从源码构建需要 Rust 与 Xcode Command Line Tools，在仓库根目录执行：

```sh
sh scripts/build-macos.sh
```

输出位于 `dist/sysu-netauth-macos-arm64/` 或 `sysu-netauth-macos-x64/`，并生成对应的 `.tar.gz`。
