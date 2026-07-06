from __future__ import annotations

import json
import os
import time
from dataclasses import asdict, dataclass, fields
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

APP_DISPLAY_NAME = "SYSU NetAuth"
APP_ID = "SYSUNetAuth"
APP_VERSION = "0.6.1"

PROGRAMDATA_ROOT = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData"))
APP_DIR = PROGRAMDATA_ROOT / APP_ID
CONFIG_PATH = APP_DIR / "config.json"
STATUS_PATH = APP_DIR / "status.json"
COMMAND_PATH = APP_DIR / "command.json"


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
    updated_at: float = 0.0
    authenticated_at: str | None = None


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
    except json.JSONDecodeError:
        _backup_invalid_config(path)
        return AppConfig()
    if not isinstance(raw, dict):
        return AppConfig()
    config = _coerce_config(raw)
    save_config(config, path)
    return config


def save_config(config: AppConfig, path: Path = CONFIG_PATH) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = asdict(config)
    if not data.get("last_success_mac"):
        data.pop("last_success_mac", None)
    tmp_path = path.with_name(f"{path.name}.tmp")
    tmp_path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    tmp_path.replace(path)


# ── Status / Command store ───────────────────────────────────────────────────


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def read_status() -> ServiceStatus:
    data = _read_json(STATUS_PATH)
    if not data:
        return ServiceStatus(state="stopped", message="服务状态不可用")
    return ServiceStatus(
        state=str(data.get("state") or "stopped"),
        message=str(data.get("message") or ""),
        iface=str(data.get("iface") or ""),
        mac=str(data.get("mac") or ""),
        ipv4=str(data.get("ipv4") or ""),
        gateway=str(data.get("gateway") or ""),
        dns=str(data.get("dns") or ""),
        updated_at=float(data.get("updated_at", 0)),
        authenticated_at=data.get("authenticated_at"),
    )


def write_status(status: ServiceStatus) -> None:
    data = asdict(status)
    if not data.get("updated_at"):
        data["updated_at"] = time.monotonic()
    _atomic_write_json(STATUS_PATH, data)


def update_status(
    state: str,
    message: str = "",
    *,
    iface: str = "",
    mac: str = "",
    ipv4: str = "",
    gateway: str = "",
    dns: str = "",
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
            updated_at=time.monotonic(),
            authenticated_at=authenticated_at,
        )
    )


def read_command(delete: bool = True) -> str | None:
    data = _read_json(COMMAND_PATH)
    if not data:
        return None
    action = data.get("action")
    if delete:
        try:
            COMMAND_PATH.unlink(missing_ok=True)
        except Exception:
            pass
    return str(action) if action else None


def write_command(action: str) -> None:
    _atomic_write_json(
        COMMAND_PATH,
        {"action": action, "created_at": utc_now_iso()},
    )


def _atomic_write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_name(f"{path.name}.tmp")
    tmp_path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    for attempt in range(3):
        try:
            tmp_path.replace(path)
            return
        except PermissionError:
            if attempt < 2:
                time.sleep(0.1)
    path.write_text(json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8")
    try:
        tmp_path.unlink(missing_ok=True)
    except Exception:
        pass


def _read_json(path: Path) -> dict[str, Any] | None:
    try:
        if not path.exists():
            return None
        data = json.loads(path.read_text(encoding="utf-8"))
        return data if isinstance(data, dict) else None
    except Exception:
        return None


# ── Startup shortcut ─────────────────────────────────────────────────────────


def startup_shortcut_path() -> Path:
    appdata = Path(os.environ.get("APPDATA", str(Path.home())))
    return (
        appdata
        / "Microsoft"
        / "Windows"
        / "Start Menu"
        / "Programs"
        / "Startup"
        / f"{APP_DISPLAY_NAME}.lnk"
    )


def set_gui_launch_on_login(enabled: bool) -> None:
    """Create or remove the current user's GUI startup shortcut."""
    shortcut_path = startup_shortcut_path()
    if not enabled:
        shortcut_path.unlink(missing_ok=True)
        return

    import sys

    shortcut_path.parent.mkdir(parents=True, exist_ok=True)
    target = Path(sys.executable)
    workdir = target.parent
    try:
        import win32com.client  # type: ignore[import-untyped]

        shell = win32com.client.Dispatch("WScript.Shell")
        shortcut = shell.CreateShortCut(str(shortcut_path))
        shortcut.TargetPath = str(target)
        shortcut.Arguments = "--startup"
        shortcut.WorkingDirectory = str(workdir)
        shortcut.IconLocation = str(target)
        shortcut.save()
    except Exception as exc:
        raise RuntimeError(f"无法创建登录启动快捷方式：{exc}") from exc
