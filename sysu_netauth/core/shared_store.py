from __future__ import annotations

import json
import time
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .config import APP_DIR, CONFIG_PATH, AppConfig, load_config, save_config

STATUS_PATH = APP_DIR / "status.json"
COMMAND_PATH = APP_DIR / "command.json"


@dataclass(frozen=True)
class ServiceStatus:
    state: str = "idle"
    message: str = ""
    iface: str = ""
    mac: str = ""
    ipv4: str = ""
    updated_at: float = 0.0
    authenticated_at: str | None = None


def utc_now_iso() -> str:
    return datetime.now(timezone.utc).astimezone().isoformat(timespec="seconds")


def _atomic_write_json(path: Path, data: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp_path = path.with_name(f"{path.name}.tmp")
    tmp_path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    # 原子替换可能因安全软件扫描、文件权限临时异常而失败。
    # 重试至多 3 次，每次间隔 100ms。全部失败时回退到直接 write_text
    # （非原子但保证服务不崩溃，GUI 轮询读到半写文件时判 stale 即可）。
    for attempt in range(3):
        try:
            tmp_path.replace(path)
            return
        except PermissionError:
            if attempt < 2:
                time.sleep(0.1)
    # 兜底：直接写入目标文件（非原子，但不至于让服务崩溃）
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


def ensure_shared_config() -> AppConfig:
    return load_config(CONFIG_PATH)


def save_shared_config(config: AppConfig) -> None:
    save_config(config, CONFIG_PATH)


def _parse_updated_at(raw: Any) -> float:
    """解析 updated_at，支持新格式 (float) 和旧格式 (ISO 8601 str)。"""
    if isinstance(raw, (int, float)):
        return float(raw)
    return 0.0


def read_status() -> ServiceStatus:
    data = _read_json(STATUS_PATH)
    if not data:
        return ServiceStatus(
            state="stopped",
            message="服务状态不可用",
        )
    return ServiceStatus(
        state=str(data.get("state") or "stopped"),
        message=str(data.get("message") or ""),
        iface=str(data.get("iface") or ""),
        mac=str(data.get("mac") or ""),
        ipv4=str(data.get("ipv4") or ""),
        updated_at=_parse_updated_at(data.get("updated_at")),
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
    authenticated_at: str | None = None,
) -> None:
    write_status(
        ServiceStatus(
            state=state,
            message=message,
            iface=iface,
            mac=mac,
            ipv4=ipv4,
            updated_at=time.monotonic(),
            authenticated_at=authenticated_at,
        )
    )


def write_command(action: str) -> None:
    _atomic_write_json(
        COMMAND_PATH,
        {
            "action": action,
            "created_at": utc_now_iso(),
        },
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
