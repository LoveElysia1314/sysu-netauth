from __future__ import annotations

import re
import json
import subprocess
import time
from dataclasses import dataclass
from enum import Enum

import psutil
from scapy.all import get_if_hwaddr, sendp, sniff

from .eapol import (
    EAP_IDENTITY,
    EAP_REQUEST,
    AuthStatus,
    StopCallback,
    build_start,
    parse_eapol,
)
from .npcap import has_npcap

VIRTUAL_PATTERNS = re.compile(
    r"(loopback|virtual|vmware|virtualbox|hyper-v|wsl|bluetooth|蓝牙|tap|tun|"
    r"clash|tailscale|zerotier|npcap|host-only|docker|vmnet|ndis|pve|oracle vm)",
    re.IGNORECASE,
)

# 已知虚拟化平台 MAC OUI 前缀（前 3 字节），无元数据时的兜底
VIRTUAL_MAC_PREFIXES = {
    # VirtualBox
    "08:00:27",
    "0a:00:27",
    # VMware
    "00:0c:29",
    "00:50:56",
    "00:05:69",
    # Hyper-V
    "00:15:5d",
    "00:03:ff",
    # Docker / container
    "02:42:ac",
    "02:42:a9",
}

WIRELESS_PATTERNS = re.compile(
    r"(wi-fi|wlan|wireless|无线)",
    re.IGNORECASE,
)


class InterfaceType(Enum):
    ETHERNET = "ethernet"
    WIRELESS = "wireless"
    VIRTUAL = "virtual"
    LOOPBACK = "loopback"
    UNKNOWN = "unknown"


_ADAPTER_META_CACHE: tuple[float, dict[str, dict]] = (0.0, {})
ADAPTER_META_TTL = 30.0


def _run_powershell(script: str, timeout: int = 5) -> str | None:
    """Run a PowerShell script with UTF-8 output encoding and return decoded stdout.

    PowerShell 5.1 在管道重定向时默认用系统代码页（中文 Windows 为 GBK），
    会导致中文字符被错误解码。已在命令中强制设置 OutputEncoding = UTF8。
    返回解码后的字符串，失败时返回 None。
    """
    try:
        prefixed = "[Console]::OutputEncoding = [System.Text.Encoding]::UTF8; " + script
        result = subprocess.run(
            ["powershell", "-NoProfile", "-Command", prefixed],
            capture_output=True,
            timeout=timeout,
            creationflags=subprocess.CREATE_NO_WINDOW,
        )
        if result.returncode != 0:
            return None
        raw_bytes = result.stdout
        for enc in ("utf-8", "gbk", "utf-16-le"):
            try:
                decoded = raw_bytes.decode(enc).lstrip("\ufeff")
                if decoded.strip():
                    return decoded
            except (UnicodeDecodeError, LookupError):
                continue
        return raw_bytes.decode("utf-8", errors="replace")
    except Exception:
        return None


def _adapter_metadata() -> dict[str, dict]:
    """Return Windows adapter metadata keyed by interface name.

    psutil/Scapy expose friendly names such as "以太网 4", but virtualization
    hints often live in InterfaceDescription/HardwareInterface.
    """
    global _ADAPTER_META_CACHE
    cached_at, cached = _ADAPTER_META_CACHE
    if time.monotonic() - cached_at < ADAPTER_META_TTL:
        return cached

    try:
        decoded = _run_powershell(
            "Get-NetAdapter | "
            "Select-Object Name,InterfaceDescription,HardwareInterface | "
            "ConvertTo-Json -Compress"
        )
        if not decoded:
            raise RuntimeError(
                "Get-NetAdapter failed (exit code non-zero or empty output)"
            )
        raw = json.loads(decoded)
        if isinstance(raw, dict):
            raw = [raw]
        data = {
            item.get("Name", ""): item
            for item in raw
            if isinstance(item, dict) and item.get("Name")
        }
    except Exception as exc:
        import logging

        logging.getLogger("sysu_netauth.core.interfaces").warning(
            "_adapter_metadata failed: %s", exc
        )
        data = {}
    _ADAPTER_META_CACHE = (time.monotonic(), data)
    return data


def _resolve_interface_type(
    name: str, description: str = "", hardware: bool | None = None
) -> InterfaceType:
    """根据网卡名和 Windows 元数据判断接口类型。

    优先级（从高到低）：
    1. Loopback Pseudo-Interface
    2. HardwareInterface=False（Windows 明确标记为非物理网卡）
    3. 名称/描述含虚拟网卡关键词
    4. 名称/描述含无线网卡关键词
    5. 名称含 ethernet/以太网
    6. UNKNOWN
    """
    text = f"{name} {description}"
    lower = text.lower()

    # 1. Loopback —— 名称本身即可判断
    if "loopback" in lower or "loopback pseudo" in lower:
        return InterfaceType.LOOPBACK

    # 2. 硬件标记 —— Get-NetAdapter HardwareInterface 是 Windows 官方判定，最权威
    if hardware is False:
        return InterfaceType.VIRTUAL

    # 3. 虚拟网卡关键词匹配（名称+描述）
    if VIRTUAL_PATTERNS.search(text):
        return InterfaceType.VIRTUAL

    # 4. 无线网卡关键词匹配
    if WIRELESS_PATTERNS.search(text):
        return InterfaceType.WIRELESS

    # 5. 包含 ethernet/以太网 — 仅在 Windows 确认为硬件时可信
    if "ethernet" in lower or "以太网" in lower:
        if hardware is True:
            return InterfaceType.ETHERNET
        # hardware is None → 元数据缺失，仅靠名称不可靠（虚拟网卡也以"以太网 N"命名）
        return InterfaceType.UNKNOWN

    return InterfaceType.UNKNOWN


def _format_speed(speed: int | None) -> str:
    if speed is None or speed <= 0:
        return ""
    if speed >= 1000:
        return f"{speed // 1000}Gbps"
    return f"{speed}Mbps"


@dataclass(frozen=True)
class InterfaceCandidate:
    name: str
    mac: str | None
    is_up: bool
    speed_mbps: int | None
    interface_type: InterfaceType
    adapter_description: str
    reason: str
    score: int


@dataclass(frozen=True)
class ProbeResult:
    iface: str
    status: AuthStatus
    message: str
    mac: str | None = None


@dataclass(frozen=True)
class InterfaceNetworkInfo:
    """User-facing IP details for a selected Windows network interface."""

    ipv4: str | None = None
    gateway: str | None = None
    dns: tuple[str, ...] = ()


def list_candidates() -> list[InterfaceCandidate]:
    """返回所有网卡，按评分降序排列。"""
    stats = psutil.net_if_stats()
    addrs = psutil.net_if_addrs()
    metadata = _adapter_metadata()
    candidates: list[InterfaceCandidate] = []
    for name, stat in stats.items():
        score = 0
        reasons: list[str] = []
        meta = metadata.get(name, {})
        description = str(meta.get("InterfaceDescription") or "")
        hardware = meta.get("HardwareInterface")
        iface_type = _resolve_interface_type(name, description, hardware)

        if stat.isup:
            score += 30
            reasons.append("up")
        else:
            reasons.append("down")

        if iface_type == InterfaceType.VIRTUAL:
            score -= 100
            reasons.append("virtual")
        elif iface_type == InterfaceType.WIRELESS:
            score -= 50
            reasons.append("wireless")
        elif iface_type == InterfaceType.LOOPBACK:
            score -= 100
            reasons.append("loopback")
        elif iface_type == InterfaceType.ETHERNET:
            # 有元数据支撑的物理以太网卡加分更多
            if hardware is True:
                score += 20
                reasons.append("ethernet")
            else:
                # 元数据缺失时保守加分，避免虚拟网卡冒顶
                score += 10
                reasons.append("ethernet?")
                reasons.append("no-metadata")

        if stat.speed and stat.speed > 0:
            score += 10
            reasons.append(_format_speed(stat.speed))

        mac = None
        for addr in addrs.get(name, []):
            if getattr(addr, "family", None) == psutil.AF_LINK:
                mac = addr.address
                break
        if mac == "-":
            mac = None
        if mac:
            score += 10
            # MAC 前缀匹配已知虚拟 OUI → 降级为虚拟网卡
            if iface_type in (InterfaceType.ETHERNET, InterfaceType.UNKNOWN):
                mac_lower = mac.lower().replace("-", ":")
                if any(mac_lower.startswith(p) for p in VIRTUAL_MAC_PREFIXES):
                    iface_type = InterfaceType.VIRTUAL
                    reasons.append("virtual-mac")
        else:
            reasons.append("no-mac")
        if description and description != name:
            reasons.append(description)

        candidates.append(
            InterfaceCandidate(
                name=name,
                mac=mac,
                is_up=stat.isup,
                speed_mbps=stat.speed if stat.speed > 0 else None,
                interface_type=iface_type,
                adapter_description=description,
                reason=", ".join(reasons),
                score=score,
            )
        )
    return sorted(candidates, key=lambda item: item.score, reverse=True)


def list_ethernet_candidates() -> list[InterfaceCandidate]:
    """仅返回真实以太网卡（排除虚拟、无线、回环）。"""
    return [
        c
        for c in list_candidates()
        if c.interface_type == InterfaceType.ETHERNET and c.mac
    ]


def list_auth_candidate_interfaces() -> list[InterfaceCandidate]:
    """返回可用于 802.1X 认证的已连接明确有线网卡。"""
    return [c for c in list_ethernet_candidates() if c.is_up]


def find_iface_by_mac(mac: str) -> InterfaceCandidate | None:
    """通过 MAC 地址查找网卡，网卡名变化时仍能匹配。"""
    candidates = list_auth_candidate_interfaces()
    for c in candidates:
        if c.mac and c.mac.lower() == mac.lower():
            return c
    return None


def pick_best_candidate() -> InterfaceCandidate | None:
    """返回评分最高的可认证网卡。"""
    candidates = list_auth_candidate_interfaces()
    return candidates[0] if candidates else None


def get_interface_network_info(iface: str) -> InterfaceNetworkInfo:
    """Return IPv4, default gateway and DNS servers for a Windows interface.

    psutil is reliable for local addresses, but not for gateways/DNS. Query the
    Windows cmdlets separately because Get-NetIPConfiguration can leave gateway
    or DNS empty even when the route and DNS client tables contain them.
    """
    try:
        escaped_iface = iface.replace("'", "''")
        decoded = _run_powershell(
            f"$alias = '{escaped_iface}'; "
            "$cfg = Get-NetIPConfiguration -InterfaceAlias $alias -ErrorAction Stop; "
            "$route = Get-NetRoute -InterfaceAlias $alias "
            "-DestinationPrefix '0.0.0.0/0' -ErrorAction SilentlyContinue | "
            "Sort-Object RouteMetric,InterfaceMetric | Select-Object -First 1; "
            "$dns = Get-DnsClientServerAddress -InterfaceAlias $alias "
            "-AddressFamily IPv4 -ErrorAction SilentlyContinue; "
            "[PSCustomObject]@{"
            "IPv4=@($cfg.IPv4Address | Select-Object -First 1 "
            "-ExpandProperty IPAddress);"
            "Gateway=@($route.NextHop);"
            "DNS=@($dns.ServerAddresses)"
            "} | ConvertTo-Json -Compress"
        )
        if not decoded:
            raise RuntimeError("Get-NetIPConfiguration failed")
        data = json.loads(decoded)
        if not isinstance(data, dict):
            raise RuntimeError("Unexpected network info shape")
        dns_raw = data.get("DNS") or []
        if isinstance(dns_raw, str):
            dns = (dns_raw,)
        else:
            dns = tuple(str(item) for item in dns_raw if item)
        ipv4_raw = data.get("IPv4")
        if isinstance(ipv4_raw, list):
            ipv4 = str(ipv4_raw[0]) if ipv4_raw else None
        else:
            ipv4 = str(ipv4_raw) if ipv4_raw else None
        gateway_raw = data.get("Gateway")
        if isinstance(gateway_raw, list):
            gateway = str(gateway_raw[0]) if gateway_raw else None
        else:
            gateway = str(gateway_raw) if gateway_raw else None
        return InterfaceNetworkInfo(
            ipv4=ipv4,
            gateway=gateway,
            dns=dns,
        )
    except Exception:
        return InterfaceNetworkInfo()


def probe_eapol(
    iface: str, timeout: int = 5, should_stop: StopCallback | None = None
) -> ProbeResult:
    if not has_npcap():
        return ProbeResult(iface, AuthStatus.NO_NPCAP, "Npcap unavailable")
    try:
        mac = get_if_hwaddr(iface)
        sendp(build_start(mac), iface=iface, verbose=False)
        started_at = time.monotonic()
        while time.monotonic() - started_at < timeout:
            if should_stop and should_stop():
                return ProbeResult(iface, AuthStatus.TIMEOUT, "probe cancelled", mac)
            packets = sniff(
                iface=iface, filter="ether proto 0x888e", timeout=1, count=1
            )
            if should_stop and should_stop():
                return ProbeResult(iface, AuthStatus.TIMEOUT, "probe cancelled", mac)
            if not packets:
                continue
            parsed = parse_eapol(bytes(packets[0]))
            if parsed and parsed.code == EAP_REQUEST:
                if parsed.eap_type == EAP_IDENTITY:
                    return ProbeResult(
                        iface,
                        AuthStatus.WAIT_EAP_SERVER,
                        "EAP identity request detected",
                        mac,
                    )
                return ProbeResult(
                    iface,
                    AuthStatus.WAIT_EAP_SERVER,
                    f"EAP request type={parsed.eap_type} detected",
                    mac,
                )
        return ProbeResult(iface, AuthStatus.TIMEOUT, "no EAPOL response", mac)
    except Exception as exc:
        return ProbeResult(iface, AuthStatus.AUTH_FAILED, str(exc))
