# 命令行与配置

Windows 使用管理员终端；Linux 和 macOS 的服务操作使用 `sudo`。以下命令省略平台权限前缀。

## 服务操作

```sh
sysu_netauth interfaces
sysu_netauth status --json
sysu_netauth watch --json
sysu_netauth preferences --json
sysu_netauth diagnostics --json
sysu_netauth authenticate
sysu_netauth logoff
sysu_netauth config
sysu_netauth config password
sysu_netauth config import /path/to/config.json
sysu_netauth check-update
sysu_netauth update-preferences --auto-check true --notify true
```

`interfaces` 列出网卡，`status` 读取当前状态，`watch` 持续输出状态变化。`authenticate` 和 `logoff` 请求服务连接或断开。`config` 显示配置并隐藏密码，`config password` 交互式修改密码，`config import` 导入 JSON 设置。

`check-update` 通过当前可用网络检查正式版更新。输出中的 `checking` 表示检查已启动，结果可从 `status --json` 或 `watch --json` 中读取。

`update-preferences` 单独保存两个更新开关，保留现有账号和认证设置。

`preferences` 返回不含账号和密码的界面相关设置；`diagnostics` 返回版本、状态和有长度限制的最近服务日志，遮盖当前账号与密码，不导出配置。分享日志前仍应检查其他个人信息。

所有命令支持 `--json`。服务操作需要后台服务正在运行。

## 直接操作网卡

直接操作前应停止后台服务，然后指定网卡探测、认证或注销：

```sh
sysu_netauth probe --iface eth0
sysu_netauth authenticate --iface eth0 --username NetID --timeout 90
sysu_netauth logoff --iface eth0
```

省略密码参数时会交互式输入。直接认证完成后程序退出；持续响应交换机续期请求需要后台服务。

## 配置字段

```json
{"username":"NetID","password":"密码","iface_mode":"auto","auto_auth":true}
```

| 字段 | 默认值 | 含义 |
| --- | --- | --- |
| `username`, `password` | 空 | NetID 和密码，需可编码为 GBK |
| `iface_mode` | `auto` | `auto` 自动选择，`manual` 固定网卡 |
| `iface` | 空 | `interfaces --json` 返回的设备 ID；Windows 为系统设备 ID，Linux/macOS 为 MAC 地址 |
| `auto_auth` | `true` | 自动认证及失败重试 |
| `retry_interval` | `60` | 失败重试间隔的上限，范围为 15–3600 秒 |
| `service_mode` | `true` | Windows 后台服务随系统启动并独立运行 |
| `launch_gui_on_login` | `false` | Windows 开机后进入桌面时启动界面程序 |
| `hide_window_on_login` | `true` | 登录后自动启动时隐藏窗口，手动打开仍显示 |
| `desktop_notify` | `false` | Windows 联网状态变化通知，由独立通知助手发送 |
| `auto_update_check` | `true` | 后台定时检查正式版更新；关闭后仍可手动检查 |
| `update_notify` | `true` | 有新版本时发送通知，与联网状态通知分别设置 |

未指定的字段使用默认值。设置包含明文凭据，应限制文件访问权限。

自动认证失败后，按 3、6、12、24 秒逐步延长重试间隔，达到 `retry_interval` 后按该间隔继续尝试。凭据不完整等配置问题等待设置修正，未连接的网卡等待网络事件。手动断开在当前服务运行期间保持，插拔和唤醒不改变此选择；重新启动服务时按自动认证设置执行。

## 更新规则

`connect <配置文件>` 会保存该配置并立即发起认证。配置文件应保持私有权限。

程序在发现较新的正式版时提示更新。例如，使用 v0.6.3 时可收到 v0.7.0 的更新提醒。

更新检查同时请求 GitHub 和 Gitee，采用可用来源中版本较新的正式版信息。任一来源不可用时，仍可使用另一来源；落后的镜像不会覆盖已发现的较新版本。

成功检查后约一天再次检查。失败重试间隔依次为 30 分钟、2 小时、6 小时和一天；手动检查间隔至少一分钟。检查结果写入服务缓存并绑定客户端版本。升级或调整版本后重新检查，避免继续使用旧版本的更新提示。
