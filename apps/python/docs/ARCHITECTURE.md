# 架构说明

> 本文面向维护者。用户文档见 [README](../README.md)。
>
> Python 包名 `sysu_netauth`，用户可见名称 **SYSU NetAuth**。

---

## 1. 架构概览

### 1.1 双进程模型

```text
系统启动
  └─ SYSUNetAuth Windows 服务 (Session 0)     ← 认证执行者
       ├─ 读取 config.json
       ├─ 自动选择有线网卡，执行 802.1X 认证
       ├─ 被动监听交换机重认证 (RenewListener)
       ├─ 监听网卡插拔/重命名/故障转移
       ├─ 写入 service_cache.json
       └─ 写入 status.json

用户登录
  └─ sysu_netauth.exe GUI (Session 1)         ← 配置面板 + 状态监视器
       ├─ 编辑共享配置 → config.json
       ├─ 轮询 status.json 展示服务状态
       ├─ 写 commands/*.json 队列触发操作
       └─ 引导 Npcap 安装
```

**核心原则**：认证的唯一执行者是后台 Windows 服务。GUI 不含认证逻辑，退出 GUI 不影响在线状态。

### 1.2 模块边界

| 模块       | 可导入                       | 不可导入                |
| ---------- | ---------------------------- | ----------------------- |
| `core/`    | Python 标准库、scapy、psutil | PySide6、`app.*`        |
| `service/` | `core.*`、pywin32            | PySide6、`app.*`        |
| `app/`     | `core.*`、PySide6            | `service.*`（文件通信） |

### 1.3 入口分发

```text
run.py ──→ runner.py ──┬─ 无 CLI 参数 ──→ app/tray.py（GUI 配置面板）
                        ├─ --startup    ──→ 处理服务模式 + 启动 GUI 或静默
                        ├─ --service    ──→ service/win_service.py（Windows 服务）
                        └─ 有 CLI 参数 ──→ cli.py（命令行模式）
```

- `runner.py`：单例保护（Win32 Mutex）、`AttachConsole` 挂接父进程终端、分发入口
- GUI 模式：`--startup` 标记时以用户登录后自启模式运行
- 服务模式：`--service` 转交 pywin32 服务框架；打包后使用 `sysu_netauth_service.exe`（不含 PySide6）
- CLI 模式：所有非 `--startup` 非 `--service` 参数转交 `cli.py` 解析

### 1.4 文件结构

```text
sysu_netauth/
├── runner.py           # 入口分发：单例保护(Win32 Mutex)、AttachConsole、CLI/GUI 路由
├── cli.py              # argparse CLI：认证/探测/注销/网卡列表/检查 Npcap
│
├── core/               # ── 无 GUI 核心库（可被 service 和 app 共用）──
│   ├── config.py       # 配置/状态/命令存储、服务运行时缓存
│   ├── assets.py       # 资源路径解析（开发/frozen 模式兼容）
│   ├── eapol.py        # EAPOL/MD5 协议栈：帧构造、parse、认证握手、注销
│   ├── interfaces.py   # 网卡枚举(Win32 GetAdaptersAddresses)、类型判定、评分、EAPOL 探测、网络信息
│   ├── npcap.py        # Npcap 检测、下载、完整性校验、提权安装
│   └── update.py       # 更新清单解析、版本比较、可信 URL 校验
│
├── service/            # ── Windows 服务进程（不含 Qt）──
│   ├── engine.py       # 无 Qt 认证状态机：认证、续期、网卡监听、命令处理、重试
│   ├── update_checker.py # 联网后的低频更新检查与失败退避
│   └── win_service.py  # pywin32 服务宿主
│
└── app/               # ── GUI 配置面板（不含认证逻辑）──
    ├── startup.py      # GUI 的最高权限登录计划任务
    ├── tray.py         # 托盘、服务状态轮询、配置编辑、Npcap 引导
    ├── views.py        # GUI 组件：MainWindow、_NetworkTable、CloseBehaviorDialog
    └── workers.py      # QThread：NpcapDownloadWorker
```

---

## 2. 设计细节

### 2.1 进程间通信：Shared Store

共享文件位于 `%ProgramData%\SYSUNetAuth\`，所有写入使用**唯一临时文件 + 原子替换**，避免读取到半写入文件和多进程临时文件冲突。

| 文件/目录            | 写入者 | 读取者     | 用途                         |
| -------------------- | ------ | ---------- | ---------------------------- |
| `config.json`        | GUI    | 服务 / GUI | 账号、密码、用户策略         |
| `service_cache.json` | 服务   | 服务       | 自动网卡、上次成功 MAC       |
| `status.json`        | 服务   | GUI / CLI  | 当前认证状态                 |
| `update_state.json`  | 服务   | GUI        | 更新检查事实及调度状态       |
| `ui_state.json`      | GUI    | GUI        | 通知和忽略状态               |
| `commands/*.json`    | GUI    | 服务       | 有序的服务命令队列           |
| `command.json`       | 外部   | 服务       | 兼容旧版本的单命令入口       |

**为什么用 JSON 文件而不是管道/套接字？**

- 服务运行在 Session 0，GUI 在 Session 1，跨 Session 的命名管道需要额外安全配置
- JSON 文件天然持久化——服务崩溃重启后状态不丢失
- 调试友好：直接用记事本打开即可查看状态
- 不需要低延迟通信（状态轮询间隔 2s，命令检查间隔 1s），文件 IO 完全够用

#### config.json

`core/config.py` 的 `AppConfig` 数据类管理所有配置字段。

#### status.json

```json
{
  "state": "authenticated",
  "message": "已认证",
  "iface": "以太网",
  "mac": "00:11:22:33:44:55",
  "ipv4": "10.0.0.2",
  "gateway": "10.0.0.1",
  "dns": "114.114.114.114, 223.5.5.5",
  "driver": "Realtek USB GbE Family Controller",
  "updated_at": 1783224000.0,
  "authenticated_at": "2026-07-05T12:00:00+08:00"
}
```

状态值：`idle` / `authenticating` / `authenticated` / `failed` / `stopped`

#### commands/*.json

```json
{ "action": "authenticate", "created_at": "2026-07-05T12:00:00+08:00" }
```

每条命令使用独立文件，按文件名顺序消费，避免 GUI 快速操作时相互覆盖。支持 `authenticate`、`logoff`、`reload_config`、`check_update`。旧版 `command.json` 仍可读取。

---

### 2.2 服务端状态机

引擎维护五态 `ServiceState`，各状态**互斥**——行为判定只需检查 `self.state`，无需组合标志位：

```text
                     ┌──────────────┐
                     │    IDLE      │
                     │ (待命/无网线 │
                     │  重试等待)    │
                     └──┬───────┬───┘
                        │       │
                        ▼       │
                  ┌──────────┐  │
                  │AUTHENTIC.│  │
                  │ (握手进行)│  │
                  └────┬─────┘  │
                       │        │
                  ┌────▼─────┐  │
                  │AUTHENTIC.│  │
                  │ (已认证)  │  │
                  └────┬─────┘  │
                       │        │
                  ┌────▼─────┐  │
                  │  FAILED  │──┘
                  │(不可恢复) │
                  └──────────┘
                  ┌──────────┐
                  │  STOPPED │
                  │ (服务退出)│
                  └──────────┘
```

| 状态             | 含义         | 行为                                     |
| ---------------- | ------------ | ---------------------------------------- |
| `IDLE`           | 待命中       | 等待触发（介质恢复、重试到期、用户指令） |
| `AUTHENTICATING` | 握手进行中   | 防止并发启动多次认证                     |
| `AUTHENTICATED`  | 认证成功     | RenewListener 常驻保活                   |
| `FAILED`         | 不可恢复错误 | 停止自动重试，需用户介入                 |
| `STOPPED`        | 服务停止     | 不进入主循环                             |

**设计原则**：

- **最小状态集**：中间态（无网线、等待网络、重试等待）统一归入 `IDLE`，通过 `status.json` 的 `message` 字段区分子场景，避免状态组合爆炸
- **正交关注点分离**：介质检测、重试调度、手动断开三者独立管理，各自的判定条件不与状态值耦合
- **GUI 仅依赖状态枚举值**：5 个图标颜色直接映射 5 个状态，`message` 仅作展示文本

GUI 端通过轮询 `status.json` 映射为图标：

| 图标颜色 | 对应服务状态                  |
| -------- | ----------------------------- |
| 灰色     | `IDLE`（手动断开）/ `STOPPED` |
| 蓝色     | `IDLE`                        |
| 橙色     | `AUTHENTICATING`              |
| 绿色     | `AUTHENTICATED`               |
| 红色     | `FAILED`                      |

---

### 2.3 服务主循环

引擎以 1 秒 tick 运行，周期性任务通过统一的 `_timers` 列表管理：

1. 读取兼容 `command.json` 与 `commands/*.json` 队列指令
2. 周期性任务调度（每个 timer 持有 `[interval, next_run, callback]`）：
   - 每 3 秒重载配置文件
   - 每 5 秒检查网卡状态变化
   - 每 5 秒刷新 `status.json` 心跳
   - 每 60 秒判断更新检查是否到期；请求使用 Windows 路由表选择任意可用网络
3. 检测 `RenewListener` 失效事件 → 触发重认证
4. 重试定时器到期执行重试

认证线程与 `RenewListener` 不会同时嗅探同一网卡。

**为什么单线程 + tick 而不是事件驱动？**

- Windows 服务环境的事件源有限（无 `select`/`epoll` 监听网卡事件）
- 轮询间隔 1 秒对认证场景足够（交换机握手间隔 ~120 秒）
- 状态机逻辑集中在一个循环中，便于调试和日志追踪

---

### 2.4 认证流程

#### 候选网卡构建

```text
_auth_candidates()
    ├── manual 模式 → _resolve_saved_iface()
    │                  1. 配置的网卡名
    │                  2. 按 last_success_mac 找回
    └── auto 模式 → _auto_auth_candidates()
                     1. last_success_mac 对应的网卡
                     2. 配置的 iface
                     3. 其余已连接有线网卡（按评分）
```

空列表 → 不重试，等待 5 秒硬件监听触发。

#### 协议握手

`core/eapol.authenticate()` 实现标准 EAP-MD5：

```text
EAPOL-Start ──广播──→ PAE 组播 MAC 01:80:c2:00:00:03
                    ←── EAP-Request/Identity ──
EAP-Response/Identity (NetID, GBK) ──→
                    ←── EAP-Request/MD5-Challenge ──
EAP-Response/MD5 (MD5(id|GBK(password)|challenge)) ──→
                    ←── EAP-Success / EAP-Failure / 超时
```

关键细节：

- 密码编码：GBK（`password.encode("gbk")`）
- MD5 计算：`hashlib.md5(bytes([identifier]) + password.encode("gbk") + challenge)`
- 超时处理：每秒发一次 `EAPOL-Start` + `sniff` 等待
- 日志去重：同一次认证中连续相同的消息仅 emit 一次

#### 认证结果处理

- **SUCCESS**：先置 `AUTHENTICATING`，后台 ping 验证连通性；通过后设 `AUTHENTICATED` 并启动 `RenewListener`；未通过则退避重试
- **FAILURE/TIMEOUT**：候选网卡故障转移或退避重试；启动宽限期（60s）内使用 Fibonacci 间隔

---

### 2.5 握手响应（在线维护）

`RenewListener` —— 认证成功、连通性验证通过后启动的常驻后台线程：

- 持续嗅探网卡上的 EAPOL 帧（`filter="ether proto 0x888e"`）
- 收到 **EAP-Request/Identity**：inline 回复 `EAP-Response/Identity`（握手保活应答）
- 收到 **EAP-Request/MD5**：inline 回复 `EAP-Response/MD5`（真正重认证）
- 收到 **EAP-Failure**：通知引擎会话失效，触发完整重认证
- 线程不退出一—只要网络在线就持续响应

**为什么不在主线程做？** `sniff()` 是阻塞调用，不能放在 tick 循环里。独立线程 + daemon 模式确保服务退出时自动终止。

---

### 2.6 网卡状态监听

唯一的硬件检测入口，由服务主循环每 5 秒调度：

```text
_check_iface_status()
    ├── 无配置网卡 → pick_best_candidate()
    │   ├── 找到 → 补充 iface 配置 + auto_auth 时自动认证
    │   └── 无网卡 → 静默跳过
    ├── 网卡名消失 → 按 last_success_mac 找回
    │   ├── 找到 → 更新配置 + 继续
    │   └── 未找到 → current_up = False
    ├── 状态未变 → 跳过
    └── 状态变化:
         ├── 连接→断开: 更新状态，尝试故障转移
         └── 断开→连接: auto_auth + 未认证 → 自动认证
```

**设计原则**：硬件变化直接触发认证，不依赖独立的被动重试机制。避免"网卡已恢复但还在等重试定时器"的延迟。

---

### 2.7 重试策略

```text
_schedule_retry()
    ├── 已认证 → 跳过
    ├── 启动宽限期内（60 秒）→ Fibonacci: 3/5/8/13/21 秒
    ├── 超过 MAX_RETRIES (5) → 停止，"请检查 NetID 或网络"
    └── 正常 → config.retry_interval 秒（默认 60）
```

只有实际发起了认证且交换机无响应或拒绝之后才会调用 `_schedule_retry()`。无网卡时不重试。

---

### 2.8 Npcap 管理

`core/npcap.py` 管理完整生命周期。由于 Npcap 免费版禁止重新分发，安装包不内置也不自动静默安装。

| 阶段     | 方式                                               |
| -------- | -------------------------------------------------- |
| 检测     | `ctypes.util.find_library("wpcap")` + 系统目录查找 |
| 下载     | `urllib.request.urlretrieve` → `%TEMP%`            |
| 完整性   | 文件大小校验 ≥ 1 MB                                |
| 提权安装 | `ShellExecuteW("runas")` → 兜底 PowerShell         |

---

### 2.9 单例保护（GUI 模式）

使用 Win32 命名 Mutex（`CreateMutexW`），错误码 `ERROR_ALREADY_EXISTS`（183）表示已有实例运行。

```text
第二次启动
  └─ CreateMutexW(name="SYSUNetAuth")
       ├─ GetLastError() == 183 → sys.exit(0)
       └─ 否则 → 成为主实例，进入 GUI 循环
```

进程退出时 Mutex 由 OS 自动释放。仅在 GUI 模式启用；CLI 模式允许多开。

---

### 2.10 GUI 行为概要

GUI 的详细布局和组件不在此重复（直接读 `views.py` 和 `tray.py` 更快），这里只记录关键设计意图：

- **GUI 不执行认证**：全程通过 `write_command()` 触发服务操作
- **状态轮询**：每 2 秒读 `status.json`；`AUTHENTICATED` 稳定态下服务每 5 秒刷新心跳
- **防闪烁**：状态卡片通过 150ms debounce 防止连续状态跳变时的视觉闪烁
- **防重复通知**：内置冷却期，防止短时间内重复弹窗
- **通知归属**：服务运行在 Session 0 不能可靠弹出桌面通知，因此桌面通知由 GUI 负责
- **更新归属**：服务启动后独立调度更新检查，不依赖有线认证状态；可使用无线网等其他互联网通道。GUI 读取持久化结果；服务独立运行时仅写事件日志，绝不从 Session 0 弹窗
- **源优先级**：先读取 Gitee 的 `updates/release.json`，请求失败、响应非法或重定向不可信时自动回退 GitHub
- **检查频率**：首次联网延迟 2–5 分钟，成功后至少间隔 24 小时；失败按 30 分钟、2 小时、6 小时、24 小时退避
- **配置即时生效**：配置变化立即写入 `config.json` + `write_command("reload_config")`

---

### 2.11 配置模型

完整的配置字段表见 README「配置文件」章节，此处仅记录与架构相关的要点：

- **存储路径**：`%ProgramData%\SYSUNetAuth\config.json`
- **单写者**：GUI 写用户配置，服务运行时缓存写入独立 `service_cache.json`
- **未知字段静默忽略**：配置加载时仅提取已知字段，新版本新增字段对旧配置透明
- **登录启动**：管理员清单程序通过“最高权限”计划任务启动，不使用 Startup 快捷方式
- **数据 ACL**：共享目录仅允许 `SYSTEM` 与管理员访问，保护其中的明文凭据

---

## 3. 关键设计决策

### 3.1 为什么双 EXE 架构？

| 可执行文件                 | 内容         | 依赖                              |
| -------------------------- | ------------ | --------------------------------- |
| `sysu_netauth.exe`         | GUI/CLI 入口 | 包含 PySide6 (~30 MB)             |
| `sysu_netauth_service.exe` | 服务入口     | 不含 PySide6（仅 core + service） |

- 服务二进制不携带 Qt，体积减小约 30 MB，启动速度更快
- 服务进程内存占用更低（~15 MB vs GUI 的 ~80 MB）
- 条件 import 无法规避 PyInstaller 打包时自动收集 PySide6，因此必须分为两个入口

### 3.2 为什么是五态？

将认证服务的所有运行场景归入五个互斥状态而非逐一枚举每种场景：

- 状态间是互斥的，行为判定只需检查当前状态，无需组合标志位
- 可恢复的临时场景（无网线、无候选网卡、重试等待）不单独设状态，全部归入 `IDLE`，通过 `status.json` 的 `message` 字段传达具体子场景
- 每个状态对应一种确定的行为模式，新增子场景不会导致状态机膨胀

### 3.3 为什么用 JSON 文件而不是进程间 IPC？

参见 2.1 节末尾的说明。核心考量：跨 Session 通信 + 持久化 + 调试友好。

### 3.4 为什么 GUI 不直接调用认证 API？

将认证职责放在 GUI 进程中存在根本性问题：

- GUI 退出后认证中断，用户无法察觉
- Session 0 和 Session 1 的网卡枚举结果可能不一致
- GUI 阻塞时影响认证时效

因此认证职责完全交给 Windows 服务，GUI 仅作为配置面板和状态展示。

---

## 4. 打包与部署

### 4.1 构建流程

```text
python scripts\build.py              # 完整构建：EXE → 安装包 → ZIP
python scripts\build.py --skip-installer  # 仅 EXE
```

产出：

| 文件                               | 说明                     |
| ---------------------------------- | ------------------------ |
| `dist/sysu_netauth/`               | PyInstaller 输出目录     |
| `SYSUNetAuth_Setup_v{version}.exe` | Inno Setup 安装包        |
| `SYSUNetAuth_Setup_v{version}.zip` | 安装包 ZIP（浏览器友好） |

### 4.2 安装包行为

1. 停止已在运行的 GUI 进程和服务（`taskkill` + `sc stop`，最多等 10 秒）
2. 创建 `%ProgramData%\SYSUNetAuth`，ACL 仅保留 `SYSTEM` 与管理员
3. 注册 `SYSUNetAuth` 服务，启动类型自动
4. 配置故障恢复（`sc failure`：首次失败 60 秒后重启）
5. 加入系统 PATH（支持 `sysu_netauth` 命令行）
6. 安装完成后启动服务

### 4.3 卸载包行为

1. 停止并删除 `SYSUNetAuth` 服务
2. 删除登录计划任务和旧版 Startup 快捷方式等残留文件
3. 询问是否保留 `%ProgramData%\SYSUNetAuth`（含账号密码）

---

## 5. 验证指南

### 5.1 服务独立性

```text
# 未登录 Windows 时验证
sc start SYSUNetAuth
timeout /t 15 /nobreak
Get-Content "$env:ProgramData\SYSUNetAuth\status.json"
# 预期: state → "authenticated"（若已配置凭据且有网线）
```

### 5.2 GUI 与服务分离

```text
# GUI 退出后服务继续运行
# 1. 打开 GUI，确认状态显示正常
# 2. 关闭 GUI
# 3. 检查 status.json: state 仍为 "authenticated" 或后续变化
# 4. 确认仍可访问校园网
```

### 5.3 网卡热插拔

```text
# 服务已认证状态
# 1. 拔掉网线
# 2. 等待 ≤ 10 秒，检查 status.json: state → "idle", message 含网线断开
# 3. 插回网线
# 4. 等待 ≤ 20 秒，检查 status.json: state → "authenticating" → "authenticated"
```

### 5.4 交换机重认证

```text
# 认证成功后：
# 1. 启动 Wireshark 过滤 eapol，观察约每 120 秒出现 EAP-Request/Identity
# 2. 确认程序自动回复 Identity Response
# 3. status.json 应保持 "authenticated"，无状态跳变
```

### 5.5 服务进程隔离

```text
# 确认服务不加载 PySide6
Get-Process -Name sysu_netauth_service | Select-Object -ExpandProperty Modules |
    Where-Object ModuleName -like "*Qt*"
# 预期: 空列表
```
