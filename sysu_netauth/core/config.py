from __future__ import annotations

import json
import os
import tempfile
import time
import uuid
from dataclasses import asdict, dataclass, fields
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

APP_DISPLAY_NAME = "SYSU NetAuth"
APP_ID = "SYSUNetAuth"
APP_VERSION = "0.6.2"

PROGRAMDATA_ROOT = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData"))
APP_DIR = PROGRAMDATA_ROOT / APP_ID
CONFIG_PATH = APP_DIR / "config.json"
STATUS_PATH = APP_DIR / "status.json"
COMMAND_PATH = APP_DIR / "command.json"
COMMAND_DIR = APP_DIR / "commands"
SERVICE_CACHE_PATH = APP_DIR / "service_cache.json"
UPDATE_STATE_PATH = APP_DIR / "update_state.json"
UI_STATE_PATH = APP_DIR / "ui_state.json"


@dataclass
class AppConfig:
    username: str = ""
    password: str = ""
    iface: str = ""
    iface_mode: str = "auto"
    auto_auth: bool = True
    retry_interval: int = 60
    service_mode: bool = True
    launch_gui_on_login: bool = False
    hide_window_on_login: bool = True
    desktop_notify: bool = True
    last_success_mac: str = ""


@dataclass(frozen=True)
class ServiceStatus:
    state: str = "idle"
    message: str = ""
    iface: str = ""
    mac: str = ""
    ipv4: str = ""
    gateway: str = ""
    dns: str = ""
    driver: str = ""
    updated_at: float = 0.0
    authenticated_at: str | None = None


@dataclass(frozen=True)
class ServiceCache:
    """服务拥有的运行时缓存，避免后台服务回写用户配置。"""

    iface: str = ""
    last_success_mac: str = ""


@dataclass(frozen=True)
class UpdateState:
    """服务拥有的更新检查结果。"""

    schema_version: int = 1
    status: str = "never"
    current_version: str = APP_VERSION
    latest_version: str = ""
    available: bool = False
    release_url: str = ""
    summary: str = ""
    source: str = ""
    checked_at: float = 0.0
    next_check_at: float = 0.0
    failure_count: int = 0
    error: str = ""


@dataclass(frozen=True)
class UpdateUiState:
    """GUI 拥有的更新提示状态，避免服务和 GUI 写同一个文件。"""

    notified_version: str = ""
    ignored_version: str = ""


def _coerce_config(data: dict) -> AppConfig:
    """从 JSON dict 构建 AppConfig，过滤未知字段，缺失/非法值回退默认值。"""
    defaults = AppConfig()
    merged: dict[str, object] = {}
    for f in fields(AppConfig):
        raw = data.get(f.name)
        target = type(getattr(defaults, f.name))
        if target is str:
            merged[f.name] = raw if isinstance(raw, str) else getattr(defaults, f.name)
        elif target is bool:
            merged[f.name] = raw if type(raw) is bool else getattr(defaults, f.name)
        elif target is int:
            merged[f.name] = raw if type(raw) is int else getattr(defaults, f.name)
    if merged.get("iface_mode") not in ("auto", "manual"):
        merged["iface_mode"] = defaults.iface_mode
    ri = merged.get("retry_interval", defaults.retry_interval)
    if not isinstance(ri, int) or not 15 <= ri <= 3600:
        merged["retry_interval"] = defaults.retry_interval
    return AppConfig(**merged)  # type: ignore[arg-type]


def _backup_invalid_config(path: Path) -> None:
    if not path.exists():
        return
    try:
        backup = path.with_suffix(
            path.suffix + f".invalid-{time.strftime('%Y%m%d-%H%M%S')}"
        )
        path.replace(backup)
    except Exception:
        pass


def load_config(path: Path = CONFIG_PATH) -> AppConfig:
    if not path.exists():
        return AppConfig()
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except (json.JSONDecodeError, UnicodeError):
        _backup_invalid_config(path)
        return AppConfig()
    if not isinstance(raw, dict):
        return AppConfig()
    # 读取必须是无副作用操作。GUI 和 Windows 服务都会频繁读取配置；
    # 若读取时也回写，两个进程会争用同一个目标文件并可能使服务崩溃。
    return _coerce_config(raw)


def save_config(config: AppConfig, path: Path = CONFIG_PATH) -> None:
    data = asdict(config)
    if not data.get("last_success_mac"):
        data.pop("last_success_mac", None)
    _atomic_write_json(path, data)


# ── Status / Command store ───────────────────────────────────────────────────


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def read_status() -> ServiceStatus:
    data = _read_json(STATUS_PATH)
    if not data:
        return ServiceStatus(state="stopped", message="服务状态不可用")
    try:
        updated_at = float(data.get("updated_at", 0))
    except (TypeError, ValueError):
        updated_at = 0.0
    authenticated_at = data.get("authenticated_at")
    return ServiceStatus(
        state=str(data.get("state") or "stopped"),
        message=str(data.get("message") or ""),
        iface=str(data.get("iface") or ""),
        mac=str(data.get("mac") or ""),
        ipv4=str(data.get("ipv4") or ""),
        gateway=str(data.get("gateway") or ""),
        dns=str(data.get("dns") or ""),
        driver=str(data.get("driver") or ""),
        updated_at=updated_at,
        authenticated_at=str(authenticated_at) if authenticated_at else None,
    )


def write_status(status: ServiceStatus) -> None:
    data = asdict(status)
    if not data.get("updated_at"):
        data["updated_at"] = time.time()
    _atomic_write_json(STATUS_PATH, data, durable=False)


def update_status(
    state: str,
    message: str = "",
    *,
    iface: str = "",
    mac: str = "",
    ipv4: str = "",
    gateway: str = "",
    dns: str = "",
    driver: str = "",
    authenticated_at: str | None = None,
) -> None:
    write_status(
        ServiceStatus(
            state=state,
            message=message,
            iface=iface,
            mac=mac,
            ipv4=ipv4,
            gateway=gateway,
            dns=dns,
            driver=driver,
            updated_at=time.time(),
            authenticated_at=authenticated_at,
        )
    )


def read_command(delete: bool = True) -> str | None:
    # 兼容旧版本及 README 中的手工 command.json 写入方式。
    candidates = [COMMAND_PATH]
    try:
        candidates.extend(sorted(COMMAND_DIR.glob("*.json")))
    except OSError:
        pass
    for path in candidates:
        data = _read_json(path)
        if not data:
            if path != COMMAND_PATH:
                try:
                    path.unlink(missing_ok=True)
                except OSError:
                    pass
            continue
        action = data.get("action")
        if delete:
            try:
                path.unlink(missing_ok=True)
            except OSError:
                pass
        if action:
            return str(action)
    return None


def write_command(action: str) -> None:
    command_path = COMMAND_DIR / (
        f"{time.time_ns():020d}-{os.getpid()}-{uuid.uuid4().hex}.json"
    )
    _atomic_write_json(
        command_path,
        {"action": action, "created_at": utc_now_iso()},
        durable=False,
    )


def _atomic_write_json(
    path: Path,
    data: dict[str, Any],
    *,
    durable: bool = True,
) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    payload = json.dumps(data, ensure_ascii=False, indent=2)
    fd, tmp_name = tempfile.mkstemp(
        dir=path.parent,
        prefix=f"{path.name}.",
        suffix=".tmp",
    )
    tmp_path = Path(tmp_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8", newline="\n") as stream:
            stream.write(payload)
            stream.flush()
            if durable:
                os.fsync(stream.fileno())

        # Windows 上杀毒软件或另一个进程的短暂文件句柄可能让 replace
        # 返回 WinError 5。独立临时文件避免写入者互相覆盖，短暂占用则重试。
        for attempt in range(5):
            try:
                os.replace(tmp_path, path)
                return
            except PermissionError:
                if attempt == 4:
                    raise
                time.sleep(0.05 * (attempt + 1))
    finally:
        try:
            tmp_path.unlink(missing_ok=True)
        except OSError:
            pass


def _read_json(path: Path) -> dict[str, Any] | None:
    try:
        if not path.exists():
            return None
        data = json.loads(path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else None
    except Exception:
        return None


# ── Service-owned runtime cache ──────────────────────────────────────────────


def load_service_cache() -> ServiceCache:
    data = _read_json(SERVICE_CACHE_PATH) or {}
    return ServiceCache(
        iface=str(data.get("iface") or ""),
        last_success_mac=str(data.get("last_success_mac") or ""),
    )


def save_service_cache(cache: ServiceCache) -> None:
    _atomic_write_json(SERVICE_CACHE_PATH, asdict(cache))


# ── Update state stores ──────────────────────────────────────────────────────


def load_update_state(path: Path = UPDATE_STATE_PATH) -> UpdateState:
    data = _read_json(path) or {}
    try:
        checked_at = float(data.get("checked_at", 0))
    except (TypeError, ValueError):
        checked_at = 0.0
    try:
        next_check_at = float(data.get("next_check_at", 0))
    except (TypeError, ValueError):
        next_check_at = 0.0
    try:
        failure_count = max(0, int(data.get("failure_count", 0)))
    except (TypeError, ValueError):
        failure_count = 0
    status = str(data.get("status") or "never")
    if status not in {"never", "waiting", "checking", "success", "error"}:
        status = "never"
    return UpdateState(
        schema_version=1,
        status=status,
        current_version=str(data.get("current_version") or APP_VERSION),
        latest_version=str(data.get("latest_version") or ""),
        available=type(data.get("available")) is bool and data["available"],
        release_url=str(data.get("release_url") or ""),
        summary=str(data.get("summary") or ""),
        source=str(data.get("source") or ""),
        checked_at=checked_at,
        next_check_at=next_check_at,
        failure_count=failure_count,
        error=str(data.get("error") or ""),
    )


def save_update_state(
    state: UpdateState,
    path: Path = UPDATE_STATE_PATH,
) -> None:
    _atomic_write_json(path, asdict(state), durable=False)


def load_update_ui_state(path: Path = UI_STATE_PATH) -> UpdateUiState:
    data = _read_json(path) or {}
    return UpdateUiState(
        notified_version=str(data.get("notified_version") or ""),
        ignored_version=str(data.get("ignored_version") or ""),
    )


def save_update_ui_state(
    state: UpdateUiState,
    path: Path = UI_STATE_PATH,
) -> None:
    _atomic_write_json(path, asdict(state), durable=False)
