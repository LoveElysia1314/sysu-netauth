# SYSU NetAuth

中山大学（SYSU）有线校园网 802.1X (EAPOL/MD5) 自动认证客户端，Windows 桌面端。

使用 **NetID**（网络身份标识）认证，与[统一身份认证](https://cas.sysu.edu.cn)一致。
NetID 未激活时可前往 [cas.sysu.edu.cn](https://cas.sysu.edu.cn) 点击「NetID 激活」完成激活。

## 功能

- **自动认证** — 开机或检测到有线网卡后自动完成 802.1X 握手
- **智能网卡选择** — 自动排除虚拟、无线、回环网卡，支持多网卡故障转移
- **手动/自动模式** — 可固定指定网卡，或让程序自动探测最佳网卡
- **续期维护** — 被动监听交换机重认证请求，无需轮询
- **自动重试** — 认证失败后按 Fibonacci 或固定间隔自动重试
- **Windows 服务** — 系统启动后即可认证，无需等待用户登录
- **系统托盘** — 驻留托盘，作为配置面板和服务状态监视器
- **Npcap 引导安装** — 内置 Npcap 下载、完整性校验、提权安装向导
- **CLI 模式** — 支持脚本化认证、探测、注销、网卡列表
- **双进程架构** — 后台 Windows 服务始终执行认证，GUI 负责配置和状态展示

## 架构概览

```
系统启动
  └─ SYSUNetAuth Windows 服务 (Session 0)
       ├─ 读取 %ProgramData%\SYSUNetAuth\config.json
       ├─ 自动选择有线网卡，执行 802.1X 认证
       ├─ 被动监听交换机重认证请求
       ├─ 监听网卡插拔/重命名/故障转移
       └─ 写入 status.json 反映认证状态

用户登录
  └─ sysu_netauth.exe GUI (Session 1)
       ├─ 编辑共享配置 → 写入 config.json
       ├─ 轮询 status.json 展示服务状态
       └─ 写 command.json 触发认证/注销/重载
```

认证始终由后台 Windows 服务执行，GUI 仅作为配置面板和状态监视器。退出 GUI 不影响后台认证。

## 与官方客户端的差异

中山大学官方提供的是[锐捷认证客户端 v4.97](https://inc.sysu.edu.cn/sites/default/files/2021-04/%E9%94%90%E6%8D%B7%E8%AE%A4%E8%AF%81%E5%AE%A2%E6%88%B7%E7%AB%AF%20Windows%E7%89%88%20v4.97.zip)，
两者均使用标准 802.1X (EAPOL/MD5) 协议。本项目的优势：

- **自动网卡故障转移** — 多网卡场景下自动切换可用网卡
- **被动续期监听** — 交换机发起重认证时自动响应，无需频繁轮询
- **轻量无广告** — 仅托盘图标 + 控制面板，无额外驻留进程
- **开源可审计** — 代码公开，仅实现标准 EAP-MD5 路径，无闭源组件

## 快速开始

### 前置依赖

需要 **Npcap**（WinPcap API 兼容模式），用于收发二层 EAPOL 帧。
程序内置安装引导，首次启动时会提示下载安装。保持默认选项即可。

### 从源码运行

```powershell
pip install -r requirements.txt
python run.py
```

### 命令行诊断

```powershell
# 列出可用有线网卡
python run.py --list-ifaces

# 探测指定网卡是否有 EAPOL 认证服务器
python run.py -i "以太网 2" --probe-iface

# 执行认证
python run.py -i "以太网 2" -u "你的NetID" -p "你的密码"

# 发送注销帧
python run.py -i "以太网 2" --logoff

# 检查 Npcap 状态
python run.py --check-npcap
```

省略 `-p/--password` 时会安全提示输入密码。安装后可用 `sysu_netauth` 直接调用：

```powershell
sysu_netauth --list-ifaces
sysu_netauth -i "以太网 2" -u "NetID"
```

### CLI 参数

| 参数             | 说明                        |
| ---------------- | --------------------------- |
| `-i, --iface`    | 指定网卡名称                |
| `-u, --username` | NetID                       |
| `-p, --password` | 密码（省略则交互式输入）    |
| `--timeout`      | 超时秒数（默认 30）         |
| `--logoff`       | 发送 EAPOL-Logoff 注销      |
| `--check-npcap`  | 检查 Npcap 可用性           |
| `--list-ifaces`  | 列出候选网卡及评分          |
| `--probe-iface`  | 探测指定网卡的 EAPOL 服务器 |
| `--client-ip`    | 覆盖自动检测的客户端 IPv4   |

## 工作原理

项目只实现标准 EAP-MD5 路径：

1. 发送 `EAPOL-Start` 广播帧到 PAE 组播 MAC `01:80:c2:00:00:03`
2. 等待交换机回复 `EAP-Request/Identity`
3. 回复 NetID（UTF-8 → GBK 编码）
4. 等待交换机回复 `EAP-Request/MD5-Challenge`
5. 计算 `MD5(EAP identifier || GBK(password) || challenge)` 并回复
6. 根据 `EAP-Success` / `EAP-Failure` / 超时更新状态

不实现厂商私有扩展字段，降低维护和公开发布风险。

## 配置推荐场景

| 服务模式 | 开机启动程序 | 启动后自动认证 | 效果                                    |
| -------- | ------------ | -------------- | --------------------------------------- |
| ✅ 开启  | ✅ 开启      | ✅ 开启        | 服务后台认证 + 开机弹出 GUI             |
| ✅ 开启  | ❌ 关闭      | ✅ 开启        | 服务后台静默认证，完全无托盘/GUI        |
| ✅ 开启  | ✅ 开启      | ❌ 关闭        | 开机弹出 GUI，需手动点击"重新连接"      |
| ❌ 关闭  | ✅ 开启      | 任意           | 开机弹出 GUI，GUI 退出后服务停止认证    |
| ❌ 关闭  | ❌ 关闭      | —              | 程序不自动运行，手动启动后 GUI 决定认证 |

## 构建

额外安装 PyInstaller：

```powershell
pip install -r requirements.txt
pip install PyInstaller
```

```powershell
python scripts\build.py              # 完整构建（EXE → 安装包 → 便携包）
python scripts\build.py --skip-installer  # 仅 EXE + 便携包
```

输出：

```
dist\sysu_netauth\sysu_netauth.exe       # GUI/CLI 程序
dist\sysu_netauth\sysu_netauth_service.exe  # 服务程序
SYSUNetAuth_Setup_v{version}.exe         # 安装包
SYSUNetAuth_Portable_v{version}.zip      # 便携版
```

### 构建脚本

| 脚本                         | 用途                                              |
| ---------------------------- | ------------------------------------------------- |
| `scripts/build.py`           | 完整构建入口：PyInstaller → Inno Setup → 便携 ZIP |
| `scripts/build_exe.py`       | PyInstaller 独立打包（被 build.py 调用）          |
| `scripts/setup.template.iss` | Inno Setup 安装脚本模板                           |
| `scripts/portable/`          | 便携版辅助脚本（安装/卸载服务、启动 GUI）         |

## 测试与验证

### 安装包测试

1. 安装 Npcap，保持默认选项
2. 双击运行 `SYSUNetAuth_Setup_v{version}.exe`
3. 启动 GUI，填写 NetID/密码
4. 点击"立即认证"或重启等待服务自动认证

```powershell
# 检查服务状态
sc query SYSUNetAuth
Get-Content "$env:ProgramData\SYSUNetAuth\status.json" | ConvertFrom-Json
Get-Content "$env:ProgramData\SYSUNetAuth\service.log" -Tail 20
```

### 便携版测试

1. 解压 `SYSUNetAuth_Portable_v{version}.zip`
2. 右键"以管理员身份运行"`Install-Service.cmd`
3. 运行 `Start-GUI.cmd`，填写 NetID/密码
4. 运行 `Service-Status.cmd` 查看服务状态

### 常见问题

**服务 RUNNING 但状态 waiting_network**：未检测到物理有线网卡，或网线未连接。

**service.log 为空**：先查看 `service_bootstrap.log`，它记录服务进程是否被 SCM 正常拉起。

**修改配置后服务未立即响应**：GUI 会自动写 `command.json` 通知服务重新加载。也可手动触发：

```powershell
'{"action":"reload_config"}' | Set-Content "$env:ProgramData\SYSUNetAuth\command.json" -Encoding UTF8
'{"action":"authenticate"}'  | Set-Content "$env:ProgramData\SYSUNetAuth\command.json" -Encoding UTF8
```

## 配置文件

路径：`%ProgramData%\SYSUNetAuth\config.json`

| 字段                   | 类型   | 默认值       | 说明                                |
| ---------------------- | ------ | ------------ | ----------------------------------- |
| `username`             | string | `""`         | NetID                               |
| `password`             | string | `""`         | 明文密码                            |
| `iface`                | string | `""`         | 指定网卡名                          |
| `iface_mode`           | string | `"auto"`     | `"auto"` 自动 / `"manual"` 手动     |
| `auto_auth`            | bool   | `true`       | GUI 启动后自动认证                  |
| `retry_interval`       | int    | `60`         | 失败重试间隔秒数（15–3600）         |
| `close_behavior`       | string | `"minimize"` | `"minimize"` 最小化 / `"quit"` 退出 |
| `close_behavior_asked` | bool   | `false`      | 是否已询问过关闭行为                |
| `service_mode`         | bool   | `true`       | 服务独立运行，退出 GUI 不影响认证   |
| `launch_gui_on_login`  | bool   | `false`      | 用户登录后启动 GUI                  |
| `hide_window_on_login` | bool   | `true`       | 仅 --startup 时启动后隐藏窗口       |
| `desktop_notify`       | bool   | `true`       | 状态变化时弹出桌面通知              |
| `last_success_iface`   | string | `""`         | 上次成功网卡名（自动缓存）          |
| `last_success_mac`     | string | `""`         | 上次成功 MAC（自动缓存）            |

> **安全说明**：密码以明文保存。请不要提交配置文件到仓库。

## 进程间通信

三个 JSON 文件位于 `%ProgramData%\SYSUNetAuth\`，均使用原子写入：

| 文件           | 写入者     | 读取者     | 用途                                              |
| -------------- | ---------- | ---------- | ------------------------------------------------- |
| `config.json`  | GUI / 服务 | 服务 / GUI | 账号、密码、网卡、策略                            |
| `status.json`  | 服务       | GUI / CLI  | 当前认证状态（state/message/iface/ip）            |
| `command.json` | GUI        | 服务       | 命令：`authenticate` / `logoff` / `reload_config` |

## 相关链接

| 资源                      | 地址                                                                                                 |
| ------------------------- | ---------------------------------------------------------------------------------------------------- |
| 网络与信息中心            | [inc.sysu.edu.cn](https://inc.sysu.edu.cn)                                                           |
| 统一身份认证 / NetID 激活 | [cas.sysu.edu.cn](https://cas.sysu.edu.cn)                                                           |
| 网费在线缴费              | [netpay.sysu.edu.cn](http://netpay.sysu.edu.cn)                                                      |
| 信息技术服务帮助台        | helpdesk@mail.sysu.edu.cn / 020-84036866                                                             |
| 有线网络接入说明          | [inc.sysu.edu.cn/service/wired-network-access](https://inc.sysu.edu.cn/service/wired-network-access) |

## 维护者

- [架构说明](docs/ARCHITECTURE.md) — 面向维护者的完整架构文档
