# 架构说明

> 本文面向维护者。用户文档见 [README](../README.md)。
>
> Python 包名 `sysu_netauth`，用户可见名称 **SYSU NetAuth**。

---

## 1. 架构概览

### 1.1 双进程模型

```
系统启动
  └─ SYSUNetAuth Windows 服务 (Session 0)     ← 认证执行者
       ├─ 读取 config.json
       ├─ 自动选择有线网卡，执行 802.1X 认证
       ├─ 被动监听交换机重认证 (RenewListener)
       ├─ 监听网卡插拔/重命名/故障转移
       └─ 写入 status.json

用户登录
  └─ sysu_netauth.exe GUI (Session 1)         ← 配置面板 + 状态监视器
       ├─ 编辑共享配置 → config.json
       ├─ 轮询 status.json 展示服务状态
       ├─ 写 command.json 触发操作
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

```
run.py ──→ runner.py ──┬─ 无 CLI 参数 ──→ app/tray.py（GUI 配置面板）
                        ├─ --startup    ──→ 处理服务模式 + 启动 GUI 或静默
                        ├─ --service    ──→ service/win_service.py（Windows 服务）
                        └─ 有 CLI 参数 ──→ cli.py（命令行模式）
```

- `runner.py`：单例保护（`QLocalServer` IPC）、`AttachConsole` 挂接父进程终端、分发入口
- GUI 模式：`--startup` 标记时以用户登录后自启模式运行
- 服务模式：`--service` 转交 pywin32 服务框架；打包后使用 `sysu_netauth_service.exe`（不含 PySide6）
- CLI 模式：所有非 `--startup` 非 `--service` 参数转交 `cli.py` 解析

### 1.4 文件结构

```
sysu_netauth/
├── runner.py           # 入口分发：单例保护、AttachConsole、CLI/GUI 路由
├── cli.py              # argparse CLI：认证/探测/注销/网卡列表/检查 Npcap
│
├── core/               # ── 无 GUI 核心库（可被 service 和 app 共用）──
│   ├── config.py       # AppConfig 数据类、JSON 读写、旧配置迁移、自启快捷方式
│   ├── shared_store.py # 进程间共享文件 (config/status/command JSON)
│   ├── single_instance.py  # QLocalServer 单实例 IPC（仅 GUI 模式）
│   ├── eapol.py        # EAPOL/MD5 协议栈：帧构造、parse、认证握手、注销
│   ├── interfaces.py   # 网卡枚举、类型判定、评分排序、EAPOL 探测
│   └── npcap.py        # Npcap 检测、下载、完整性校验、提权安装
│
├── service/            # ── Windows 服务进程（不含 Qt）──
│   ├── engine.py       # 无 Qt 认证状态机：认证、续期、网卡监听、命令处理、重试
│   └── win_service.py  # pywin32 服务宿主
│
└── app/               # ── GUI 配置面板（不含认证逻辑）──
    ├── tray.py         # 托盘、服务状态轮询、配置编辑、Npcap 引导
    ├── views.py        # GUI 组件：MainWindow、_NetworkTable、CloseBehaviorDialog
    └── workers.py      # QThread：NpcapDownloadWorker
```

---

## 2. 设计细节

### 2.1 进程间通信：Shared Store

三个 JSON 文件位于 `%ProgramData%\SYSUNetAuth\`，所有写入使用**临时文件 + 原子替换**（先写 `.tmp` 再 `replace`），避免读取到半写入文件。

| 文件           | 写入者                 | 读取者     | 用途                 |
| -------------- | ---------------------- | ---------- | -------------------- |
| `config.json`  | GUI / 服务缓存成功网卡 | 服务 / GUI | 账号、密码、网卡策略 |
| `status.json`  | 服务                   | GUI / CLI  | 当前认证状态         |
| `command.json` | GUI                    | 服务       | 手动认证/注销/重载   |

**为什么用 JSON 文件而不是管道/套接字？**

- 服务运行在 Session 0，GUI 在 Session 1，跨 Session 的命名管道需要额外安全配置
- JSON 文件天然持久化——服务崩溃重启后状态不丢失
- 调试友好：直接用记事本打开即可查看状态
- 不需要低延迟通信（状态轮询间隔 2s，命令检查间隔 1s），文件 IO 完全够用

#### config.json

`core/config.py` 的 `AppConfig` 数据类管理所有配置字段。旧版 `%APPDATA%\SYSUNetAuth\config.json` 首次运行时自动迁移到 `%ProgramData%`（不删除旧文件）。

#### status.json

```json
{
  "state": "authenticated",
  "message": "已认证",
  "iface": "以太网",
  "mac": "00:11:22:33:44:55",
  "ipv4": "10.0.0.2",
  "updated_at": 697.2198229,
  "authenticated_at": "2026-07-05T12:00:00+08:00"
}
```

状态值：`idle` / `authenticating` / `authenticated` / `failed` / `stopped`

#### command.json

```json
{ "action": "authenticate", "created_at": "2026-07-05T12:00:00+08:00" }
```

支持的命令：`authenticate`、`logoff`、`reload_config`。服务读取后删除文件。

---

### 2.2 服务端状态机

引擎维护五态 `ServiceState`，各状态**互斥**——行为判定只需检查 `self.state`，无需组合标志位：

```
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

| 状态             | 含义         | 行为                                                                            |
| ---------------- | ------------ | ------------------------------------------------------------------------------- |
| `IDLE`           | 待命中       | 全量 tick，由 `_next_retry_at`/`_media_available`/`_manual_disconnect` 驱动决策 |
| `AUTHENTICATING` | 握手进行中   | 全量 tick，`_auth_thread.is_alive()` 防止重复                                   |
| `AUTHENTICATED`  | 认证成功     | 全量 tick，`authenticated` 属性唯一依赖本状态                                   |
| `FAILED`         | 不可恢复错误 | 全量 tick，停止自动重试                                                         |
| `STOPPED`        | 服务停止     | 不进入主循环                                                                    |

**设计要点**（来自重构经验）：

- 原 `STARTING`、`NO_MEDIA`、`WAITING_NETWORK` 三态统一归入 `IDLE`，通过消息字符串区分子场景。减少状态组合爆炸
- `NO_MEDIA` 的行为守卫改用 `_check_iface_status()` 中实时计算的 `current_media` 条件，而非状态值
- 重试调度由 `_next_retry_at` 定时器独立驱动，与状态正交
- `_manual_disconnect` 是与状态正交的布尔标志，仅用于 GUI 图标区分

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

引擎以 1 秒 tick 运行，依次处理：

1. 读取 `command.json` 指令
2. 每 3 秒重载配置文件
3. 每 5 秒检查网卡状态变化（`_check_iface_status`）
4. 检测 `RenewListener` 失效事件 → 触发重认证
5. 重试定时器到期执行重试
6. 每 5 秒刷新 `status.json` 心跳

认证线程与 `RenewListener` 不会同时嗅探同一网卡。

**为什么单线程 + tick 而不是事件驱动？**

- Windows 服务环境的事件源有限（无 `select`/`epoll` 监听网卡事件）
- 轮询间隔 1 秒对认证场景足够（交换机握手间隔 ~120 秒）
- 状态机逻辑集中在一个循环中，便于调试和日志追踪

---

### 2.4 认证流程

#### 候选网卡构建

```
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

```
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

```
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

```
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

使用 `QLocalServer` / `QLocalSocket`（命名管道 IPC），见 `core/single_instance.py`。

```
第二次启动
  ├─ QApplication(sys.argv)
  ├─ SingleInstanceManager.notify_existing()
  │   ├─ 管道存在 → 发送 "activate" → sys.exit(0)
  │   └─ 管道不存在 → start_server() → 成为主实例
  │
主实例
  ├─ activate_requested 信号 → tray.show_status() ← 自己恢复窗口
  └─ 进程退出 → 管道自动释放
```

**为什么不用 Mutex？**

- 进程崩溃后 Mutex 可能残留
- Mutex 方案需要 `EnumWindows` 猜 HWND，不可靠
- `QLocalServer` 与 Qt 事件循环天然集成，无竞态

仅在 GUI 模式启用；CLI 模式允许多开。

---

### 2.10 GUI 行为概要

GUI 的详细布局和组件不在此重复（直接读 `views.py` 和 `tray.py` 更快），这里只记录关键设计意图：

- **GUI 不执行认证**：全程通过 `write_command()` 触发服务操作
- **状态轮询**：每 2 秒读 `status.json`；`AUTHENTICATED` 稳定态下服务每 5 秒刷新心跳
- **防闪烁**：状态卡片通过 150ms debounce 防止连续状态跳变时的视觉闪烁
- **防重复通知**：内置冷却期，防止短时间内重复弹窗
- **通知归属**：服务运行在 Session 0 不能可靠弹出桌面通知，因此桌面通知由 GUI 负责
- **配置即时生效**：配置变化立即写入 `config.json` + `write_command("reload_config")`
- **启动时序**：见 `12.3`（README 有精简版），完整逻辑在 `runner.py` 和 `tray.py`

---

### 2.11 配置模型

完整的配置字段表见 README「配置文件」章节，此处仅记录与架构相关的要点：

- **存储路径**：`%ProgramData%\SYSUNetAuth\config.json`（旧版 `%APPDATA%` 自动迁移）
- **密码存储**：明文。服务在 Session 0 无法可靠调用 Credential Manager，JSON 文件 ACL 设为仅管理员可读
- **废弃字段**：旧版配置中的 `operation_mode`、`auto_start`、`close_behavior` 等在读取时静默忽略，写入时不再输出

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

### 3.2 为什么五态而不是枚举所有场景？

旧版本有 7 个状态（含 `STARTING`、`NO_MEDIA`、`WAITING_NETWORK`），问题：

- 状态组合爆炸：`NO_MEDIA + 重试中` vs `WAITING_NETWORK + 待命` 难以区分
- 新增场景需要新增状态，导致状态机膨胀
- GUI 端需要等量的映射分支

重构后：无网线、无候选网卡、重试等待等全部归入 `IDLE`，通过 `status.json` 的 `message` 字段区分子场景。GUI 端只需要 5 种图标映射。

### 3.3 为什么用 JSON 文件而不是进程间 IPC？

参见 2.1 节末尾的说明。核心考量：跨 Session 通信 + 持久化 + 调试友好。

### 3.4 为什么 GUI 不直接调用认证 API？

历史教训：旧版本 GUI 内置认证状态机，导致：

- GUI 退出后认证中断（用户以为还在线）
- Session 0 和 Session 1 的网卡枚举结果不一致
- GUI 卡死时影响认证

解决方案：认证职责完全交给 Windows 服务，GUI 退化为配置面板。

---

## 4. 打包与部署

### 4.1 构建流程

```
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

1. 停止旧版 GUI 和旧服务（`taskkill` + `sc stop`，最多等 10 秒）
2. 删除旧的 SYSTEM 计划任务（兼容旧版遗留）
3. 创建 `%ProgramData%\SYSUNetAuth` 并设置 Users 组可写
4. 注册 `SYSUNetAuth` 服务，启动类型自动
5. 配置故障恢复（`sc failure`：首次失败 60 秒后重启）
6. 加入系统 PATH（支持 `sysu_netauth` 命令行）
7. 安装完成后启动服务

### 4.3 卸载包行为

1. 停止并删除 `SYSUNetAuth` 服务
2. 删除旧 Startup 快捷方式和旧计划任务
3. 询问是否保留 `%ProgramData%\SYSUNetAuth`（含账号密码）

---

## 5. 验证指南

### 5.1 服务独立性

```
# 未登录 Windows 时验证
sc start SYSUNetAuth
timeout /t 15 /nobreak
Get-Content "$env:ProgramData\SYSUNetAuth\status.json"
# 预期: state → "authenticated"（若已配置凭据且有网线）
```

### 5.2 GUI 与服务分离

```
# GUI 退出后服务继续运行
# 1. 打开 GUI，确认状态显示正常
# 2. 关闭 GUI
# 3. 检查 status.json: state 仍为 "authenticated" 或后续变化
# 4. 确认仍可访问校园网
```

### 5.3 网卡热插拔

```
# 服务已认证状态
# 1. 拔掉网线
# 2. 等待 ≤ 10 秒，检查 status.json: state → "idle", message 含网线断开
# 3. 插回网线
# 4. 等待 ≤ 20 秒，检查 status.json: state → "authenticating" → "authenticated"
```

### 5.4 交换机重认证

```
# 认证成功后：
# 1. 启动 Wireshark 过滤 eapol，观察约每 120 秒出现 EAP-Request/Identity
# 2. 确认程序自动回复 Identity Response
# 3. status.json 应保持 "authenticated"，无状态跳变
```

### 5.5 服务进程隔离

```
# 确认服务不加载 PySide6
Get-Process -Name sysu_netauth_service | Select-Object -ExpandProperty Modules |
    Where-Object ModuleName -like "*Qt*"
# 预期: 空列表
```
