# Python 架构

认证由 `SYSUNetAuth` Windows 服务执行，GUI 管理设置与状态。启用服务独立运行后，退出 GUI 不影响认证。

| 模块 | 职责 |
| --- | --- |
| `runner.py`、`cli.py` | GUI / CLI / 服务入口分发、GUI 单例 |
| `core/` | 配置、EAP-MD5、网卡枚举、Npcap、更新检查 |
| `service/engine.py` | 认证状态机、续期、重试、网卡检测 |
| `service/win_service.py` | pywin32 服务宿主 |
| `app/` | PySide6 设置窗口、托盘、登录任务 |

`core/` 和 `service/` 不依赖 PySide6。打包分别生成 GUI/CLI 与服务 EXE。

## 服务运行

状态为 `idle`、`authenticating`、`authenticated`、`failed`、`stopped`。主循环每秒处理命令和重试，周期任务包括：

| 间隔 | 任务 |
| --- | --- |
| 3 秒 | 重载配置 |
| 5 秒 | 网卡状态检测、状态心跳 |
| 60 秒 | 检查更新调度 |

认证使用 GBK 编码的 EAP-MD5。通过互联网连通性验证后启动 `RenewListener`，响应交换机重认证。启动前 60 秒按 3/5/8/13/21 秒退避，之后使用配置的重试间隔，最多五次；无可用网卡时等待硬件检测。

服务未处理 Windows 电源事件；休眠前后网卡状态相同时可能漏掉重认证。诊断与 Rust 修复见 [休眠恢复](../../rust/docs/RESUME.md)。

## 共享存储

目录：`%ProgramData%\SYSUNetAuth\`，仅 SYSTEM 与管理员可访问。文件通过临时文件和原子替换写入。

| 文件 | 写入方 | 用途 |
| --- | --- | --- |
| `config.json` | GUI | 凭据和用户设置 |
| `service_cache.json` | 服务 | 网卡选择缓存 |
| `status.json` | 服务 | 认证状态；GUI 每 2 秒读取 |
| `update_state.json` | 服务 | 更新结果与调度 |
| `ui_state.json` | GUI | 更新通知与忽略状态 |
| `commands/*.json` | GUI | 按文件名顺序处理的命令队列 |
| `command.json` | 外部工具 | 兼容命令入口 |

命令支持 `authenticate`、`logoff`、`reload_config`、`check_update`，例如：

```json
{"action":"authenticate"}
```

配置字段、构建与启动命令见 [README](../README.md)。
