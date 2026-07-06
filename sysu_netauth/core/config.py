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
    last_success_mac: str = ""


CONFIG_RANGES = {
    "retry_interval": (15, 3600),
}

# 哨兵值，用于区分"键不存在"和"值为 None"
_sentinel = object()


def _coerce_field(name: str, raw_value: object, default: object) -> object:
    """将原始值强制转换为字段的目标类型，失败时返回 default。"""
    target_type = type(default)
    # None / 空字符串对 str 字段视为"未设置"，保留原始值（可能是 ""）
    if target_type is str:
        return raw_value if isinstance(raw_value, str) else default
    if target_type is bool:
        return raw_value if type(raw_value) is bool else default
    if target_type is int:
        return raw_value if type(raw_value) is int else default
    return raw_value


def _backup_invalid_config(path: Path) -> None:
    """将损坏的配置文件重命名为 .invalid- 后缀备份。"""
    if not path.exists():
        return
    try:
        backup = path.with_suffix(
            path.suffix + f".invalid-{time.strftime('%Y%m%d-%H%M%S')}"
        )
        path.replace(backup)
    except Exception:
        pass


def normalize_config_data(data: dict) -> AppConfig:
    """从原始 JSON dict 中过滤出已知字段，类型强制后返回干净的 AppConfig。

    自动丢弃未知字段（如旧版遗留的 last_success_iface），
    缺失字段用 dataclass 默认值填充，非法类型退回到默认值。
    """
    if not isinstance(data, dict):
        return AppConfig()

    allowed = {field.name for field in fields(AppConfig)}
    defaults = AppConfig()

    # 构建合并字典：已知字段 + 类型强制
    merged: dict[str, object] = {}
    for field in fields(AppConfig):
        raw = data.get(field.name, _sentinel)
        if raw is _sentinel:
            merged[field.name] = getattr(defaults, field.name)
            continue
        if field.name not in allowed:
            continue
        merged[field.name] = _coerce_field(
            field.name, raw, getattr(defaults, field.name)
        )

    # 特殊后处理
    if merged.get("iface_mode") not in ("auto", "manual"):
        merged["iface_mode"] = defaults.iface_mode
    retry = merged.get("retry_interval", defaults.retry_interval)
    low, high = CONFIG_RANGES["retry_interval"]
    if not isinstance(retry, int) or not low <= retry <= high:
        merged["retry_interval"] = defaults.retry_interval

    return AppConfig(**merged)  # type: ignore[arg-type]


def load_config(path: Path = CONFIG_PATH) -> AppConfig:
    """加载配置文件，自动清理未知字段并持久化回写。"""
    if not path.exists():
        return AppConfig()
    try:
        raw = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError:
        _backup_invalid_config(path)
        return AppConfig()
    config = normalize_config_data(raw)
    # 自动回写规范格式：丢弃未知字段、补充缺失字段、统一缩进
    save_config(config, path)
    return config


def save_config(config: AppConfig, path: Path = CONFIG_PATH) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    data = asdict(config)
    # 密码明文保存，便于排查认证问题。
    # 清理空值，保持配置文件简洁
    for key in ("last_success_mac",):
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
