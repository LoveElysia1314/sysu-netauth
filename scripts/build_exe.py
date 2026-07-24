"""
SYSU NetAuth — PyInstaller 独立打包脚本

用法:
    python scripts/build_exe.py                          # 打包为单目录（推荐）
    python scripts/build_exe.py --onefile                # 打包为单文件
    python scripts/build_exe.py --outdir dist/sysu_netauth
    python scripts/build_exe.py --force                  # 强制重新打包（忽略缓存）
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import sys
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
DIST_DIR = PROJECT_ROOT / "dist"
APP_NAME = "sysu_netauth"
SERVICE_APP_NAME = "sysu_netauth_service"
BUILD_HASH_FILE = "dist/.sysu_netauth_exe_hash"
ICO_SRC = PROJECT_ROOT / "sysu_netauth" / "assets" / "icon-ethernet.ico"

# ── 核心依赖（版本变更时触发重建） ───
_CORE_DEPS = ("psutil", "pywin32", "PySide6", "scapy", "PyInstaller")


def _site_packages_hash() -> str:
    """对核心依赖的版本计算哈希，版本升级时触发重建。"""
    h = hashlib.sha256()
    try:
        import importlib.metadata as importlib_metadata

        for name in sorted(_CORE_DEPS):
            try:
                ver = importlib_metadata.version(name)
                h.update(f"{name}=={ver}".encode("utf-8"))
            except Exception:
                pass
    except Exception:
        pass
    return h.hexdigest()


def _collect_source_files() -> list[Path]:
    """收集影响构建产物的所有源码与资源文件（排除 __pycache__）。"""
    files: list[Path] = []
    # sysu_netauth/ 目录下的所有文件
    src_dir = PROJECT_ROOT / "sysu_netauth"
    if src_dir.is_dir():
        for p in src_dir.rglob("*"):
            if p.is_file() and "__pycache__" not in p.parts:
                files.append(p)
    # 打包脚本自身——修改会影响输出产物
    for script in (
        "scripts/build_exe.py",
        "scripts/build.py",
        "run.py",
        "run_service.py",
    ):
        fp = PROJECT_ROOT / script
        if fp.is_file():
            files.append(fp)
    return files


def _compute_build_hash() -> str:
    """综合源码、资源、脚本、依赖元数据的构建哈希。"""
    h = hashlib.sha256()
    for fp in sorted(_collect_source_files(), key=lambda x: str(x.resolve())):
        rel = fp.relative_to(PROJECT_ROOT)
        h.update(str(rel).encode("utf-8"))
        try:
            h.update(fp.read_bytes())
        except Exception:
            pass
    h.update(_site_packages_hash().encode("utf-8"))
    return h.hexdigest()


def _is_build_cached(outdir_path: Path) -> bool:
    """检查哈希缓存与输出目录是否匹配。"""
    hash_path = PROJECT_ROOT / BUILD_HASH_FILE
    if not hash_path.is_file():
        return False
    if (
        not outdir_path.is_dir()
        or not (outdir_path / f"{APP_NAME}.exe").is_file()
        or not (outdir_path / f"{SERVICE_APP_NAME}.exe").is_file()
    ):
        return False
    try:
        data = json.loads(hash_path.read_text(encoding="utf-8"))
        return data.get("hash") == _compute_build_hash()
    except Exception:
        return False


def _save_build_hash() -> None:
    """保存构建哈希到缓存文件。"""
    h = _compute_build_hash()
    hash_path = PROJECT_ROOT / BUILD_HASH_FILE
    hash_path.parent.mkdir(parents=True, exist_ok=True)
    hash_path.write_text(
        json.dumps({"hash": h, "version": 2}, ensure_ascii=False),
        encoding="utf-8",
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="SYSU NetAuth PyInstaller 打包")
    parser.add_argument(
        "--onefile",
        action="store_true",
        help="打包为单文件（启动较慢）；默认不传即为文件夹模式",
    )
    parser.add_argument(
        "--outdir", default=str(DIST_DIR / "sysu_netauth"), help="输出目录"
    )
    parser.add_argument("--clean", action="store_true", help="清理旧的 build/dist")
    parser.add_argument("--force", action="store_true", help="忽略缓存，强制重新打包")
    return parser.parse_args()


def build_spec_content(onefile: bool, collect_name: str) -> str:
    excludes_list = [
        "IPython",
        "jupyter",
        "jupyter_client",
        "jupyter_console",
        "jupyter_core",
        "matplotlib",
        "notebook",
        "numpy",
        "pandas",
        "PIL",
        "tkinter",
        "test",
        "unittest",
        "setuptools",
        "pip",
        "packaging",
    ]
    excludes = ",\n        ".join(repr(e) for e in excludes_list)
    hidden = [
        "scapy.all",
        "scapy.arch.windows",
        "scapy.layers.l2",
        "scapy.layers.eap",
        "PySide6",
        "PySide6.QtCore",
        "PySide6.QtGui",
        "PySide6.QtSvg",
        "PySide6.QtWidgets",
        "psutil",
        "win32crypt",
        "win32com.client",
    ]
    service_hidden = [
        "scapy.all",
        "scapy.arch.windows",
        "scapy.layers.l2",
        "scapy.layers.eap",
        "psutil",
        "servicemanager",
        "win32event",
        "win32service",
        "win32serviceutil",
        "win32timezone",
    ]
    hiddenimports = ",\n        ".join(repr(e) for e in hidden)
    service_hiddenimports = ",\n        ".join(repr(e) for e in service_hidden)
    exe_payload = "a.binaries,\n    a.datas," if onefile else ""
    exclude_binaries = "False" if onefile else "True"
    collect = ""
    if not onefile:
        collect = f"""
coll = COLLECT(
    exe,
    service_exe,
    a.binaries,
    a.datas,
    service_a.binaries,
    service_a.datas,
    strip=False,
    upx=True,
    upx_exclude=[],
    name={collect_name!r},
)
"""

    # ── 收集 assets 下所有 ICO/SVG 资源文件 ──────────────
    assets_dir = PROJECT_ROOT / "sysu_netauth" / "assets"
    asset_datas_lines: list[str] = []
    if assets_dir.is_dir():
        for fp in sorted(assets_dir.iterdir()):
            if fp.is_file() and fp.suffix.lower() in (".ico", ".svg"):
                dest = "sysu_netauth/assets"
                asset_datas_lines.append(f"        ({repr(str(fp))}, {repr(dest)}),")
    asset_datas = "\n".join(asset_datas_lines) if asset_datas_lines else ""

    return f"""# -*- mode: python ; coding: utf-8 -*-
# Auto-generated by scripts/build_exe.py


a = Analysis(
    ['run.py'],
    pathex=[],
    binaries=[],
    datas=[
{asset_datas}
    ],
    hiddenimports=[
        {hiddenimports}
    ],
    hookspath=[],
    hooksconfig={{}},
    runtime_hooks=[],
    excludes=[
        {excludes}
    ],
    noarchive=False,
    optimize=0,
)
pyz = PYZ(a.pure)

exe = EXE(
    pyz,
    a.scripts,
    {exe_payload}
    [],
    name={APP_NAME!r},
    icon={repr(str(ICO_SRC))},
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    exclude_binaries={exclude_binaries},
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    uac_admin=True,
)

service_a = Analysis(
    ['run_service.py'],
    pathex=[],
    binaries=[],
    datas=[],
    hiddenimports=[
        {service_hiddenimports}
    ],
    hookspath=[],
    hooksconfig={{}},
    runtime_hooks=[],
    excludes=[
        'PySide6',
        'PySide6.QtCore',
        'PySide6.QtGui',
        'PySide6.QtSvg',
        'PySide6.QtWidgets',
        {excludes}
    ],
    noarchive=False,
    optimize=0,
)
service_pyz = PYZ(service_a.pure)

service_exe = EXE(
    service_pyz,
    service_a.scripts,
    {exe_payload.replace("a.", "service_a.")}
    [],
    name={SERVICE_APP_NAME!r},
    icon={repr(str(ICO_SRC))},
    debug=False,
    bootloader_ignore_signals=False,
    strip=False,
    upx=True,
    upx_exclude=[],
    runtime_tmpdir=None,
    console=False,
    exclude_binaries={exclude_binaries},
    disable_windowed_traceback=False,
    argv_emulation=False,
    target_arch=None,
    codesign_identity=None,
    entitlements_file=None,
    uac_admin=True,
)
{collect}"""


def ensure_pyinstaller_available() -> None:
    if (
        importlib.util.find_spec("PyInstaller") is not None
        and importlib.util.find_spec("PyInstaller.__main__") is not None
    ):
        return

    install_cmd = f'"{sys.executable}" -m pip install PyInstaller'
    print("[FAIL] 未安装 PyInstaller，无法执行打包。", file=sys.stderr)
    print(f"       请先运行: {install_cmd}", file=sys.stderr)
    raise SystemExit(2)


def main() -> None:
    args = parse_args()
    outdir_path = Path(os.path.abspath(args.outdir))

    ensure_pyinstaller_available()

    if args.clean:
        for p in [PROJECT_ROOT / "build", PROJECT_ROOT / "dist"]:
            if p.exists():
                shutil.rmtree(p, ignore_errors=True)

    # ── 哈希缓存检查（--force 跳过） ──
    if not args.force and not args.clean and _is_build_cached(outdir_path):
        print("[CACHE] 源码、资源、依赖均未变动，跳过打包（使用已有缓存）")
        print(f"[CACHE] 输出: {outdir_path}")
        return

    if args.onefile:
        distpath = outdir_path
        collect_name = outdir_path.name
    else:
        distpath = outdir_path.parent
        collect_name = outdir_path.name
    distpath.mkdir(parents=True, exist_ok=True)

    spec_content = build_spec_content(args.onefile, collect_name)
    spec_path = PROJECT_ROOT / "sysu_netauth.spec"
    spec_path.write_text(spec_content, encoding="utf-8")
    print(f"[OK] Spec 已生成: {spec_path}")

    import PyInstaller.__main__

    cmd = [
        str(spec_path),
        "--clean",
        "--noconfirm",
        "--distpath",
        str(distpath),
        "--workpath",
        str(PROJECT_ROOT / "build"),
    ]
    print("[BUILD] 正在打包...")
    PyInstaller.__main__.run(cmd)
    _save_build_hash()
    print(f"[OK] 打包完成: {outdir_path}")


if __name__ == "__main__":
    main()
