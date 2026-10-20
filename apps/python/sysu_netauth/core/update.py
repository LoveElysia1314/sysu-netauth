"""Update manifest parsing and validation.

This module is deliberately independent from Qt and the Windows service so its
security-sensitive parsing and version comparison can be tested in isolation.
"""

from __future__ import annotations

import json
import re
from dataclasses import dataclass, replace as dataclass_replace
from typing import Callable
from urllib.parse import urlparse
from urllib.request import Request, urlopen

from sysu_netauth.core.config import APP_VERSION

GITEE_PROJECT_URL = "https://gitee.com/LoveElysia1314/sysu-netauth"
PROJECT_URL = "https://github.com/LoveElysia1314/sysu-netauth"
RELEASES_URL = f"{PROJECT_URL}/releases"
ISSUES_URL = f"{PROJECT_URL}/issues"
NPCAP_URL = "https://npcap.com/#download"
SYSU_WIRED_HELP_URL = "https://inc.sysu.edu.cn/service/wired-network-access"

MAX_MANIFEST_BYTES = 64 * 1024
REQUEST_TIMEOUT_SECONDS = 8
_VERSION_RE = re.compile(r"^v?(\d+)\.(\d+)\.(\d+)$")
_ALLOWED_RELEASE_HOSTS = {"gitee.com", "github.com"}


@dataclass(frozen=True)
class ManifestSource:
    name: str
    url: str
    allowed_hosts: frozenset[str]


UPDATE_MANIFEST_SOURCES = (
    ManifestSource(
        name="gitee",
        url=(
            "https://gitee.com/LoveElysia1314/"
            "sysu-netauth/raw/main/updates/release.json"
        ),
        allowed_hosts=frozenset({"gitee.com"}),
    ),
    ManifestSource(
        name="github",
        url=(
            "https://raw.githubusercontent.com/"
            "LoveElysia1314/sysu-netauth/main/updates/release.json"
        ),
        allowed_hosts=frozenset({"raw.githubusercontent.com"}),
    ),
)


class UpdateManifestError(ValueError):
    """The remote update manifest is unavailable or invalid."""


@dataclass(frozen=True)
class ReleaseInfo:
    version: str
    published_at: str
    release_url: str
    summary: str
    channel: str = "stable"
    source: str = ""


def parse_version(value: str) -> tuple[int, int, int]:
    match = _VERSION_RE.fullmatch(value.strip())
    if not match:
        raise UpdateManifestError(f"无效版本号: {value!r}")
    parts = tuple(int(part) for part in match.groups())
    if any(part > 999_999 for part in parts):
        raise UpdateManifestError("版本号数值超出允许范围")
    return parts  # type: ignore[return-value]


def is_newer_version(candidate: str, current: str = APP_VERSION) -> bool:
    return parse_version(candidate) > parse_version(current)


def is_safe_external_url(url: str, *, allowed_hosts: set[str]) -> bool:
    parsed = urlparse(url)
    try:
        port = parsed.port
    except ValueError:
        return False
    return (
        parsed.scheme == "https"
        and parsed.hostname is not None
        and parsed.hostname.lower() in allowed_hosts
        and port in (None, 443)
        and not parsed.username
        and not parsed.password
    )


def parse_release_manifest(payload: bytes | str) -> ReleaseInfo:
    if isinstance(payload, bytes):
        try:
            text = payload.decode("utf-8")
        except UnicodeDecodeError as exc:
            raise UpdateManifestError("更新清单不是 UTF-8") from exc
    else:
        text = payload
    try:
        data = json.loads(text)
    except json.JSONDecodeError as exc:
        raise UpdateManifestError("更新清单不是有效 JSON") from exc
    if not isinstance(data, dict) or data.get("schema_version") != 1:
        raise UpdateManifestError("不支持的更新清单格式")

    version = str(data.get("version") or "").strip()
    parse_version(version)
    channel = str(data.get("channel") or "stable").strip().lower()
    if channel != "stable":
        raise UpdateManifestError("当前仅支持 stable 更新通道")
    release_url = str(data.get("release_url") or "").strip()
    if not is_safe_external_url(
        release_url,
        allowed_hosts=_ALLOWED_RELEASE_HOSTS,
    ):
        raise UpdateManifestError("更新页面地址不受信任")
    summary = str(data.get("summary") or "").strip()
    if len(summary) > 500:
        raise UpdateManifestError("更新摘要过长")
    published_at = str(data.get("published_at") or "").strip()
    if len(published_at) > 64:
        raise UpdateManifestError("发布时间字段过长")
    return ReleaseInfo(
        version=version.removeprefix("v"),
        published_at=published_at,
        release_url=release_url,
        summary=summary,
        channel=channel,
    )


def fetch_release_info(
    *,
    opener: Callable[..., object] = urlopen,
    timeout: int = REQUEST_TIMEOUT_SECONDS,
) -> ReleaseInfo:
    errors: list[str] = []
    for source in UPDATE_MANIFEST_SOURCES:
        request = Request(
            source.url,
            headers={
                "Accept": "application/json",
                "User-Agent": f"SYSUNetAuth/{APP_VERSION}",
            },
            method="GET",
        )
        try:
            response = opener(request, timeout=timeout)
            with response:  # type: ignore[attr-defined]
                final_url_getter = getattr(response, "geturl", None)
                final_url = (
                    str(final_url_getter())
                    if callable(final_url_getter)
                    else source.url
                )
                if not is_safe_external_url(
                    final_url,
                    allowed_hosts=set(source.allowed_hosts),
                ):
                    raise UpdateManifestError("重定向地址不受信任")
                payload = response.read(  # type: ignore[attr-defined]
                    MAX_MANIFEST_BYTES + 1
                )
            if len(payload) > MAX_MANIFEST_BYTES:
                raise UpdateManifestError("清单超过大小限制")
            return dataclass_replace(
                parse_release_manifest(payload),
                source=source.name,
            )
        except Exception as exc:
            errors.append(f"{source.name}: {exc}")
    raise UpdateManifestError("所有更新源均不可用（" + "; ".join(errors) + "）")
