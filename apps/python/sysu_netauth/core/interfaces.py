from __future__ import annotations

import ctypes
import re
import time
from ctypes import wintypes
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

# ============================================================
# Win32 GetAdaptersAddresses 常量 & 结构体
# 替代 PowerShell Get-NetAdapter / Get-NetIPConfiguration，
# 消除进程 spawn 开销和 Session 0 下间歇性超时问题。
# ============================================================

_ERROR_SUCCESS = 0
_ERROR_BUFFER_OVERFLOW = 111
_AF_UNSPEC = 0
_AF_INET = 2

_GAA_FLAG_INCLUDE_PREFIX = 0x0010
_GAA_FLAG_SKIP_ANYCAST = 0x0002
_GAA_FLAG_SKIP_MULTICAST = 0x0004
_GAA_FLAG_INCLUDE_GATEWAYS = 0x0080

_IF_TYPE_ETHERNET_CSMACD = 6
_IF_TYPE_SOFTWARE_LOOPBACK = 24
_IF_TYPE_IEEE80211 = 71

_MAX_ADAPTER_ADDRESS_LENGTH = 8

# LWF / filter driver 后缀 —— GetAdaptersAddresses 列出 WFP/Npcap/QoS
# 轻量过滤器，它们不是独立物理网卡，必须过滤。
_LWF_SUFFIX_RE = re.compile(
    r"-(WFP|Npcap|QoS|VirtualBox|NDIS|LightWeight|Packet (Scheduler|Driver))",
    re.IGNORECASE,
)


class _SOCKET_ADDRESS(ctypes.Structure):
    _fields_ = [
        ("lpSockaddr", ctypes.c_void_p),
        ("iSockaddrLength", ctypes.c_int),
    ]


class _SOCKADDR_IN(ctypes.Structure):
    _fields_ = [
        ("sin_family", ctypes.c_ushort),
        ("sin_port", ctypes.c_ushort),
        ("sin_addr", ctypes.c_ubyte * 4),
        ("sin_zero", ctypes.c_ubyte * 8),
    ]


class _UA(ctypes.Structure):
    pass


_UA._fields_ = [
    ("Length", ctypes.c_ulong),
    ("Reserved", ctypes.c_ulong),
    ("Next", ctypes.POINTER(_UA)),
    ("Address", _SOCKET_ADDRESS),
    ("PrefixOrigin", ctypes.c_int),
    ("SuffixOrigin", ctypes.c_int),
    ("DadState", ctypes.c_int),
    ("ValidLifetime", ctypes.c_ulong),
    ("PreferredLifetime", ctypes.c_ulong),
    ("LeaseLifetime", ctypes.c_ulong),
    ("OnLinkPrefixLength", ctypes.c_ubyte),
]


class _DNS(ctypes.Structure):
    pass


_DNS._fields_ = [
    ("Length", ctypes.c_ulong),
    ("Reserved", ctypes.c_ulong),
    ("Next", ctypes.POINTER(_DNS)),
    ("Address", _SOCKET_ADDRESS),
]


class _GW(ctypes.Structure):
    pass


_GW._fields_ = [
    ("Length", ctypes.c_ulong),
    ("Reserved", ctypes.c_ulong),
    ("Next", ctypes.POINTER(_GW)),
    ("Address", _SOCKET_ADDRESS),
]


class _AA(ctypes.Structure):
    pass


_AA._fields_ = [
    ("Length", ctypes.c_ulong),
    ("IfIndex", ctypes.c_uint32),
    ("Next", ctypes.POINTER(_AA)),
    ("AdapterName", ctypes.c_char_p),
    ("FirstUnicastAddress", ctypes.POINTER(_UA)),
    ("FirstAnycastAddress", ctypes.c_void_p),
    ("FirstMulticastAddress", ctypes.c_void_p),
    ("FirstDnsServerAddress", ctypes.POINTER(_DNS)),
    ("DnsSuffix", ctypes.c_wchar_p),
    ("Description", ctypes.c_wchar_p),
    ("FriendlyName", ctypes.c_wchar_p),
    ("PhysicalAddress", ctypes.c_ubyte * _MAX_ADAPTER_ADDRESS_LENGTH),
    ("PhysicalAddressLength", ctypes.c_uint32),
    ("Flags", ctypes.c_uint32),
    ("Mtu", ctypes.c_uint32),
    ("IfType", ctypes.c_uint32),
    ("OperStatus", ctypes.c_uint32),
    ("Ipv6IfIndex", ctypes.c_uint32),
    ("ZoneIndices", ctypes.c_uint32 * 16),
    ("FirstPrefix", ctypes.c_void_p),
    ("TransmitLinkSpeed", ctypes.c_uint64),
    ("ReceiveLinkSpeed", ctypes.c_uint64),
    ("FirstWinsServerAddress", ctypes.c_void_p),
    ("FirstGatewayAddress", ctypes.POINTER(_GW)),
]

# ============================================================

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


# ── GetAdaptersAddresses 统一缓存 ──
# 一次 Win32 调用同时满足 _adapter_metadata() 和 get_interface_network_info()
_WIN32_CACHE: tuple[float, list[dict]] = (0.0, [])
_WIN32_CACHE_TTL = 30.0


def _sockaddr_to_ipv4(lpSockaddr) -> str:
    """SOCKADDR_IN 指针 → IPv4 字符串."""
    if not lpSockaddr:
        return ""
    sa = ctypes.cast(lpSockaddr, ctypes.POINTER(_SOCKADDR_IN)).contents
    if sa.sin_family != _AF_INET:
        return ""
    return ".".join(str(b) for b in bytes(sa.sin_addr))


def _win32_enumerate(force: bool = False) -> list[dict]:
    """调用 GetAdaptersAddresses，返回所有网卡的原始数据列表。

    每个条目: name, description, mac, if_type, oper_status, speed_mbps,
    ipv4, gateway, dns, is_lwf.
    外部通过 _adapter_metadata() 和 get_interface_network_info() 访问。

    force=True 时跳过缓存，用于需要最新 IP/网关/DNS 的场景
    （如认证刚成功后获取 DHCP 分配的地址）。
    """
    global _WIN32_CACHE
    now = time.monotonic()
    cached = _WIN32_CACHE[1]
    if not force:
        cached_at = _WIN32_CACHE[0]
        if cached and now - cached_at < _WIN32_CACHE_TTL:
            return cached

    try:
        iphlpapi = ctypes.windll.iphlpapi
        buf_len = wintypes.ULONG(0)
        flags = (
            _GAA_FLAG_INCLUDE_PREFIX
            | _GAA_FLAG_SKIP_ANYCAST
            | _GAA_FLAG_SKIP_MULTICAST
            | _GAA_FLAG_INCLUDE_GATEWAYS
        )

        ret = iphlpapi.GetAdaptersAddresses(
            _AF_UNSPEC, flags, None, None, ctypes.byref(buf_len)
        )
        if ret != _ERROR_BUFFER_OVERFLOW:
            return cached  # 返回旧缓存

        buf = (ctypes.c_ubyte * buf_len.value)()
        ptr = ctypes.cast(buf, ctypes.POINTER(_AA))
        ptr.contents.Length = ctypes.sizeof(_AA)

        ret = iphlpapi.GetAdaptersAddresses(
            _AF_UNSPEC, flags, None, ptr, ctypes.byref(buf_len)
        )
        if ret != _ERROR_SUCCESS:
            return cached

        adapters: list[dict] = []
        node = ptr
        while node:
            a = node.contents
            name = a.FriendlyName or ""
            desc = a.Description or ""

            mac = ""
            if a.PhysicalAddressLength > 0:
                pa = bytes(a.PhysicalAddress)[: a.PhysicalAddressLength]
                mac = ":".join(f"{b:02x}" for b in pa)

            ipv4 = ""
            u = a.FirstUnicastAddress
            while u:
                ipv4 = _sockaddr_to_ipv4(u.contents.Address.lpSockaddr)
                if ipv4:
                    break
                u = u.contents.Next

            gateway = ""
            g = a.FirstGatewayAddress
            while g:
                gw_ip = _sockaddr_to_ipv4(g.contents.Address.lpSockaddr)
                if gw_ip and gw_ip != "0.0.0.0":
                    gateway = gw_ip
                    break
                g = g.contents.Next

            dns_list: list[str] = []
            d = a.FirstDnsServerAddress
            while d:
                dns_ip = _sockaddr_to_ipv4(d.contents.Address.lpSockaddr)
                if dns_ip and dns_ip != "0.0.0.0":
                    dns_list.append(dns_ip)
                d = d.contents.Next

            speed = int(max(a.TransmitLinkSpeed, a.ReceiveLinkSpeed) // 1_000_000)

            adapters.append(
                {
                    "name": name,
                    "description": desc,
                    "mac": mac,
                    "if_type": a.IfType,
                    "oper_status": a.OperStatus,
                    "speed_mbps": speed,
                    "ipv4": ipv4,
                    "gateway": gateway,
                    "dns": dns_list,
                    "is_lwf": bool(_LWF_SUFFIX_RE.search(name)),
                }
            )

            node = a.Next

        _WIN32_CACHE = (now, adapters)
    except Exception:
        import logging

        logging.getLogger("sysu_netauth.core.interfaces").warning(
            "GetAdaptersAddresses failed, reusing cached data"
        )

    return _WIN32_CACHE[1]


def _adapter_metadata() -> dict[str, dict]:
    """返回 {网卡名: {description, if_type, oper_status, speed_mbps}} 元数据字典。

    数据源为 GetAdaptersAddresses（Win32 API），一次调用替代
    Get-NetAdapter + Get-NetIPConfiguration + Get-DnsClientServerAddress。
    结果按 name 索引，供 list_candidates() 使用。
    """
    return {
        a["name"]: {
            "description": a["description"],
            "if_type": a["if_type"],
            "oper_status": a["oper_status"],
            "speed_mbps": a["speed_mbps"],
        }
        for a in _win32_enumerate()
        if a["name"] and not a["is_lwf"]
    }


def _resolve_interface_type(
    name: str, description: str = "", if_type: int | None = None
) -> InterfaceType:
    """根据网卡名、描述和 Win32 IfType 判断接口类型。

    IfType 来自 GetAdaptersAddresses（替代已移除的 Get-NetAdapter
    HardwareInterface）。优先级：
    1. IfType=IF_TYPE_SOFTWARE_LOOPBACK → LOOPBACK
    2. IfType=IF_TYPE_IEEE80211 → WIRELESS
    3. 名称/描述含虚拟网卡关键词 → VIRTUAL/LOOPBACK
    4. 名称/描述含无线关键词 → WIRELESS
    5. IfType=IF_TYPE_ETHERNET_CSMACD + 非 LWF → ETHERNET
    6. UNKNOWN
    """
    # 1. Win32 IfType 硬分类
    if if_type == _IF_TYPE_SOFTWARE_LOOPBACK:
        return InterfaceType.LOOPBACK
    if if_type == _IF_TYPE_IEEE80211:
        return InterfaceType.WIRELESS

    # 2. 名称/描述关键词
    if _name_is_virtual(name, description):
        if "loopback" in name.lower():
            return InterfaceType.LOOPBACK
        return InterfaceType.VIRTUAL
    if WIRELESS_PATTERNS.search(f"{name} {description}"):
        return InterfaceType.WIRELESS

    # 3. IfType 指示为以太网 → ETHERNET（Win32 无 LWF 噪音）
    if if_type == _IF_TYPE_ETHERNET_CSMACD:
        return InterfaceType.ETHERNET

    # 4. 名称含 ethernet/以太网 但 IfType 未知 → 保守给 UNKNOWN
    lower = name.lower()
    if "ethernet" in lower or "以太网" in lower:
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
    has_media: bool
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


def _is_virtual_mac(mac: str) -> bool:
    """Check if a MAC address matches known virtual OUI prefixes."""
    mac_colon = mac.lower().replace("-", ":")
    return any(mac_colon.startswith(p) for p in VIRTUAL_MAC_PREFIXES)


def _name_is_virtual(name: str, description: str = "") -> bool:
    """Check if interface name/description hints at virtual or loopback."""
    text = f"{name} {description}"
    lower = text.lower()
    if "loopback" in lower:
        return True
    return bool(VIRTUAL_PATTERNS.search(text))


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
        description = str(meta.get("description") or "")
        if_type = meta.get("if_type")
        iface_type = _resolve_interface_type(name, description, if_type)

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
            # Win32 IfType 已确认物理以太网
            score += 20
            reasons.append("ethernet")

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
                if _is_virtual_mac(mac):
                    iface_type = InterfaceType.VIRTUAL
                    reasons.append("virtual-mac")
        else:
            reasons.append("no-mac")
        if description and description != name:
            reasons.append(description)

        # ── 物理链路状态 ──
        # Win32 OperStatus: 1=IfOperStatusUp, 2=IfOperStatusDown
        # 与 Get-NetAdapter MediaConnectState 不完全等价但足够用于判定
        oper_status = meta.get("oper_status")
        if oper_status == 1:
            has_media = True
            reasons.append("media")
        elif oper_status is not None:
            has_media = False
            reasons.append("no-media")
        else:
            # 元数据缺失，以 speed > 0 作为近似判定
            has_media = bool(stat.speed and stat.speed > 0)
            reasons.append("media?" if has_media else "no-media?")

        candidates.append(
            InterfaceCandidate(
                name=name,
                mac=mac,
                is_up=stat.isup,
                has_media=has_media,
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
    """返回指定网卡的 IPv4、默认网关和 DNS 服务器。

    数据源：GetAdaptersAddresses（Win32 API），一次调用即返回完整
    网络信息（IP/网关/DNS），无需 PowerShell 子进程，无 Session 0
    兼容性问题。
    """
    for a in _win32_enumerate(force=True):
        if a["name"] == iface and not a["is_lwf"]:
            dns = tuple(a["dns"]) if a["dns"] else ()
            return InterfaceNetworkInfo(
                ipv4=a["ipv4"] or None,
                gateway=a["gateway"] or None,
                dns=dns,
            )
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
