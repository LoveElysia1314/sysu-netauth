# 程序架构

`netauth` 库供 CLI、后台服务和 Windows GUI 共用。后台服务独占认证会话；GUI 和 CLI 通过本地 IPC 读取状态、修改配置和发送操作请求。直接认证也获取同一实例锁，避免与后台服务同时操作网卡。

## 认证引擎

`Engine` 管理配置、网卡候选、认证状态和重试期限。主线程等待有界事件队列或定时期限，不以状态文件传递界面事件。每个选中的网卡在认证、联网验证和续期期间共用一个抓包会话。

| 平台 | 抓包与通知 |
| --- | --- |
| Windows | 动态加载 Npcap；使用 EAPOL 过滤器、`pcap_getevent` 和 IP Helper 网络通知 |
| Linux | 绑定 EtherType 的 AF_PACKET 套接字和 rtnetlink 通知 |
| macOS | BPF 抓包、SystemConfiguration 网络通知和 IOKit 电源通知 |

凭据使用 GBK 编码，协议为 EAP-MD5。引擎校验帧长、目的地址和认证端 MAC；配置变化、网卡切换及停止操作递增会话代次，使旧回调失效。

收到 EAP-Success 后，抓包继续运行，同时探测 `119.29.29.29` 和 `114.114.114.114`。探测沿系统路由发送，成功可能来自其他网络接口。验证失败后先尝试剩余候选网卡，再进入重试调度。

原生网络通知负责及时唤醒引擎，五秒一次的核对补足通知遗漏。状态快照包含活动网卡和完整网卡列表；网卡列表变化时也会推送快照，GUI 按设备 ID 保留用户尚未保存的选择。

Windows 服务通过 SCM 接收电源事件，唤醒后重新打开抓包并认证。具体流程见[睡眠恢复](RESUME.md)。

## 本地控制

| 平台 | 端点 | 访问权限 |
| --- | --- | --- |
| Windows | `\\.\pipe\SYSUNetAuth-v1` | 本机 SYSTEM / Administrators |
| Linux | `/run/sysu-netauth/control.sock` | root |
| macOS | `/var/run/sysu-netauth/control.sock` | root |

消息由四字节大端长度和 UTF-8 JSON 组成，上限为 65,536 字节。请求包含 `version: 1`、`id` 和 `action`；响应包含 `version`、`id`、`ok`、可选的 `error` 以及 `data`。

操作包括 `status`、`watch`、`interfaces`、`authenticate`、`logoff`、`get_config`、`set_config`、`reload_config` 和 `check_update`。每个连接处理一次请求，`watch` 在首次响应后持续推送快照。最多同时处理 16 个客户端，每个订阅者最多缓存八条快照；处理过慢的订阅者会断开。

## 正式版更新

服务并行读取 GitHub 和 Gitee 的更新清单，校验格式、正式版版本号、HTTPS 来源和发布链接，再选择版本较新的结果。已缓存的较新发布信息可抵御镜像暂时落后；启动时重新按当前程序版本计算更新状态，避免升级后继续显示旧提示。

更新检查独立于认证状态，下载线程不阻塞认证引擎。检查频率、手动操作冷却和失败退避见[更新规则](CLI.md#更新规则)。

## 存储与目录

配置、缓存和日志由服务管理。配置写入采用原子替换；普通读取只校验内容，不回写文件。Windows 数据目录仅 SYSTEM 和管理员可访问；Linux/macOS 私有目录使用 0700 权限，配置文件使用 0600 权限。

| 目录 | 内容 |
| --- | --- |
| `src/` | 认证引擎、平台实现、IPC、界面及命令入口 |
| `assets/`、`packaging/` | 图标、可执行文件清单和系统服务定义 |
| `scripts/` | 构建、安装、发布与测量工具 |
| `tests/` | Rust 网络/服务集成测试与发行工具测试 |
| `docs/` | 安装、开发及维护文档 |
