from __future__ import annotations

import json
import os
import time
from dataclasses import asdict, dataclass, fields
from pathlib import Path

APP_DISPLAY_NAME = "SYSU NetAuth"
APP_ID = "SYSUNetAuth"
APP_VERSION = "0.6.0"

PROGRAMDATA_ROOT = Path(os.environ.get("PROGRAMDATA", r"C:\ProgramData"))
APP_DIR = PROGRAMDATA_ROOT / APP_ID
CONFIG_PATH = APP_DIR / "config.json"


@dataclass
class AppConfig:
    # ── NetID ──
    username: str = ""
    password: str = ""
    iface: str = ""
    iface_mode: str = "auto"  # "auto" | "manual"

    # ── 认证 ──
    auto_auth: bool = True
    retry_interval: int = 60  # 秒

    # ── 行为 ──
    service_mode: bool = True  # 以服务模式自动认证（独立于 GUI）
    launch_gui_on_login: bool = False
    hide_window_on_login: bool = True
    desktop_notify: bool = True

    # ── 缓存 ──
    last_success_iface: str = ""
    last_success_mac: str = ""


CONFIG_RANGES = {
    "retry_interval": (15, 3600),
}


def _require_str(data: dict, key: str, default: str = "") -> str:
    value = data.get(key, default)
    if not isinstance(value, str):
        raise ValueError(f"{key} 必须是字符串")
    return value


def _require_bool(data: dict, key: str, default: bool) -> bool:
    value = data.get(key, default)
    if type(value) is not bool:
        raise ValueError(f"{key} 必须是 true 或 false")
    return value


def _require_int(data: dict, key: str, default: int) -> int:
    value = data.get(key, default)
    if type(value) is not int:
        raise ValueError(f"{key} 必须是整数")
    low, high = CONFIG_RANGES[key]
    if not low <= value <= high:
        raise ValueError(f"{key} 必须在 {low} 到 {high} 之间")
    return value


def validate_config_data(data: dict) -> AppConfig:
    """Validate raw JSON data and return a normalized AppConfig."""
    if not isinstance(data, dict):
        raise ValueError("配置文件顶层必须是 JSON 对象")

    data = dict(data)

    allowed = {field.name for field in fields(AppConfig)}
    unknown = sorted(set(data) - allowed)
    if unknown:
        raise ValueError(f"未知配置字段：{', '.join(unknown)}")

    defaults = AppConfig()
    iface_mode = _require_str(data, "iface_mode", defaults.iface_mode)
    if iface_mode not in ("auto", "manual"):
        raise ValueError("iface_mode 必须是 auto 或 manual")

    service_mode = _require_bool(data, "service_mode", defaults.service_mode)
    launch_gui_on_login = _require_bool(
        data, "launch_gui_on_login", defaults.launch_gui_on_login
    )
    hide_window_on_login = _require_bool(
        data, "hide_window_on_login", defaults.hide_window_on_login
    )
    desktop_notify = _require_bool(data, "desktop_notify", defaults.desktop_notify)

    return AppConfig(
        username=_require_str(data, "username"),
        password=_require_str(data, "password"),
        iface=_require_str(data, "iface"),
        iface_mode=iface_mode,
        auto_auth=_require_bool(data, "auto_auth", defaults.auto_auth),
        retry_interval=_require_int(data, "retry_interval", defaults.retry_interval),
        service_mode=service_mode,
        launch_gui_on_login=launch_gui_on_login,
        hide_window_on_login=hide_window_on_login,
        desktop_notify=desktop_notify,
        last_success_iface=_require_str(data, "last_success_iface"),
        last_success_mac=_require_str(data, "last_success_mac"),
    )


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
        data = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        _backup_invalid_config(path)
        return AppConfig()
    try:
        return validate_config_data(data)
    except ValueError:
        return AppConfig()


def save_config(config: AppConfig, path: Path = CONFIG_PATH) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = asdict(config)
    # 密码明文保存，便于排查认证问题。
    # 清理空值，保持配置文件简洁
    for key in ("last_success_iface", "last_success_mac"):
        if not data.get(key):
            data.pop(key, None)
    tmp_path = path.with_name(f"{path.name}.tmp")
    tmp_path.write_text(
        json.dumps(data, ensure_ascii=False, indent=2), encoding="utf-8"
    )
    tmp_path.replace(path)


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
