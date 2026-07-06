# 续期 Bug 根因与正确修复摘要

> 本文档记录 2026-07-06 调试中发现的关键问题根因和正确方案，作为后续代码清理的判断依据。

---

## 核心发现

### 1. 交换机的定时 EAP-Request/Identity 是握手保活，不是重认证

- **实测频率**：精确每 120 秒
- **性质**：H3C/锐捷风格在线 handshake keepalive probe，交换机期望客户端回一帧确认在线即可
- **不期望的行为**：客户端不该触发完整 EAP-MD5 重认证，不该发 EAPOL-Start

### 2. 正确响应方式

```
收到 EAP-Request/Identity → inline 回 EAP-Response/Identity
  dst = 01:80:c2:00:00:03（PAE 组播，非交换机单播）
  identifier = 复用 Request 的 identifier
  identity = NetID 原文
  不回 EAPOL-Start，不退监听，不改变认证状态
```

### 3. RenewListener 必须是常驻响应器，不是一次性触发器

```
旧（错误）：嗅探到第一帧 → 设事件 → 退出 → 引擎重认证 → 失败循环
新（正确）：嗅探到帧 → inline 回复 → 继续嗅探 → 不退出一
          仅在 EAP-Failure 时通知引擎重认证
```

### 4. 其他重要规则

| 规则                       | 说明                                    |
| -------------------------- | --------------------------------------- |
| EAP-Success ≠ 网络可用     | 须 ping 验证连通性后再设 AUTHENTICATED  |
| 出站 EAPOL 用 PAE 组播     | `01:80:c2:00:00:03`，不用交换机单播 MAC |
| 通知需冷却                 | 防止显示器关闭期间积累弹窗轰炸          |
| EAPOL-Start 仅用于初始认证 | 在线期间不主动发 Start                  |

---

## 对应的代码位置

| 文件                                   | 关键改动                                                            |
| -------------------------------------- | ------------------------------------------------------------------- |
| `service/engine.py` RenewListener      | 常驻 sniff 循环，inline sendp 回复，failure_event 替代 reauth_event |
| `service/engine.py` \_on_auth_finished | 移除续期失败分支，EAP-Success 后先 ping 再设 AUTHENTICATED          |
| `app/tray.py`                          | NOTIFY_COOLDOWN 字典，\_notify_state 方法                           |
