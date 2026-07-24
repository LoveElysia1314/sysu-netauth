"""Generate pre-colored tray icons (blue/gray/orange/green/red)."""

from __future__ import annotations

import io
import struct
import sys
from pathlib import Path

from PIL import Image
from PySide6.QtCore import QByteArray, QBuffer, QIODevice, Qt
from PySide6.QtGui import QPixmap, QPainter, QGuiApplication
from PySide6.QtSvg import QSvgRenderer

TRAY_COLORS = {
    "blue": "#3b82f6",
    "gray": "#6b7280",
    "orange": "#f59e0b",
    "green": "#22c55e",
    "red": "#ef4444",
}
ICON_SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
ASSETS = Path(__file__).resolve().parents[1] / "sysu_netauth" / "assets"

_SHIELD = b"#4080ff"
_PORT = b"#22c55e"


def _svg_bytes() -> bytes:
    p = ASSETS / "icon-ethernet.svg"
    return p.read_bytes() if p.is_file() else b""


def _render(svg: QByteArray, size: int) -> Image.Image:
    px = QPixmap(size, size)
    px.fill(Qt.GlobalColor.transparent)
    QSvgRenderer(svg).render(QPainter(px))
    QPainter(px).end()
    b = QBuffer()
    b.open(QIODevice.OpenModeFlag.WriteOnly)
    px.save(b, "PNG")
    img = Image.open(io.BytesIO(bytes(b.data().data())))
    img.load()
    b.close()
    return img


def _write_ico(imgs: list[Image.Image], path: Path) -> None:
    n = len(imgs)
    hdr = struct.pack("<HHH", 0, 1, n)
    pngs = []
    for im in imgs:
        b = io.BytesIO()
        im.save(b, "PNG")
        pngs.append(b.getvalue())
    off = 6 + 16 * n
    ents = b""
    for s, p in zip(ICON_SIZES, pngs, strict=True):
        w = 0 if s >= 256 else s
        ents += struct.pack("<BBBBHHII", w, w, 0, 0, 1, 32, len(p), off)
        off += len(p)
    with open(path, "wb") as f:
        f.write(hdr + ents + b"".join(pngs))


def _color(raw: bytes, c: str) -> QByteArray:
    return QByteArray(raw.replace(_SHIELD, c.encode()).replace(_PORT, c.encode()))


def generate() -> None:
    raw = _svg_bytes()
    if not raw:
        print("ERROR: icon-ethernet.svg not found", file=sys.stderr)
        sys.exit(1)
    # Window icon (blue default)
    imgs = [_render(_color(raw, "#3b82f6"), s) for s in ICON_SIZES]
    _write_ico(imgs, ASSETS / "icon-ethernet.ico")
    print(
        f"  icon-ethernet.ico  ({Path(ASSETS/'icon-ethernet.ico').stat().st_size:,} bytes)"
    )
    # Tray icons
    for name, clr in TRAY_COLORS.items():
        imgs = [_render(_color(raw, clr), s) for s in ICON_SIZES]
        _write_ico(imgs, ASSETS / f"tray-{name}.ico")
        print(
            f"  tray-{name}.ico  ({Path(ASSETS/f'tray-{name}.ico').stat().st_size:,} bytes)"
        )


if __name__ == "__main__":
    _app = QGuiApplication([])
    print("Generating icons ...")
    generate()
    print("Done.")
