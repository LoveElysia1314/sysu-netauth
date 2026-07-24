from __future__ import annotations

import hashlib
import time
from dataclasses import dataclass
from enum import Enum
from typing import Callable

from scapy.all import get_if_addr, get_if_hwaddr, sendp, sniff
from scapy.config import conf
from scapy.layers.l2 import Ether
from scapy.packet import Packet

from .npcap import explain_npcap_requirement, has_npcap

PAE_GROUP = "01:80:c2:00:00:03"
EAPOL_TYPE = 0x888E
EAPOL_VERSION = 1
EAPOL_EAP_PACKET = 0
EAPOL_START = 1
EAPOL_LOGOFF = 2

EAP_REQUEST = 1
EAP_RESPONSE = 2
EAP_SUCCESS = 3
EAP_FAILURE = 4

EAP_IDENTITY = 1
EAP_MD5 = 4


class AuthStatus(str, Enum):
    NO_NPCAP = "no_npcap"
    WAIT_EAP_SERVER = "wait_eap_server"
    AUTHENTICATING = "authenticating"
    AUTH_SUCCESS = "auth_success"
    AUTH_FAILED = "auth_failed"
    TIMEOUT = "timeout"


@dataclass(frozen=True)
class EapPacket:
    code: int
    identifier: int
    eap_type: int | None
    data: bytes


@dataclass(frozen=True)
class AuthOptions:
    iface: str
    username: str
    password: str
    timeout: int = 10
    client_ip: str | None = None


@dataclass(frozen=True)
class AuthResult:
    status: AuthStatus
    message: str
    iface: str
    mac: str | None = None
    ip: str | None = None


ProgressCallback = Callable[[AuthStatus, str], None]
StopCallback = Callable[[], bool]


def eapol_header(packet_type: int, payload: bytes = b"") -> bytes:
    return (
        bytes([EAPOL_VERSION, packet_type]) + len(payload).to_bytes(2, "big") + payload
    )


def eap_packet(
    code: int, identifier: int, eap_type: int | None = None, data: bytes = b""
) -> bytes:
    body = b"" if eap_type is None else bytes([eap_type]) + data
    length = 4 + len(body)
    return bytes([code, identifier]) + length.to_bytes(2, "big") + body


def md5_challenge_response(identifier: int, password: str, challenge: bytes) -> bytes:
    return hashlib.md5(
        bytes([identifier]) + password.encode("gbk") + challenge
    ).digest()


def build_start(src_mac: str, dst_mac: str = PAE_GROUP) -> Packet:
    return Ether(dst=dst_mac, src=src_mac, type=EAPOL_TYPE) / eapol_header(EAPOL_START)


def build_logoff(src_mac: str, dst_mac: str = PAE_GROUP) -> Packet:
    return Ether(dst=dst_mac, src=src_mac, type=EAPOL_TYPE) / eapol_header(EAPOL_LOGOFF)


def build_identity_response(
    src_mac: str, dst_mac: str, identifier: int, username: str
) -> Packet:
    payload = eap_packet(EAP_RESPONSE, identifier, EAP_IDENTITY, username.encode("gbk"))
    return Ether(dst=dst_mac, src=src_mac, type=EAPOL_TYPE) / eapol_header(
        EAPOL_EAP_PACKET, payload
    )


def build_md5_response(
    src_mac: str,
    dst_mac: str,
    identifier: int,
    username: str,
    password: str,
    challenge: bytes,
) -> Packet:
    digest = md5_challenge_response(identifier, password, challenge)
    md5_body = bytes([len(digest)]) + digest + username.encode("gbk")
    payload = eap_packet(EAP_RESPONSE, identifier, EAP_MD5, md5_body)
    return Ether(dst=dst_mac, src=src_mac, type=EAPOL_TYPE) / eapol_header(
        EAPOL_EAP_PACKET, payload
    )


def parse_eapol(raw: bytes) -> EapPacket | None:
    if len(raw) < 18 or raw[12:14] != b"\x88\x8e":
        return None
    payload = raw[14:]
    if len(payload) < 4 or payload[1] != EAPOL_EAP_PACKET:
        return None
    eap = payload[4:]
    if len(eap) < 4:
        return None
    code = eap[0]
    identifier = eap[1]
    eap_len = int.from_bytes(eap[2:4], "big")
    if eap_len < 4 or eap_len > len(eap):
        return None
    if code in (EAP_SUCCESS, EAP_FAILURE):
        return EapPacket(code, identifier, None, b"")
    if eap_len < 5:
        return None
    return EapPacket(code, identifier, eap[4], eap[5:eap_len])


def detect_iface_ip(iface: str) -> str | None:
    try:
        ip = get_if_addr(iface)
        return ip if ip and ip != "0.0.0.0" else None
    except Exception:
        return None


def authenticate(
    options: AuthOptions,
    progress: ProgressCallback | None = None,
    should_stop: StopCallback | None = None,
    *,
    send_start: bool = True,
    initial_packet: bytes | None = None,
) -> AuthResult:
    if not has_npcap():
        return AuthResult(
            AuthStatus.NO_NPCAP, explain_npcap_requirement(), options.iface
        )

    def emit(status: AuthStatus, message: str) -> None:
        if progress:
            progress(status, message)

    old_iface = conf.iface
    conf.iface = options.iface
    try:
        src_mac = get_if_hwaddr(options.iface)
        client_ip = options.client_ip or detect_iface_ip(options.iface)

        emit(AuthStatus.WAIT_EAP_SERVER, f"iface={options.iface} mac={src_mac}")
        if send_start:
            sendp(build_start(src_mac), iface=options.iface, verbose=False)
        started_at = time.monotonic()
        pending_raw = initial_packet

        while True:
            remaining = options.timeout - (time.monotonic() - started_at)
            if remaining <= 0:
                break
            if should_stop and should_stop():
                return AuthResult(
                    AuthStatus.TIMEOUT,
                    "authentication cancelled",
                    options.iface,
                    src_mac,
                    client_ip,
                )
            if pending_raw is not None:
                raw_packet = pending_raw
                pending_raw = None
                packet_src = ":".join(f"{part:02x}" for part in raw_packet[6:12])
            else:
                packets = sniff(
                    iface=options.iface,
                    filter="ether proto 0x888e",
                    timeout=min(1.0, remaining),
                    count=1,
                )
                if not packets:
                    emit(AuthStatus.WAIT_EAP_SERVER, "waiting for EAP server")
                    if send_start:
                        sendp(build_start(src_mac), iface=options.iface, verbose=False)
                    continue
                packet = packets[0]
                raw_packet = bytes(packet)
                packet_src = packet.src
            if should_stop and should_stop():
                return AuthResult(
                    AuthStatus.TIMEOUT,
                    "authentication cancelled",
                    options.iface,
                    src_mac,
                    client_ip,
                )

            parsed = parse_eapol(raw_packet)
            if not parsed:
                continue
            switch_mac = packet_src

            if parsed.code == EAP_SUCCESS:
                emit(AuthStatus.AUTH_SUCCESS, "EAP success")
                return AuthResult(
                    AuthStatus.AUTH_SUCCESS,
                    "EAP success",
                    options.iface,
                    src_mac,
                    client_ip,
                )
            if parsed.code == EAP_FAILURE:
                emit(AuthStatus.AUTH_FAILED, "EAP failure")
                return AuthResult(
                    AuthStatus.AUTH_FAILED,
                    "EAP failure",
                    options.iface,
                    src_mac,
                    client_ip,
                )
            if parsed.code != EAP_REQUEST:
                continue

            emit(AuthStatus.AUTHENTICATING, f"EAP request type={parsed.eap_type}")
            if parsed.eap_type == EAP_IDENTITY:
                sendp(
                    build_identity_response(
                        src_mac, switch_mac, parsed.identifier, options.username
                    ),
                    iface=options.iface,
                    verbose=False,
                )
            elif parsed.eap_type == EAP_MD5:
                if not parsed.data:
                    return AuthResult(
                        AuthStatus.AUTH_FAILED,
                        "empty MD5 challenge",
                        options.iface,
                        src_mac,
                        client_ip,
                    )
                challenge_len = parsed.data[0]
                if challenge_len == 0 or len(parsed.data) < 1 + challenge_len:
                    return AuthResult(
                        AuthStatus.AUTH_FAILED,
                        "invalid MD5 challenge",
                        options.iface,
                        src_mac,
                        client_ip,
                    )
                challenge = parsed.data[1 : 1 + challenge_len]
                sendp(
                    build_md5_response(
                        src_mac,
                        switch_mac,
                        parsed.identifier,
                        options.username,
                        options.password,
                        challenge,
                    ),
                    iface=options.iface,
                    verbose=False,
                )

        emit(AuthStatus.TIMEOUT, "authentication timed out")
        return AuthResult(
            AuthStatus.TIMEOUT,
            "authentication timed out",
            options.iface,
            src_mac,
            client_ip,
        )
    finally:
        conf.iface = old_iface


def send_logoff(iface: str) -> str:
    if not has_npcap():
        raise RuntimeError(explain_npcap_requirement())
    old_iface = conf.iface
    conf.iface = iface
    try:
        src_mac = get_if_hwaddr(iface)
        sendp(build_logoff(src_mac), iface=iface, verbose=False)
        return src_mac
    finally:
        conf.iface = old_iface
