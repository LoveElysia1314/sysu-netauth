# macOS

macOS 11+，支持 Apple Silicon 与 Intel，提供原生界面、菜单栏、CLI 和 launchd 后台服务。需要有线以太网。

## 安装与配置

从 [Releases](https://github.com/LoveElysia1314/sysu-netauth/releases) 下载 `macos_arm64`（Apple Silicon）或 `macos_x64`（Intel）的 `.pkg`，完成安装后在“应用程序”中打开 SYSU NetAuth。安装程序将应用放入 `/Applications/SYSUNetAuth.app`，安装后台服务并保留已有配置。

### 首次打开时的安全提示

当前安装包尚无 Developer ID 签名或 Apple 公证。若首次打开时提示“Apple 无法验证是否包含可能危害 Mac 安全或泄露隐私的恶意软件”，确认文件来自本项目的 GitHub 或 Gitee 发布页后，可为该安装包单独授权：

1. 双击 `.pkg`，出现提示后关闭提示框。
2. 打开“系统设置 → 隐私与安全性”，找到该安装包的拦截提示，点击“仍要打开”。
3. 按提示确认并授权，继续安装。

安装后首次打开应用若出现同类提示，也可按上述步骤单独授权。操作依据见 [Apple 官方说明](https://support.apple.com/zh-cn/102445)。

### 配置与使用

账户和网卡信息编辑后，点击“认证”保存并发起认证；已认证且账户未变时，按钮显示“重新认证”。常规选项修改后立即保存。账户区的“退出程序”关闭界面，后台服务继续运行。

macOS 13+ 的独立通知助手在用户登录后无窗口运行；退出状态程序后，仍可接收已启用的认证通知和正式版更新提醒。系统通知权限和专注模式等设置会影响投递。macOS 11/12 保留界面内的单次授权操作，后台通知助手需要 macOS 13+。

“应用信息”提供检查更新、运行日志和更新日志三个按钮。“更新日志”优先展示未忽略的新版本说明，其余情况展示当前版本说明。主菜单和菜单栏的“软件更新”还可忽略指定版本。“常规设置”中的“自动检查更新”和“有新版本时通知”分别控制后台检查与提醒，修改后立即保存，无需重新填写账号密码。后台服务自动检查正式版，助手对每个新版本提醒一次；更新提醒独立于认证状态通知。首次需要通知时，系统会请求权限。

需要手动管理时可使用 CLI：

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

手动选卡时，将 `iface_mode` 设为 `manual`，并将 `sysu_netauth interfaces --json` 返回的网卡 `id` 原样填入 `iface`。例如 `"iface_mode":"manual","iface":"00:11:22:33:44:55"`，其中 MAC 地址需替换为实际设备 ID。其他命令见[命令行与配置](CLI.md)。

配置与日志位于 `/Library/Application Support/SYSUNetAuth/`，其中 `config.json` 包含凭据。连接问题查看 `service.log`，启动问题查看 `service_bootstrap.log`。请避免在同一网卡上同时运行其他 802.1X 客户端。

## 服务管理与卸载

查看服务：`sudo launchctl print system/io.github.peterrobot.sysu-netauth`。

在应用菜单中选择“卸载 SYSU NetAuth…”，确认并完成管理员授权。卸载会停止认证与通知，移除自动启动项、应用、后台服务和 helper。默认保留账户、设置和日志；勾选“同时删除账户、设置和日志”可一并清理。

终端中也可卸载：

```sh
sudo sh /Applications/SYSUNetAuth.app/Contents/Resources/uninstall-macos.sh --remove-app
```

加上 `--purge` 可删除账户、设置和日志。终端卸载前请退出界面，并关闭“开机后启动界面程序”。
