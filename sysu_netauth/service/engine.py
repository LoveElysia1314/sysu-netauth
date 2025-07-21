from __future__ import annotations

import logging
import subprocess
import threading
import time
from dataclasses import replace
from enum import Enum
from typing import Callable

import psutil
from scapy.all import get_if_hwaddr, sniff

from sysu_netauth.core.config import AppConfig
from sysu_netauth.core.eapol import (
    AuthOptions,
    AuthResult,
    AuthStatus,
    authenticate,
    detect_reauth_trigger,
    send_logoff,
)
from sysu_netauth.core.interfaces import (
    find_iface_by_mac,
    get_interface_network_info,
    list_auth_candidate_interfaces,
    pick_best_candidate,
)
from sysu_netauth.core.npcap import has_npcap
from sysu_netauth.core.shared_store import (
    ensure_shared_config,
    read_command,
    save_shared_config,
    update_status,
    utc_now_iso,
)

MAX_RETRIES = 5
AUTH_TIMEOUT_SECONDS = 10
IFACE_WATCH_INTERVAL = 5.0
CONFIG_RELOAD_INTERVAL = 3.0
STARTUP_GRACE_SECONDS = 60.0
STARTUP_RETRY_INTERVALS = (3, 5, 8, 13, 21)
RENEW_GRACE_SECONDS = 30.0


class ServiceState(str, Enum):
    STARTING = "starting"
    IDLE = "idle"
    WAITING_NETWORK = "waiting_network"
    AUTHENTICATING = "authenticating"
    AUTHENTICATED = "authenticated"
    FAILED = "failed"
    STOPPED = "stopped"


# IP 连通性验证目标（国内通用 DNS + 腾讯云 Anycast）
_CONNECTIVITY_TARGETS = ("119.29.29.29", "114.114.114.114")


class RenewListener(threading.Thread):
    def __init__(
        self,
        iface: str,
        stop_event: threading.Event,
        reauth_event: threading.Event,
        logger: logging.Logger,
        trigger_callback: Callable[[bytes], None] | None = None,
        grace_seconds: float = 0.0,
    ) -> None:
        super().__init__(name=f"RenewListener-{iface}", daemon=True)
        self.iface = iface
        self.stop_event = stop_event
        self.reauth_event = reauth_event
        self.logger = logger
        self.trigger_callback = trigger_callback
        self.grace_seconds = grace_seconds

    def run(self) -> None:
        ignore_until = time.monotonic() + self.grace_seconds
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
                if detect_reauth_trigger(raw):
                    self.logger.info("reauth trigger detected on %s", self.iface)
                    if self.trigger_callback:
                        self.trigger_callback(raw)
                    self.reauth_event.set()
                    return
            except Exception as exc:
                if not self.stop_event.is_set():
                    self.logger.warning("renew listener error: %s", exc)
                    self.stop_event.wait(1)


class AuthServiceEngine:
    def __init__(self, stop_event: threading.Event | None = None) -> None:
        self.stop_event = stop_event or threading.Event()
        self.logger = logging.getLogger("sysu_netauth.service")
        self.config: AppConfig = ensure_shared_config()
        self.state = ServiceState.STARTING
        self._status_message = ""
        self._status_iface = ""
        self._status_mac = ""
        self._status_ipv4 = ""
        self._lock = threading.RLock()
        self._auth_thread: threading.Thread | None = None
        self._auth_stop = threading.Event()
        self._auth_candidate_queue: list[str] = []
        self._auth_flow_manual = False
        self._prev_iface_up: bool | None = None
        self._retry_count = 0
        self._next_retry_at = 0.0
        self._startup_grace_until = 0.0
        self._startup_retry_index = 0
        self._renew_stop = threading.Event()
        self._renew_listener: RenewListener | None = None
        self._reauth_event = threading.Event()
        self._reauth_packet: bytes | None = None
        self._next_iface_check_at = 0.0
        self._next_config_reload_at = 0.0
        self._next_heartbeat_at = IFACE_WATCH_INTERVAL
        self._reauth_retry_at = 0.0
        self._authenticated_at: str | None = None
        self._manual_disconnect = False

    @property
    def authenticated(self) -> bool:
        return self.state == ServiceState.AUTHENTICATED

    def run(self) -> None:
        self.logger.info("service engine starting")
        self._set_status(ServiceState.STARTING, "服务启动中")
        if not has_npcap():
            self._set_status(ServiceState.FAILED, "Npcap 未安装")
        else:
            self._schedule_startup_auth()

        while not self.stop_event.is_set():
            now = time.monotonic()
            self._handle_command()
            if now >= self._next_config_reload_at:
                self.reload_config()
                self._next_config_reload_at = now + CONFIG_RELOAD_INTERVAL
            if now >= self._next_iface_check_at:
                self._check_iface_status()
                self._next_iface_check_at = now + IFACE_WATCH_INTERVAL
            if self._reauth_event.is_set():
                self._reauth_event.clear()
                self._safe_reauth()
            if self._reauth_retry_at and now >= self._reauth_retry_at:
                self._reauth_retry_at = 0.0
                self._start_auth_flow(manual=False, force=True)
            if self._next_retry_at and now >= self._next_retry_at:
                self._next_retry_at = 0.0
                self._do_retry()
            # 通用心跳：每 5 秒刷新 updated_at，防止 GUI 判 stale
            if now >= self._next_heartbeat_at:
                self._set_status(self.state, self._status_message or self.state.value)
                self._next_heartbeat_at = now + IFACE_WATCH_INTERVAL
            self.stop_event.wait(1)

        self.shutdown()

    def shutdown(self) -> None:
        self.logger.info("service engine stopping")
        self._cancel_retry()
        self._cancel_renew()
        self._auth_stop.set()
        thread = self._auth_thread
        if thread and thread.is_alive():
            thread.join(timeout=4)
        self._set_status(ServiceState.STOPPED, "服务已停止")

    def reload_config(self) -> None:
        with self._lock:
            self.config = ensure_shared_config()

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
            self._set_status(self.state, "配置已重新加载")
        else:
            self.logger.warning("unknown command ignored: %s", action)

    def _set_status(
        self,
        state: ServiceState,
        message: str,
        *,
        iface: str = "",
        mac: str = "",
        ipv4: str = "",
    ) -> None:
        self.state = state
        self._status_message = message
        # 缓存接口信息；心跳调用时不传参数 → 复用上次的有效值
        if iface:
            self._status_iface = iface
        if mac:
            self._status_mac = mac
        if ipv4:
            self._status_ipv4 = ipv4
        # 非 AUTHENTICATED 状态 → 清除缓存的 mac/ipv4
        if state != ServiceState.AUTHENTICATED:
            self._status_mac = ""
            self._status_ipv4 = ""
        update_status(
            state.value,
            message,
            iface=self._status_iface or self.config.iface,
            mac=self._status_mac,
            ipv4=self._status_ipv4,
            authenticated_at=(
                self._authenticated_at if state == ServiceState.AUTHENTICATED else None
            ),
        )

    def _has_auth_credentials(self) -> bool:
        return bool(self.config.username) and bool(self.config.password)

    def _schedule_startup_auth(self) -> None:
        if not self.config.auto_auth:
            self._set_status(ServiceState.IDLE, "待命")
            return
        if not self._has_auth_credentials():
            self._set_status(ServiceState.FAILED, "需要配置 NetID 和密码")
            return
        self._startup_grace_until = time.monotonic() + STARTUP_GRACE_SECONDS
        self._startup_retry_index = 0
        self._set_status(ServiceState.WAITING_NETWORK, "准备自动认证")
        self._start_auth_flow(manual=False)

    def _auth_options(self, iface: str) -> AuthOptions:
        if not self.config.username:
            raise RuntimeError("请先填写NetID")
        if not self.config.password:
            raise RuntimeError("请先填写密码")
        return AuthOptions(
            iface=iface,
            username=self.config.username,
            password=self.config.password,
            timeout=AUTH_TIMEOUT_SECONDS,
        )

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
        *,
        reauth_packet: bytes | None = None,
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
        self._auth_flow_manual = manual
        if not self._auth_candidate_queue:
            self._set_status(ServiceState.WAITING_NETWORK, "无已连接有线网卡")
            # 开机时网卡可能尚未就绪，调度重试（_check_iface_status 边沿检测
            # 只在网卡状态变化时触发，不加 _schedule_retry 可能永远不再检查）
            self._schedule_retry()
            return
        self._authenticate_next_candidate(reauth_packet=reauth_packet)

    def _authenticate_next_candidate(self, reauth_packet: bytes | None = None) -> None:
        if not self._auth_candidate_queue:
            self._set_status(ServiceState.WAITING_NETWORK, "所有候选网卡认证未成功")
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
            kwargs={"reauth_packet": reauth_packet},
            daemon=True,
        )
        self._auth_thread.start()

    def _run_authentication(
        self,
        iface: str,
        auth_stop: threading.Event,
        *,
        reauth_packet: bytes | None = None,
    ) -> None:
        try:
            result = authenticate(
                self._auth_options(iface),
                progress=lambda _status, message: self.logger.info(message),
                should_stop=auth_stop.is_set,
                send_start=reauth_packet is None,
                initial_packet=reauth_packet,
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
            # 直接进入 AUTHENTICATED，后台静默验证连通性（不改变状态消息）
            self._set_status(
                ServiceState.AUTHENTICATED,
                "已认证",
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

        # 续期失败：静默重试，不改变状态、不发送 logoff、不中断当前会话
        if self.authenticated and status in (
            AuthStatus.TIMEOUT,
            AuthStatus.AUTH_FAILED,
        ):
            self._reauth_retry_at = time.monotonic() + self.config.retry_interval
            return

        if status != AuthStatus.NO_NPCAP and self._auth_candidate_queue:
            self._authenticate_next_candidate()
            return

        self._authenticated_at = None
        if not self._auth_flow_manual and status != AuthStatus.NO_NPCAP:
            self._set_status(ServiceState.WAITING_NETWORK, "认证未成功，等待重试")
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
        """后台线程：静默验证外网连通性，不改变用户可见状态。"""
        ip = result.ip or ""
        # 先等 5 秒给 DHCP 分配 IP
        self.stop_event.wait(5)
        for attempt in range(3):
            if self.stop_event.is_set():
                return
            if self._ping_ok():
                self._authenticated_at = utc_now_iso()
                self.logger.info("connectivity OK on %s / %s", result.iface, ip)
                self._schedule_renew()
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

    def _save_success_iface(self, result: AuthResult) -> None:
        iface = result.iface or self.config.iface
        if not iface:
            return
        try:
            mac = result.mac or get_if_hwaddr(iface)
            self.config = replace(
                self.config,
                iface=iface,
                last_success_iface=iface,
                last_success_mac=mac,
            )
            save_shared_config(self.config)
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
            return
        if self._retry_count >= MAX_RETRIES:
            self._set_status(ServiceState.FAILED, "多次认证失败，请检查 NetID 或网络")
            return
        self._retry_count += 1
        self._next_retry_at = time.monotonic() + self.config.retry_interval

    def _cancel_retry(self) -> None:
        self._next_retry_at = 0.0
        self._reauth_retry_at = 0.0

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
            self._reauth_event,
            self.logger,
            trigger_callback=self._set_reauth_packet,
            grace_seconds=RENEW_GRACE_SECONDS,
        )
        self._renew_listener.start()

    def _cancel_renew(self) -> None:
        if self._renew_listener and self._renew_listener.is_alive():
            self._renew_stop.set()
            self._renew_listener.join(timeout=1.5)
        self._renew_listener = None

    def _safe_reauth(self) -> None:
        if self._auth_thread and self._auth_thread.is_alive():
            return
        packet = self._reauth_packet
        self._reauth_packet = None
        self._start_auth_flow(manual=False, force=True, reauth_packet=packet)

    def _set_reauth_packet(self, packet: bytes) -> None:
        self._reauth_packet = packet

    def _check_iface_status(self) -> None:
        iface = self.config.iface
        if not iface:
            best = pick_best_candidate()
            if best and best.is_up:
                self.config = replace(self.config, iface=best.name)
                save_shared_config(self.config)
                iface = best.name
                if self.config.auto_auth and not self._manual_disconnect:
                    self._start_auth_flow(manual=False)
            else:
                if not best:
                    self._prev_iface_up = False
                    self._set_status(ServiceState.WAITING_NETWORK, "等待有线网卡")
                return

        stats = psutil.net_if_stats()
        if iface in stats:
            current_up = stats[iface].isup
        elif self.config.last_success_mac:
            matched = find_iface_by_mac(self.config.last_success_mac)
            if matched:
                self.config = replace(self.config, iface=matched.name)
                save_shared_config(self.config)
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
            self._set_status(ServiceState.WAITING_NETWORK, "网卡已断开")
            self._cancel_renew()
            if self.config.auto_auth and not self._manual_disconnect:
                failover = pick_best_candidate()
                if failover and failover.is_up and failover.name != iface:
                    self.config = replace(
                        self.config,
                        iface=failover.name,
                        iface_mode="auto",
                    )
                    save_shared_config(self.config)
                    current_up = True
                    self._retry_count = 0
                    self._start_auth_flow(manual=False)
            self._cancel_retry()
        self._prev_iface_up = current_up

    def _set_authenticated_status(self) -> None:
        iface = self.config.iface
        mac = ""
        ipv4 = ""
        try:
            if iface:
                mac = get_if_hwaddr(iface)
                ipv4 = get_interface_network_info(iface).ipv4 or ""
        except (OSError, RuntimeError):
            self.logger.warning("failed to get MAC/IP for %s", iface, exc_info=True)
        self._set_status(
            ServiceState.AUTHENTICATED,
            "已认证",
            iface=iface,
            mac=mac,
            ipv4=ipv4,
        )

    def logoff(self) -> None:
        self._manual_disconnect = True
        self._cancel_retry()
        self._cancel_renew()
        self._auth_stop.set()
        thread = self._auth_thread
        if thread and thread.is_alive():
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
