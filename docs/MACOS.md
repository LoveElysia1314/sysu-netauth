# macOS

macOS 11+，支持 Apple Silicon 与 Intel，提供原生界面、菜单栏、CLI 和 launchd 后台服务。需要有线以太网。

## 安装与配置

从 [Releases](https://github.com/LoveElysia1314/sysu-netauth/releases) 下载 `macos_arm64`（Apple Silicon）或 `macos_x64`（Intel）的 `.pkg`，完成安装后在“应用程序”中打开 SYSU NetAuth。安装程序将应用放入 `/Applications/SYSUNetAuth.app`，安装后台服务并保留已有配置。

填写账号后点击“重新连接”。后台服务独立运行，退出界面不会断开认证。需要手动管理时可使用 CLI：

```sh
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

手动选卡可设置 `"iface_mode":"manual","iface":"00:11:22:33:44:55"`。这里的 MAC 地址仅为示例，请先运行 `sysu_netauth interfaces --json`，将所选网卡的 `id` 原样填入 `iface`，不要填写 `en5` 等网卡名称。其他命令见 [命令行与配置](CLI.md)。

配置与日志位于 `/Library/Application Support/SYSUNetAuth/`，其中 `config.json` 包含凭据。连接问题查看 `service.log`，启动问题查看 `service_bootstrap.log`。请避免在同一网卡上同时运行其他 802.1X 客户端。

## 管理与构建

原生界面的构建、签名与离线测试见[开发说明](../macos/README.md)。当前应用采用 ad-hoc 签名，尚无 Developer ID 签名或公证。

查看服务：`sudo launchctl print system/io.github.peterrobot.sysu-netauth`。

卸载后台组件：`sudo sh /Applications/SYSUNetAuth.app/Contents/Resources/uninstall-macos.sh`；加上 `--purge` 可同时删除保存的账号和设置。随后退出界面并将应用移到废纸篓。

从源码构建需要 Rust 与 Xcode Command Line Tools，在仓库根目录执行：

```sh
sh scripts/build-macos.sh
```

构建需要匹配目标架构的 macOS 主机。输出为 `dist/sysu-netauth-macos-arm64.pkg` 或 `dist/sysu-netauth-macos-x64.pkg`；同目录的元数据文件用于发布校验，不作为面向用户的下载资源。
