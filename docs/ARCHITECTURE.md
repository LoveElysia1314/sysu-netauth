# 架构说明

本文面向维护者。用户文档见 [README](../README.md)。

Python 包名 `sysu_netauth`，用户可见名称 `SYSU NetAuth`。

---

## 一、双进程架构

```
系统启动
  └─ SYSUNetAuth Windows 服务 (Session 0) — 认证执行者
       ├─ 读取 config.json
       ├─ 自动选择有线网卡
       ├─ 执行 EAPOL-MD5 认证
       ├─ 被动监听交换机重认证 (RenewListener)
       ├─ 监听网卡插拔/重命名/故障转移
       └─ 写入 status.json

用户登录
  └─ sysu_netauth.exe GUI (Session 1) — 配置面板和状态监视器
       ├─ 编辑共享配置 → config.json
       ├─ 轮询 status.json 展示服务状态
       ├─ 写 command.json 触发命令（authenticate/logoff/reload_config）
       └─ 引导 Npcap 安装
```

**核心原则**：服务始终是认证的唯一执行者。GUI 不再实现认证状态机、
网卡监听、续期嗅探等逻辑，退化为纯配置面板和状态监视器。退出 GUI 不
影响后台认证。

### 1.1 模块边界

| 模块       | 可导入                       | 不可导入                        |
| ---------- | ---------------------------- | ------------------------------- |
| `core/`    | Python 标准库、scapy、psutil | PySide6、`app.*`                |
| `service/` | `core.*`、pywin32            | PySide6、`app.*`                |
| `app/`     | `core.*`、PySide6            | `service.*`（通过共享文件通信） |

---

## 二、入口分发

```
run.py ──→ runner.py ──┬─ 无 CLI 参数 ──→ app/tray.py（GUI 配置面板）
                        ├─ --startup    ──→ 处理服务模式 + 启动 GUI 或静默
                        ├─ --service    ──→ service/win_service.py（Windows 服务）
                        └─ 有 CLI 参数 ──→ cli.py（命令行模式）
```

- **runner.py**：单例保护（`QLocalServer` IPC）、`AttachConsole` 挂接父进程终端、分发入口
- GUI 模式：`--startup` 标记时以用户登录后自启模式运行（若 `hide_window_on_login=true` 则隐藏主窗口）
- 服务模式：`--service` 转交 pywin32 服务命令；打包后推荐使用 `sysu_netauth_service.exe`（不含 PySide6）
- CLI 模式：所有非 `--startup` 非 `--service` 参数转交 `cli.py` 的 `argparse` 解析

### 2.1 单例保护（GUI 模式）

使用 `QLocalServer` / `QLocalSocket`（命名管道 IPC），见
`core/single_instance.py`。

```
第二次启动
  ├─ QApplication(sys.argv)
  ├─ SingleInstanceManager.notify_existing()
  │    ├─ 管道存在 → 发送 "activate" → sys.exit(0)
  │    └─ 管道不存在 → start_server() → 成为主实例
  │
主实例
  ├─ single.activate_requested → tray.show_status()  ← 自己恢复窗口
  └─ 进程退出 → 管道自动释放
```

相比 Mutex 方案的优势：

1. 进程崩溃后无残留锁
2. 不需要从外部猜 HWND，而是让主实例自己恢复窗口
3. 与 Qt 事件循环天然集成，无竞态

仅在 GUI 模式启用；CLI 模式允许多开。

---

## 三、模块结构

```
sysu_netauth/
├── runner.py           # 入口分发：QLocalServer 单例保护、AttachConsole、CLI/GUI 路由
├── cli.py              # argparse CLI：认证/探测/注销/网卡列表/检查 Npcap
│
├── core/               # ── 无 GUI 核心库 ──
│   ├── config.py       # AppConfig 数据类、ProgramData JSON 读写、旧配置迁移、GUI 自启快捷方式
│   ├── shared_store.py # 服务/GUI 共享状态 (status.json) 与命令文件 (command.json)
│   ├── single_instance.py  # QLocalServer/QLocalSocket 单实例 IPC
│   ├── eapol.py        # EAPOL/MD5 协议栈：帧构造、parse、认证握手、注销
│   ├── interfaces.py   # 网卡枚举、类型判定（虚拟/有线/无线/回环）、评分排序、EAPOL 探测
│   └── npcap.py        # Npcap 检测、下载、完整性校验、提权安装
│
├── service/           # ── Windows 服务 —— 不含 Qt ──
│   ├── engine.py       # 无 Qt 认证状态机：认证、续期、网卡监听、命令处理、重试
│   └── win_service.py  # pywin32 Windows 服务宿主（SYSUNetAuthService）
│
└── app/               # ── GUI 配置面板 —— 不含认证逻辑 ──
    ├── tray.py         # CampusTray：托盘、服务状态轮询、配置编辑、Npcap 引导
    ├── views.py        # GUI 组件：MainWindow、_NetworkTable、CloseBehaviorDialog
    └── workers.py      # QThread：NpcapDownloadWorker（仅保留下载安装 Worker）
```

### 行数参考

| 模块                      | 行数 | 职责                                                   |
| ------------------------- | ---- | ------------------------------------------------------ |
| `core/config.py`          | ~230 | 配置模型、JSON 持久化、Startup 快捷方式自启            |
| `core/eapol.py`           | ~315 | 802.1X/EAPOL-MD5 协议栈（authenticate, send_logoff）   |
| `core/interfaces.py`      | ~330 | 网卡发现、类型判定、候选排序、EAPOL 探测               |
| `core/npcap.py`           | ~280 | Npcap 生命周期管理（检测、下载、完整性校验、提权安装） |
| `core/shared_store.py`    | ~100 | 进程间共享文件（config/status/command JSON）           |
| `core/single_instance.py` | ~90  | QLocalServer/QLocalSocket 单实例 IPC                   |
| `service/engine.py`       | ~450 | 无 Qt 认证状态机、服务主循环                           |
| `service/win_service.py`  | ~150 | pywin32 服务宿主、日志轮转、命令行处理                 |
| `app/tray.py`             | ~490 | CampusTray：服务状态轮询、GUI 控制器                   |
| `app/views.py`            | ~550 | GUI 组件：主窗口、网络信息面板、关闭行为对话框         |
| `app/workers.py`          | ~50  | QThread：仅 NpcapDownloadWorker                        |

---

## 四、进程间通信（Shared Store）

三个 JSON 文件位于 `%ProgramData%\SYSUNetAuth\`，所有写入均使用
**临时文件 + 原子替换**（先写 `.tmp` 再 `replace`），避免读到半写入文件。

| 文件           | 写入者                 | 读取者     | 用途                   |
| -------------- | ---------------------- | ---------- | ---------------------- |
| `config.json`  | GUI / 服务缓存成功网卡 | 服务 / GUI | 账号、密码、网卡、策略 |
| `status.json`  | 服务                   | GUI / CLI  | 当前认证状态           |
| `command.json` | GUI                    | 服务       | 手动认证、注销、重载   |

### 4.1 config.json

`core/config.py` 中的 `AppConfig` 数据类管理所有配置字段。
旧版 `%APPDATA%\SYSUNetAuth\config.json` 会在首次运行时自动迁移到
`%ProgramData%`（不删除旧文件，降低回滚风险）。

### 4.2 status.json

```
{
  "state": "authenticated",
  "message": "EAP success",
  "iface": "以太网",
  "mac": "00:11:22:33:44:55",
  "ipv4": "10.0.0.2",
  "updated_at": "2026-07-05T12:00:00+08:00",
  "authenticated_at": "2026-07-05T12:00:00+08:00",
  "expires_at": null
}
```

状态值：`starting` → `idle` / `waiting_network` / `authenticating` / `authenticated` / `failed` / `stopped`

### 4.3 command.json

```json
{ "action": "authenticate", "created_at": "2026-07-05T12:00:00+08:00" }
```

支持的命令：`authenticate`、`logoff`、`reload_config`。服务读取后删除文件。

---

## 五、双状态机设计

系统有两个独立的状态机：**服务端 `ServiceState`**（认证执行者）和
**GUI 端 `SessionState`**（UI 展示层）。

### 5.1 服务端状态机（`service/engine.py`）

七态 `ServiceState` 由 `AuthServiceEngine` 在独立线程中维护：

```
STARTING ──→ IDLE ──→ WAITING_NETWORK ──→ AUTHENTICATING ──→ AUTHENTICATED
                ↑                                                   │
                └──────────────────── FAILED ←──────────────←───────┘
                                                           (STOPPED)
```

| 状态              | 含义                                             |
| ----------------- | ------------------------------------------------ |
| `STARTING`        | 服务启动中，检查 Npcap                           |
| `IDLE`            | 待命，`auto_auth=false` 或无网卡                 |
| `WAITING_NETWORK` | 无可用有线网卡或候选耗尽等待重试                 |
| `AUTHENTICATING`  | EAPOL 握手进行中                                 |
| `AUTHENTICATED`   | 认证成功，续期监听中                             |
| `FAILED`          | 不可恢复错误（Npcap 未安装、凭据缺失、多次失败） |
| `STOPPED`         | 服务停止                                         |

### 5.2 GUI 端状态机（`app/tray.py`）

`SessionState` 五态仅用于 UI 展示，通过轮询 `status.json` 驱动：

```
IDLE ──→ WAITING_NETWORK ──→ AUTHENTICATING ──→ AUTHENTICATED
  ↑                                                 │
  └────────────────── LOGGING_OFF ←─────────────────┘
```

| GUI 表现     | 对应状态                       |
| ------------ | ------------------------------ |
| 灰色待命     | `IDLE` / `STOPPED`             |
| 黄色提示     | `WAITING_NETWORK` / `STARTING` |
| 黄色"认证中" | `AUTHENTICATING`               |
| 绿色"已认证" | `AUTHENTICATED`                |
| 红色错误     | `FAILED`                       |

---

## 六、状态心跳机制

服务处于 `AUTHENTICATED` 稳定状态时，每 5 秒的 `_check_iface_status()`
周期会调用 `_set_authenticated_status()` 刷新 `status.json` 的 `updated_at`
字段（心跳），确保 GUI 端不会因 15 秒无更新而误判"服务无响应"。

续期失败时服务静默重试，不改变状态、不发送 EAPOL-Logoff、不中断当前网络会话。

---

## 七、服务主循环（`service/engine.py`）

`AuthServiceEngine.run()` 使用 `threading.Event.wait(1)` 实现 1 秒 tick：

```
while not stop_event.is_set():
    1. _handle_command()       — 处理 command.json
    2. reload_config()          — 每 3 秒重载 config.json
    3. _check_iface_status()    — 每 5 秒检查网卡
    4. _do_retry()              — 重试到期执行
    5. _reauth_event 检测       — RenewListener 触发续期
```

**关键规则**：认证线程和续期监听线程不能同时嗅探同一网卡。开始认证前
必须停止续期监听，认证成功后再启动新的续期监听。

---

## 八、认证流程

### 8.1 候选网卡构建

```
_auth_candidates()
    │
    ├── manual 模式 → _resolve_saved_iface()
    │                  1. 配置的网卡名
    │                  2. 按 last_success_mac 找回
    │
    └── auto 模式 → _auto_auth_candidates()
                     1. last_success_mac 对应的网卡
                     2. 配置的 iface
                     3. 其余已连接有线网卡（按评分）
```

空列表 → 不重试，等待 5 秒硬件监听触发。

### 8.2 协议握手

`core/eapol.authenticate()` 实现标准 EAP-MD5：

```
EAPOL-Start ──广播──→ PAE 组播 MAC (01:80:c2:00:00:03)
                             │
                    ←─ EAP-Request/Identity ──
                             │
        EAP-Response/Identity (NetID, GBK 编码) ──→
                             │
                    ←─ EAP-Request/MD5-Challenge ──
                             │
        EAP-Response/MD5 (MD5(id|GBK(password)|challenge)) ──→
                             │
                    ←─ EAP-Success / EAP-Failure / 超时
```

关键细节：

- **密码编码**：GBK（`password.encode("gbk")`）
- **MD5 计算**：`hashlib.md5(bytes([identifier]) + password.encode("gbk") + challenge)`
- **超时处理**：每秒发一次 `EAPOL-Start` + `sniff` 等待
- **scapy 配置**：认证前后保存/恢复 `conf.iface`
- **日志去重**：同一次认证中连续相同的消息仅 emit 一次

### 8.3 认证结果处理

```
AuthWorker 完成
    │
    ├── SUCCESS:
    │    瞬态 AUTHENTICATING → 后台验证 IP 连通性（ping 对端 DNS）
    │     ├── 连通 → AUTHENTICATED + schedule_renew
    │     └── 不可达 → AUTHENTICATED + 30 秒后重试
    │
    └── 续期失败（之前已认证，非手动）:
    │    静默重试，不发送 EAPOL-Logoff、不改变状态、不中断当前会话
    │
    └── 首次认证失败:
         ├── 还有候选网卡 → 尝试下一个
         └── 候选耗尽 → schedule_retry
```

> 续期失败不发送 Logoff 是为了避免中断交换机上已有的认证会话，
> 保证重试期间网络不中断。

---

## 九、续期与维护

认证成功后调用 `_schedule_renew()`。`RenewListener` 是一个后台
`threading.Thread`，持续 `sniff` 网卡上的 EAPOL 帧：

```
RenewListener (threading)
    │
    sniff(iface, filter="ether proto 0x888e", timeout=1)
    │
    ├── 检测到 EAP-Request/Identity 或 EAP-Request/MD5
    │   → set reauth_event → 主循环触发 _safe_reauth
    │   → _start_auth_flow(force=True)
    │
    └── 一直 sniff 到 stop_event.set()
```

SYSU 交换机在认证成功后约 **2 分钟**主动发起重认证请求，纯被动监听即可
覆盖续期需求。发射一次信号后自动退出，由下一次认证成功重新创建。

`_safe_reauth` 守卫：认证线程存活时跳过，避免重叠。

---

## 十、网卡状态监听（`_check_iface_status`，5 秒）

唯一的硬件检测入口，由服务主循环调度，覆盖所有网卡相关场景：

```
_check_iface_status()
    │
    ├── 无配置网卡 → pick_best_candidate()
    │   ├── 找到 → 补充 iface 配置 + auto_auth 时自动认证
    │   └── 无网卡 → 静默跳过
    │
    ├── 网卡名消失 → 按 last_success_mac 找回
    │   ├── 找到 → 更新配置 + 继续
    │   └── 未找到 → current_up = False
    │
    ├── 状态未变 → 跳过
    │
    └── 状态变化:
         ├── 连接 → 断开: 更新状态，尝试故障转移至替代网卡
         └── 断开 → 连接: auto_auth + 未认证 → 自动认证
```

**设计原则**：硬件变化直接触发认证，不依赖独立的被动重试机制。

---

## 十一、重试策略

```
_schedule_retry()
    │
    ├── 已认证 → 跳过
    ├── 启动宽限期内（60 秒）→ Fibonacci: 3/5/8/13/21 秒
    ├── 超过 MAX_RETRIES (5) → 停止，"请检查 NetID 或网络"
    └── 正常 → config.retry_interval 秒（默认 60）
```

只有实际发起了认证且交换机无响应或拒绝之后才会调用 `_schedule_retry()`。
无网卡时不重试。

---

## 十二、Npcap 管理

`core/npcap.py` 完整生命周期：

| 阶段       | 实现                                               | 说明                    |
| ---------- | -------------------------------------------------- | ----------------------- |
| 检测       | `ctypes.util.find_library("wpcap")` + 系统目录查找 | 双保险                  |
| 下载       | `urllib.request.urlretrieve` → `%TEMP%`            | 文件大小校验 ≥ 1 MB     |
| 提权安装   | `ShellExecuteW("runas")` → 兜底 PowerShell         | 先诊断 UAC + 管理员状态 |
| UAC 检测   | 注册表 `HKLM\...Policies\System\EnableLUA`         | 辅助诊断                |
| 管理员检测 | `OpenProcessToken` + `TokenElevation`              | 辅助诊断                |

---

## 十三、GUI 行为设计

### 13.1 UI 布局

主窗口（`MainWindow`）：固定尺寸 660 × 360，双栏布局。

左侧"配置面板"组框：

```
┌─ 配置面板 ──────────────────────────────────┐
│  NetID  [_____________________________]       │
│  密码   [_____________________________] [◉]   │
│  网卡   [自动探测有线网卡             ▼  ]   │
│                                               │
│  ☑ 以服务模式自动认证，退出程序不影响认证        │
│                                               │
│  ☐ 开机启动程序      ☑ 启动后隐藏窗口          │
│  ☑ 启动后自动认证    ☑ 状态变化时通知           │
│                                               │
│  [重新连接]  [断开连接]                       │
│  [显示日志]   [退出程序]                      │
└───────────────────────────────────────────────┘
```

> 4 个复选框逻辑见[配置模型](#131-appconfig-字段)。

```

右侧"状态面板"组框：状态卡片（彩色边框） + 信息表格（网卡/IPv4/MAC/网关/DNS）。

### 13.2 托盘菜单

```

状态：已认证 / 待命 / ...
──────────────
重新连接
断开连接
──────────────
显示窗口
──────────────
退出程序

```

### 13.3 启动时序

```

Windows 启动
└─ SCM 启动 sysu_netauth_service.exe (Session 0)
├─ 读取 config.json
├─ service_mode + auto_auth + 有凭据 → 认证 → 主循环
└─ auto_auth=false / 无凭据 → 待命 → 主循环

用户登录（--startup）
├─ service_mode=true → sc config start=auto + sc start
│ （确保服务可独立运行，无需 GUI）
├─ launch_gui_on_login=false → 不启动 GUI（服务静默认证）
└─ launch_gui_on_login=true → GUI 启动
├─ hide_window_on_login + 凭据齐全 + Npcap 已装 → 隐藏到托盘
└─ 否则 → 显示窗口

用户手动双击（无参数）
├─ 单例检测（QLocalServer）→ 已有实例则激活旧窗口
└─ 始终显示主窗口 + 托盘（hide_window_on_login 仅对 --startup 生效）

```

### 13.4 关键行为

- GUI 不执行任何认证逻辑，全程通过 `write_command()` 触发服务操作
- 每 2 秒轮询 `status.json` 更新 UI 状态；AUTHENTICATED 稳定态下服务每 5 秒刷新心跳
- 状态卡片通过 150ms debounce 防止连续状态跳变时的视觉闪烁
- 配置变化立即写入 `config.json` + `write_command("reload_config")`
- 通知由 GUI 在轮询到状态变化时弹出，仅首次认证成功时通知（`_first_auth_notification_sent` 标记），续期恢复不重复弹窗
- Npcap 安装引导：下载 → 提权启动安装向导 → 每 2 秒轮询检测（最长 90 秒）
- 服务运行在 Session 0 不能可靠弹出桌面通知，因此桌面通知由 GUI 负责

---

## 十四、配置模型

### 14.1 AppConfig 字段

| 字段                   | 类型 | 默认值       | 说明                         |
| ---------------------- | ---- | ------------ | ---------------------------- |
| `username`             | str  | `""`         | NetID                        |
| `password`             | str  | `""`         | 明文密码                     |
| `iface`                | str  | `""`         | 指定网卡名（手动模式）       |
| `iface_mode`           | str  | `"auto"`     | `"auto"` / `"manual"`        |
| `auto_auth`            | bool | `true`       | GUI 启动后自动认证           |
| `retry_interval`       | int  | `60`         | 失败重试间隔秒（15–3600）    |
| `close_behavior`       | str  | `"minimize"` | `"minimize"` / `"quit"`      |
| `close_behavior_asked` | bool | `false`      | 是否已询问过关闭行为         |
| `service_mode`         | bool | `true`       | 服务独立运行，退出 GUI 不影响认证 |
| `launch_gui_on_login`  | bool | `false`      | 用户登录后启动 GUI           |
| `hide_window_on_login` | bool | `true`       | 仅 --startup 时启动后隐藏窗口 |
| `desktop_notify`       | bool | `true`       | 状态变化时弹出桌面通知       |
| `last_success_iface`   | str  | `""`         | 上次成功网卡名（自动缓存）   |
| `last_success_mac`     | str  | `""`         | 上次成功 MAC（自动缓存）     |

### 14.2 已废弃的旧字段

旧版配置中的 `operation_mode`、`auto_start`、`show_tray`、`hide_on_start`、
`notify_on_success`、`notify_on_failure` 在读取时静默忽略，写入时不再输出。


### 14.3 配置路径

- 当前：`%ProgramData%\SYSUNetAuth\config.json`
- 旧版（自动迁移）：`%APPDATA%\SYSUNetAuth\config.json`
- GUI 自启快捷方式：`%APPDATA%\Microsoft\Windows\Start Menu\Programs\Startup\SYSU NetAuth.lnk`

---

## 十五、Windows 服务

### 15.1 服务定义

- 名称：`SYSUNetAuth`
- 显示名称：`SYSU NetAuth`
- 描述：SYSU wired campus network 802.1X authentication service
- `win32serviceutil.ServiceFramework` 子类
- 日志：`RotatingFileHandler`（512KB × 3），路径 `%ProgramData%\SYSUNetAuth\service.log`
- 启动引导日志：`service_bootstrap.log`（确认 SCM 是否拉起进程）

### 15.2 命令行入口

```

sysu_netauth_service.exe install # 安装服务
sysu_netauth_service.exe remove # 卸载服务
sysu_netauth_service.exe start # 启动服务
sysu_netauth_service.exe stop # 停止服务
sysu_netauth_service.exe debug # 前台调试运行

```

### 15.3 故障恢复

安装时自动配置 `sc.exe failure`，首次失败后 60 秒重启服务。

---

## 十六、打包与部署策略

### 16.1 双 EXE 架构

| 可执行文件                 | 内容         | 依赖                              |
| -------------------------- | ------------ | --------------------------------- |
| `sysu_netauth.exe`         | GUI/CLI 入口 | 包含 PySide6                      |
| `sysu_netauth_service.exe` | 服务入口     | 不含 PySide6（仅 core + service） |

双 EXE 避免服务二进制携带 Qt，减小体积和启动开销。

### 16.2 安装包行为

1. 停止旧版 GUI 和旧服务
2. 删除旧的 SYSTEM 计划任务
3. 创建 `%ProgramData%\SYSUNetAuth` 并设置普通用户可写
4. 注册 `SYSUNetAuth` 服务，启动类型为自动
5. 配置失败恢复（`sc failure`）
6. 加入系统 PATH
7. 安装完成后启动服务

### 16.3 卸载包行为

1. 停止并删除 `SYSUNetAuth` 服务
2. 删除旧 Startup 快捷方式和旧计划任务
3. 询问是否保留 `%ProgramData%\SYSUNetAuth`（含账号密码）

### 16.4 便携版

`SYSUNetAuth_Portable_v{version}.zip` 内含：

- `sysu_netauth.exe`
- `sysu_netauth_service.exe`
- `Install-Service.cmd` / `Uninstall-Service.cmd` / `Service-Status.cmd` / `Start-GUI.cmd`

---

## 十六、验证清单

- 服务进程启动时不导入 PySide6
- 未登录 Windows 时，`status.json` 更新到 `authenticating` 或 `authenticated`
- 用户登录后 GUI 能正确显示服务状态
- GUI 关闭后服务继续运行
- 注销用户后服务继续运行
- 网线拔插后服务进入 `waiting_network`，恢复后自动认证
- 收到交换机重认证帧后触发维护认证
- 安装包升级时保留 `%ProgramData%\SYSUNetAuth` 配置

---

## 十七、配置演化历史（备忘）

| 版本   | 变更                                                               |
| ------ | ------------------------------------------------------------------ |
| 早期   | 单进程 GUI 认证，可选"服务模式"/"普通模式"                         |
| 重构后 | 服务始终执行认证，GUI 退化为配置面板；废除 `operation_mode` 等字段 |
| 当前   | 双进程架构稳定运行，配置字段保持向后兼容                           |

> 本文档中未明确提及的细节请直接阅读源代码。每个模块的入口函数和类
> 都有详细的 docstring 说明。
```
