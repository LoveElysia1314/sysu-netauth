"""
Asset path resolution for both frozen (PyInstaller) and development modes.

Provides a single source of truth for icon and resource file locations,
used by both runner.py and app/tray.py.
"""

from __future__ import annotations

import sys
from pathlib import Path


def resolve_asset_path(name: str, ext: str = "ico") -> Path:
    """Resolve the absolute path of an asset file.

    Supports both PyInstaller frozen builds and development mode.
    """
    filename = f"{name}.{ext}"
    if getattr(sys, "frozen", False):
        meipass = Path(getattr(sys, "_MEIPASS", Path(sys.executable).parent))
        exe_dir = Path(sys.executable).resolve().parent
        candidates = [
            meipass / "sysu_netauth" / "assets" / filename,
            exe_dir / "sysu_netauth" / "assets" / filename,
            meipass.parent / "sysu_netauth" / "assets" / filename,
        ]
        for candidate in candidates:
            if candidate.is_file():
                return candidate
        return candidates[0]
    return (
        Path(__file__).resolve().parent.parent.parent
        / "sysu_netauth"
        / "assets"
        / filename
    )
