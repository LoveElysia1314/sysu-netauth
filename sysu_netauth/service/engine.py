from __future__ import annotations

import logging
import subprocess
import threading
import time
from dataclasses import replace
from enum import Enum

import psutil
from scapy.all import get_if_hwaddr, sendp, sniff

from sysu_netauth.core.config import (
    AppConfig,
    load_config,
    read_command,
    save_config,
    update_status,
    utc_now_iso,
)
from sysu_netauth.core.eapol import (
    PAE_GROUP,
    AuthOptions,
    AuthResult,
    AuthStatus,
    authenticate,
    build_identity_response,
    build_md5_response,
    parse_eapol,
    send_logoff,
)
from sysu_netauth.core.interfaces import (
    _adapter_metadata,
    find_iface_by_mac,
    get_interface_network_info,
    list_auth_candidate_interfaces,
    pick_best_candidate,
)
from sysu_netauth.core.npcap import has_npcap

MAX_RETRIES = 5
AUTH_TIMEOUT_SECONDS = 10
IFACE_WATCH_INTERVAL = 5.0
CONFIG_RELOAD_INTERVAL = 3.0
STARTUP_GRACE_SECONDS = 60.0
STARTUP_RETRY_INTERVALS = (3, 5, 8, 13, 21)
RENEW_GRACE_SECONDS = 30.0


class ServiceState(str, Enum):
    IDLE = "idle"
    AUTHENTICATING = "authenticating"
    AUTHENTICATED = "authenticated"
    FAILED = "failed"
    STOPPED = "stopped"


# IP 连通性验证目标（国内通用 DNS + 腾讯云 Anycast）
_CONNECTIVITY_TARGETS = ("119.29.29.29", "114.114.114.114")


class RenewListener(threading.Thread):
    """常驻 EAPOL 响应器。

    认证成功后持续嗅探 EAPOL 帧：
    - EAP-Request/Identity → inline 回复 Identity（握手保活）
    - EAP-Request/MD5      → inline 回复 MD5（真实重认证）
    - EAP-Failure          → 通知引擎会话失效
    - EAP-Success          → 记录

    不在任何 Request 后退出一—交换机可能在在线期间发送多轮探测。
    """

    def __init__(
        self,
        iface: str,
        stop_event: threading.Event,
        failure_event: threading.Event,
        logger: logging.Logger,
        username: str,
        password: str,
        grace_seconds: float = 0.0,
    ) -> None:
        super().__init__(name=f"RenewListener-{iface}", daemon=True)
        self.iface = iface
        self.stop_event = stop_event
        self.failure_event = failure_event
        self.logger = logger
        self._username = username
        self._password = password
        self._grace_seconds = grace_seconds

    def run(self) -> None:
        try:
            src_mac = get_if_hwaddr(self.iface)
        except Exception as exc:
            self.logger.warning(
                "RenewListener cannot get MAC for %s: %s", self.iface, exc
            )
            return

        ignore_until = time.monotonic() + self._grace_seconds
        while not self.stop_event.is_set():
            try:
                packets = sniff(
                    iface=self.iface,
                    filter="ether proto 0x888e",
                    timeout=1,
                    count=1,
                )
                if not packets or self.stop_event.is_set():
                    continue
                if time.monotonic() < ignore_until:
                    continue

                raw = bytes(packets[0])
                parsed = parse_eapol(raw)
                if not parsed:
                    continue

                if parsed.code == 1:  # EAP_REQUEST
                    if parsed.eap_type == 1:  # Identity
                        resp = build_identity_response(
                            src_mac, PAE_GROUP, parsed.identifier, self._username
                        )
                        sendp(resp, iface=self.iface, verbose=False)
                        self.logger.debug("handshake probe replied on %s", self.iface)
                    elif parsed.eap_type == 4 and parsed.data:  # MD5
                        challenge_len = parsed.data[0]
                        challenge = parsed.data[1 : 1 + challenge_len]
                        resp = build_md5_response(
                            src_mac,
                            PAE_GROUP,
                            parsed.identifier,
                            self._username,
                            self._password,
                            challenge,
                        )
                        sendp(resp, iface=self.iface, verbose=False)
                        self.logger.info("reauth MD5 replied on %s", self.iface)
                elif parsed.code == 2:  # EAP_RESPONSE — 忽略（可能是自己发的）
                    pass
                elif parsed.code == 3:  # EAP_SUCCESS
                    self.logger.info("EAP success on %s", self.iface)
                elif parsed.code == 4:  # EAP_FAILURE
                    self.logger.warning("EAP failure on %s — session lost", self.iface)
                    self.failure_event.set()
                    return
            except Exception as exc:
                if not self.stop_event.is_set():
                    self.logger.warning("renew listener error: %s", exc)
                    self.stop_event.wait(1)


class AuthServiceEngine:
    def __init__(self, stop_event: threading.Event | None = None) -> None:
        self.stop_event = stop_event or threading.Event()
        self.logger = logging.getLogger("sysu_netauth.service")
        self.config: AppConfig = load_config()
        self.state = ServiceState.IDLE
        self._status_message = ""
        self._ni: dict[str, str] = {}  # iface/mac/ipv4/gateway/dns/driver
        self._lock = threading.RLock()
        self._auth_thread: threading.Thread | None = None
        self._auth_candidate_queue: list[str] = []
        self._prev_iface_up: bool | None = None
        self._prev_media: bool | None = None
        self._retry_count = 0
        self._next_retry_at = 0.0
        self._startup_grace_until = 0.0
        self._startup_retry_index = 0
        self._renew_listener: RenewListener | None = None
        self._renew_failure_event = threading.Event()
        self._authenticated_at: str | None = None
        self._manual_disconnect = False
        # 周期性任务 (interval, next_run, callback)
        self._timers: list[list] = [
            [CONFIG_RELOAD_INTERVAL, 0.0, self.reload_config],
            [IFACE_WATCH_INTERVAL, 0.0, self._check_iface_status],
            [IFACE_WATCH_INTERVAL, 0.0, self._heartbeat],
        ]

    @property
    def authenticated(self) -> bool:
        return self.state == ServiceState.AUTHENTICATED

    def run(self) -> None:
        self.logger.info("service engine starting")
        if not has_npcap():
            self._set_status(ServiceState.FAILED, "Npcap 未安装")
        else:
            self._schedule_startup_auth()

        while not self.stop_event.is_set():
            now = time.monotonic()
            self._handle_command()
            # 周期性任务
            for t in self._timers:
                interval, next_run, cb = t
                if now >= next_run:
                    cb()
                    t[1] = now + interval
            # 事件驱动
            if self._renew_failure_event.is_set():
                self._renew_failure_event.clear()
                self._cancel_renew()
                self._set_status(ServiceState.IDLE, "会话失效，重新认证")
                self._start_auth_flow(manual=False, force=True)
            # 一次性重试定时器
            if self._next_retry_at and now >= self._next_retry_at:
                self._next_retry_at = 0.0
                self._do_retry()
            self.stop_event.wait(1)

        self.shutdown()

    def _heartbeat(self) -> None:
        """刷新 status.json 的 updated_at，防止 GUI 判定 stale。"""
        self._set_status(self.state, self._status_message or self.state.value)

    def shutdown(self) -> None:
        self.logger.info("service engine stopping")
        self._cancel_retry()
        self._cancel_renew()
        if self._auth_thread:
            self._auth_stop.set()
            thread = self._auth_thread
            if thread.is_alive():
                thread.join(timeout=4)
        self._set_status(ServiceState.STOPPED, "服务已停止")

    def reload_config(self) -> None:
        with self._lock:
            self.config = load_config()

    def _handle_command(self) -> None:
        action = read_command()
        if not action:
            return
        self.logger.info("command received: %s", action)
        if action == "authenticate":
            self.reload_config()
            self._manual_disconnect = False
            self._cancel_retry()
            self._start_auth_flow(manual=True, force=True)
        elif action == "logoff":
            self.reload_config()
            self.logoff()
        elif action == "reload_config":
            self.reload_config()
        else:
            self.logger.warning("unknown command ignored: %s", action)

    def _set_status(
        self,
        state: ServiceState,
        message: str,
        *,
        notify: bool = True,
        iface: str = "",
        mac: str = "",
        ipv4: str = "",
        gateway: str = "",
        dns: str = "",
        driver: str = "",
    ) -> None:
        self.state = state
        self._status_message = message
        # 合并网络信息（空值不覆盖 → 心跳复用旧值）
        for key, val in (
            ("iface", iface),
            ("mac", mac),
            ("ipv4", ipv4),
            ("gateway", gateway),
            ("dns", dns),
            ("driver", driver),
        ):
            if val:
                self._ni[key] = val
        if state != ServiceState.AUTHENTICATED:
            self._ni.clear()
        if not notify:
            return
        try:
            update_status(
                state.value,
                message,
                iface=self._ni.get("iface", "") or self.config.iface,
                mac=self._ni.get("mac", ""),
                ipv4=self._ni.get("ipv4", ""),
                gateway=self._ni.get("gateway", ""),
                dns=self._ni.get("dns", ""),
                driver=self._ni.get("driver", ""),
                authenticated_at=(
                    self._authenticated_at
                    if state == ServiceState.AUTHENTICATED
                    else None
                ),
            )
        except Exception as exc:
            self.logger.error(
                "write status failed (state=%s, msg=%s): %s", state, message, exc
            )

    def _has_auth_credentials(self) -> bool:
        return bool(self.config.username) and bool(self.config.password)

    def _has_any_media(self) -> bool:
        """Return True if any physical Ethernet interface has a live link."""
        from sysu_netauth.core.interfaces import list_ethernet_candidates

        candidates = list_ethernet_candidates()
        return any(c.has_media for c in candidates) if candidates else False

    def _schedule_startup_auth(self) -> None:
        if not self.config.auto_auth:
            self._set_status(ServiceState.IDLE, "待命")
            return
        if not self._has_auth_credentials():
            self._set_status(ServiceState.FAILED, "需要配置 NetID 和密码")
            return
        self._startup_grace_until = time.monotonic() + STARTUP_GRACE_SECONDS
        self._startup_retry_index = 0
        self._set_status(ServiceState.IDLE, "准备自动认证")
        self._start_auth_flow(manual=False)

    def _resolve_saved_iface(self) -> str | None:
        supported = {candidate.name for candidate in list_auth_candidate_interfaces()}
        if self.config.iface and self.config.iface in supported:
            return self.config.iface
        if self.config.last_success_mac:
            matched = find_iface_by_mac(self.config.last_success_mac)
            if matched:
                return matched.name
        return None

    def _auto_auth_candidates(self) -> list[str]:
        candidates = [c for c in list_auth_candidate_interfaces() if c.is_up]
        by_name = {c.name: c for c in candidates}
        ordered: list[str] = []

        def add(name: str | None) -> None:
            if name and name in by_name and name not in ordered:
                ordered.append(name)

        if self.config.last_success_mac:
            matched = find_iface_by_mac(self.config.last_success_mac)
            add(matched.name if matched else None)
        add(self.config.iface)
        for candidate in candidates:
            add(candidate.name)
        return ordered

    def _auth_candidates(self) -> list[str]:
        if self.config.iface_mode == "manual":
            iface = self._resolve_saved_iface()
            return [iface] if iface else []
        return self._auto_auth_candidates()

    def _start_auth_flow(
        self,
        manual: bool,
        force: bool = False,
    ) -> None:
        if self._manual_disconnect and not (manual or force):
            self._set_status(ServiceState.IDLE, "已断开")
            return
        if self.authenticated and not (manual or force):
            self._schedule_renew()
            return
        if self._auth_thread and self._auth_thread.is_alive():
            self.logger.info("authentication already running")
            return
        self._cancel_retry()
        self._cancel_renew()
        if not self._has_auth_credentials():
            self._set_status(ServiceState.FAILED, "需要配置 NetID 和密码")
            return
        if self.config.iface_mode == "manual" and not self.config.iface:
            self._set_status(ServiceState.FAILED, "请先选择网卡")
            return
        self._auth_candidate_queue = self._auth_candidates()
        if not self._auth_candidate_queue:
            if not self._has_any_media():
                # 启动宽限期内不通知 GUI（网卡驱动可能尚未加载完毕），
                # 心跳将在几秒内把 IDLE 写入磁盘。宽限期外则立即告知用户。
                self._set_status(
                    ServiceState.IDLE,
                    "未检测到已连接网线",
                    notify=not self._in_startup_grace(),
                )
                return
            else:
                # 有物理链路但网卡未就绪（如 DHCP 未完成），调度重试。
                # _schedule_retry 不写 status.json，由心跳兜底。
                self._schedule_retry()
            return
        self._authenticate_next_candidate()

    def _authenticate_next_candidate(self) -> None:
        if not self._auth_candidate_queue:
            self._set_status(ServiceState.IDLE, "所有候选网卡认证未成功")
            return
        iface = self._auth_candidate_queue.pop(0)
        self._auth_stop = threading.Event()
        # 续期认证时不改变状态，避免 GUI 闪烁
        if not self.authenticated:
            self._set_status(
                ServiceState.AUTHENTICATING, f"正在认证：{iface}", iface=iface
            )
        self._auth_thread = threading.Thread(
            target=self._run_authentication,
            args=(iface, self._auth_stop),
            name=f"AuthWorker-{iface}",
            daemon=True,
        )
        self._auth_thread.start()

    def _run_authentication(
        self,
        iface: str,
        auth_stop: threading.Event,
    ) -> None:
        try:
            result = authenticate(
                AuthOptions(
                    iface=iface,
                    username=self.config.username,
                    password=self.config.password,
                    timeout=AUTH_TIMEOUT_SECONDS,
                ),
                progress=lambda _status, message: self.logger.info(message),
                should_stop=auth_stop.is_set,
            )
        except Exception as exc:
            result = AuthResult(AuthStatus.AUTH_FAILED, str(exc), iface)
        self._on_auth_finished(result)

    def _on_auth_finished(self, result: AuthResult) -> None:
        if self.stop_event.is_set() or self.state == ServiceState.STOPPED:
            return
        if self._manual_disconnect:
            self._authenticated_at = None
            self._set_status(ServiceState.IDLE, "已断开", iface=result.iface)
            return
        status = result.status
        if status == AuthStatus.AUTH_SUCCESS:
            self._startup_grace_until = 0.0
            self._startup_retry_index = 0
            self._save_success_iface(result)
            self._cancel_retry()
            self._retry_count = 0
            if not self.authenticated:
                self._set_status(
                    ServiceState.AUTHENTICATING,
                    "已认证，验证连通性...",
                    iface=result.iface,
                    mac=result.mac or "",
                    ipv4=result.ip or "",
                )
            threading.Thread(
                target=self._verify_connectivity,
                args=(result,),
                daemon=True,
            ).start()
            return

        if status != AuthStatus.NO_NPCAP and self._auth_candidate_queue:
            self._authenticate_next_candidate()
            return

        self._authenticated_at = None
        if status != AuthStatus.NO_NPCAP:
            # 还有重试机会 → 不写中间态到 status.json，避免 GUI 状态卡片闪烁。
            # _schedule_retry() 会在耗尽重试次数后写入终态 FAILED。
            if (
                status == AuthStatus.TIMEOUT
                and self._in_startup_grace()
                and result.iface
            ):
                self._send_cleanup_logoff(result.iface)
            self._schedule_retry()
            return
        self._set_status(ServiceState.FAILED, result.message, iface=result.iface)

    def _ping_ok(self) -> bool:
        """验证外网连通性。不绑定源 IP，避免 DHCP 未完成时因 IP 未生效而失败。"""
        try:
            for target in _CONNECTIVITY_TARGETS:
                r = subprocess.run(
                    ["ping", "-n", "1", "-w", "3000", target],
                    capture_output=True,
                    timeout=4,
                    creationflags=0x08000000,
                )
                if r.returncode == 0:
                    return True
        except Exception:
            pass
        return False

    def _verify_connectivity(self, result: AuthResult) -> None:
        """后台线程：验证外网连通性。成功后设 AUTHENTICATED 并启动续期监听。"""
        ip = result.ip or ""
        self.stop_event.wait(5)
        for attempt in range(3):
            if self.stop_event.is_set():
                return
            if self._ping_ok():
                self._on_connectivity_result(True, result)
                return
            self.logger.warning(
                "connectivity check #%d failed on %s / %s",
                attempt + 1,
                result.iface,
                ip,
            )
            if attempt < 2:
                self.stop_event.wait(10)
        self.logger.warning("connectivity check exhausted on %s / %s", result.iface, ip)
        self._on_connectivity_result(False, result)

    def _on_connectivity_result(self, success: bool, result: AuthResult) -> None:
        with self._lock:
            if success:
                self._authenticated_at = utc_now_iso()
                self.logger.info(
                    "connectivity OK on %s / %s", result.iface, result.ip or ""
                )
                # 每次重新认证后刷新完整网络信息（含网关/DNS/驱动描述）
                iface = result.iface or self.config.iface
                net_info = get_interface_network_info(iface) if iface else None
                # 驱动描述从 _adapter_metadata 获取
                _driver = ""
                if iface:
                    meta = _adapter_metadata().get(iface, {})
                    _driver = str(meta.get("description") or "")
                self._set_status(
                    ServiceState.AUTHENTICATED,
                    "已认证",
                    iface=iface,
                    mac=result.mac or "",
                    ipv4=(
                        net_info.ipv4
                        if net_info and net_info.ipv4
                        else (result.ip or "")
                    ),
                    gateway=net_info.gateway if net_info else "",
                    dns=(
                        ", ".join(net_info.dns[:2]) if net_info and net_info.dns else ""
                    ),
                    driver=_driver,
                )
                self._schedule_renew()
            else:
                # 连通性检查失败：退避重试
                # 续期场景下也降级状态，确保 _do_retry 能触发重新认证
                self._set_status(ServiceState.IDLE, "认证成功但无法访问网络")
                self._retry_count = 0
                self._next_retry_at = time.monotonic() + self.config.retry_interval

    def _save_success_iface(self, result: AuthResult) -> None:
        iface = result.iface or self.config.iface
        if not iface:
            return
        try:
            mac = result.mac or get_if_hwaddr(iface)
            self.config = replace(
                self.config,
                iface=iface,
                last_success_mac=mac,
            )
            save_config(self.config)
        except Exception as exc:
            self.logger.warning("failed to save success iface: %s", exc)

    def _send_cleanup_logoff(self, iface: str) -> None:
        if not iface:
            return
        try:
            send_logoff(iface)
        except Exception as exc:
            self.logger.warning("cleanup logoff failed: %s", exc)

    def _schedule_retry(self) -> None:
        if self.authenticated:
            return
        if self._in_startup_grace():
            index = min(self._startup_retry_index, len(STARTUP_RETRY_INTERVALS) - 1)
            interval = STARTUP_RETRY_INTERVALS[index]
            self._startup_retry_index += 1
            self._next_retry_at = time.monotonic() + interval
            self.state = ServiceState.IDLE
            self._status_message = "认证未成功，等待重试"
            return
        if self._retry_count >= MAX_RETRIES:
            self._set_status(ServiceState.FAILED, "多次认证失败，请检查 NetID 或网络")
            return
        self._retry_count += 1
        self._next_retry_at = time.monotonic() + self.config.retry_interval
        # 不写 status.json，仅更新内部状态。心跳（每 5s）会自然
        # 把 IDLE 同步到磁盘，避免中间态造成 GUI 状态卡片闪烁。
        self.state = ServiceState.IDLE
        self._status_message = "认证未成功，等待重试"

    def _cancel_retry(self) -> None:
        self._next_retry_at = 0.0

    def _in_startup_grace(self) -> bool:
        return time.monotonic() < self._startup_grace_until

    def _do_retry(self) -> None:
        if self.authenticated:
            return
        self._start_auth_flow(manual=False)

    def _schedule_renew(self) -> None:
        self._cancel_renew()
        iface = self.config.iface
        if not iface:
            return
        self._renew_stop = threading.Event()
        self._renew_listener = RenewListener(
            iface,
            self._renew_stop,
            self._renew_failure_event,
            self.logger,
            self.config.username,
            self.config.password,
            grace_seconds=RENEW_GRACE_SECONDS,
        )
        self._renew_listener.start()

    def _cancel_renew(self) -> None:
        if self._renew_listener and self._renew_listener.is_alive():
            self._renew_stop.set()
            self._renew_listener.join(timeout=1.5)
        self._renew_listener = None

    def _check_iface_status(self) -> None:
        # ── Phase 1: System-wide physical media edge detection ──
        current_media = self._has_any_media()

        if self._prev_media is not None and current_media != self._prev_media:
            if current_media:
                # 网线插入 → 唤醒
                self._retry_count = 0
                self._manual_disconnect = False
                self._prev_iface_up = None  # 让 Phase 3 重新初始化边沿状态
                self.logger.info("physical media detected — resuming")
                if self.config.auto_auth:
                    self._set_status(ServiceState.IDLE, "网线已连接")
                    self._start_auth_flow(manual=False)
                else:
                    self._set_status(ServiceState.IDLE, "待命")
            else:
                self._cancel_renew()
                self._cancel_retry()
                self._authenticated_at = None
                self._set_status(ServiceState.IDLE, "未检测到网线")
                self.logger.info("all physical media gone — idling")
            self._prev_media = current_media
            return

        if self._prev_media is None:
            self._prev_media = current_media

        # ── Phase 2: 无物理链路时静默等待 ──
        if not current_media:
            return

        # ── Phase 3: Original interface up/down edge detection ──
        iface = self.config.iface
        if not iface:
            best = pick_best_candidate()
            if best and best.is_up:
                self.config = replace(self.config, iface=best.name)
                save_config(self.config)
                iface = best.name
                if self.config.auto_auth and not self._manual_disconnect:
                    self._start_auth_flow(manual=False)
            else:
                if not best:
                    self._prev_iface_up = False
                    self._set_status(ServiceState.IDLE, "等待有线网卡")
                return

        stats = psutil.net_if_stats()
        if iface in stats:
            current_up = stats[iface].isup
        elif self.config.last_success_mac:
            matched = find_iface_by_mac(self.config.last_success_mac)
            if matched:
                self.config = replace(self.config, iface=matched.name)
                save_config(self.config)
                current_up = matched.is_up
            else:
                current_up = False
        else:
            current_up = False

        if self._prev_iface_up is None:
            self._prev_iface_up = current_up
            return
        if current_up == self._prev_iface_up:
            return

        if current_up:
            self._retry_count = 0
            if self.authenticated:
                self._set_authenticated_status()
                self._schedule_renew()
            else:
                self._set_status(ServiceState.IDLE, "网卡已就绪")
            if (
                self.config.auto_auth
                and not self.authenticated
                and not self._manual_disconnect
            ):
                self._start_auth_flow(manual=False)
        else:
            self._authenticated_at = None
            self._set_status(ServiceState.IDLE, "网卡已断开")
            self._cancel_renew()
            if self.config.auto_auth and not self._manual_disconnect:
                failover = pick_best_candidate()
                if failover and failover.is_up and failover.name != iface:
                    self.config = replace(
                        self.config,
                        iface=failover.name,
                        iface_mode="auto",
                    )
                    save_config(self.config)
                    current_up = True
                    self._retry_count = 0
                    self._start_auth_flow(manual=False)
            self._cancel_retry()
        self._prev_iface_up = current_up

    def _set_authenticated_status(self) -> None:
        iface = self.config.iface
        mac = ""
        ipv4 = ""
        gateway = ""
        dns = ""
        try:
            if iface:
                mac = get_if_hwaddr(iface)
                net_info = get_interface_network_info(iface)
                ipv4 = net_info.ipv4 or ""
                gateway = net_info.gateway or ""
                dns = ", ".join(net_info.dns[:2]) if net_info.dns else ""
        except (OSError, RuntimeError):
            self.logger.warning("failed to get MAC/IP for %s", iface, exc_info=True)
        self._set_status(
            ServiceState.AUTHENTICATED,
            "已认证",
            iface=iface,
            mac=mac,
            ipv4=ipv4,
            gateway=gateway,
            dns=dns,
        )

    def logoff(self) -> None:
        self._manual_disconnect = True
        self._cancel_retry()
        self._cancel_renew()
        if self._auth_thread:
            self._auth_stop.set()
            thread = self._auth_thread
            if thread.is_alive():
                thread.join(timeout=4)
        iface = self.config.iface or self._resolve_saved_iface()
        if not iface:
            self._authenticated_at = None
            self._set_status(ServiceState.IDLE, "已断开")
            return
        try:
            mac = send_logoff(iface)
            self._authenticated_at = None
            self._set_status(ServiceState.IDLE, "已断开", iface=iface, mac=mac)
        except Exception as exc:
            self._set_status(ServiceState.FAILED, f"注销失败：{exc}", iface=iface)
