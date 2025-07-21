#!/usr/bin/env python
# -*- coding: utf-8 -*-
"""
SYSU NetAuth — PyInstaller 打包 & Inno Setup 安装程序制作脚本
============================================================

用法:
    python scripts/build.py                        # 完整构建：PyInstaller → 安装包
    python scripts/build.py --skip-installer       # 仅 PyInstaller 打包，跳过安装程序
    python scripts/build.py --clean                # 清理所有构建产物后重来
    python scripts/build.py --skip-pyinstaller     # 仅生成安装包（假设已打包）

输出:
    dist/sysu_netauth/                           PyInstaller 输出（单文件夹）
    SYSUNetAuth_Setup_v{VERSION}.exe                Inno Setup 安装包
    SYSUNetAuth_Portable_v{VERSION}.zip             便携版 ZIP

依赖:
    - Python ≥ 3.10
    - PyInstaller（构建前需安装）
    - Inno Setup 6（需单独安装，https://jrsoftware.org/isinfo.php）
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import site
import subprocess
import sys
import zipfile
from pathlib import Path

site.ENABLE_USER_SITE = False  # noqa

_project_root = Path(__file__).resolve().parent.parent
if str(_project_root) not in sys.path:
    sys.path.insert(0, str(_project_root))

# ═══════════════════════════════════════════════════════════════
# 项目常量
# ═══════════════════════════════════════════════════════════════

PROJECT_ROOT = Path(__file__).resolve().parent.parent

# 动态读取版本号
_init_text = (PROJECT_ROOT / "sysu_netauth" / "core" / "config.py").read_text(
    encoding="utf-8"
)
_match = re.search(r'APP_VERSION\s*=\s*"([^"]+)"', _init_text)
VERSION = _match.group(1) if _match else "0.0.0"

APP_NAME = "SYSU NetAuth"
APP_ID = "SYSUNetAuth"
APP_EXE_NAME = "sysu_netauth.exe"
APP_SERVICE_EXE_NAME = "sysu_netauth_service.exe"
PORTABLE_SCRIPT_DIR = PROJECT_ROOT / "scripts" / "portable"


class BuildDependencyError(RuntimeError):
    pass


def _subprocess_env() -> dict[str, str]:
    env = os.environ.copy()
    env.setdefault("PYTHONIOENCODING", "utf-8")
    return env


_force_build = False
_clean_build = False


class BuildConfig:
    def __init__(self):
        self.project_root = PROJECT_ROOT
        self.script_dir = Path(__file__).resolve().parent
        self.output_dir = self.project_root / "dist"
        self.app_dir = self.output_dir / "sysu_netauth"


# ═══════════════════════════════════════════════════════════════
# 清理
# ═══════════════════════════════════════════════════════════════


def clean_build_cache(config: BuildConfig):
    print("[1/4] 清理旧构建缓存...")
    for p in [
        config.project_root / "__pycache__",
        config.project_root / "build",
        config.project_root / "dist",
    ]:
        if p.exists():
            if p.is_dir():
                shutil.rmtree(p, ignore_errors=True)
            else:
                p.unlink(missing_ok=True)


# ═══════════════════════════════════════════════════════════════
# PyInstaller 打包
# ═══════════════════════════════════════════════════════════════


def run_pyinstaller_build(config: BuildConfig):
    print("[2/4] 执行 PyInstaller 打包（委托 build_exe.py）...")
    print("     打包模式：单目录（默认）")
    # 委托 build_exe.py（内置哈希缓存，源码/依赖未变动时自动跳过）
    build_exe = str(PROJECT_ROOT / "scripts" / "build_exe.py")
    cmd = [
        sys.executable,
        build_exe,
        "--outdir",
        str(config.app_dir),
    ]
    if _force_build:
        cmd.append("--force")
    try:
        result = subprocess.run(
            cmd,
            check=True,
            cwd=str(config.project_root),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
            env=_subprocess_env(),
        )
        for line in result.stdout.splitlines():
            print(f"  {line}")
        copy_portable_scripts(config)
        print("     PyInstaller 打包完成")
    except subprocess.CalledProcessError as e:
        # 失败时显示完整输出以便排查
        for label, data in [("stdout", e.stdout), ("stderr", e.stderr)]:
            if data:
                for line in str(data).splitlines()[-15:]:
                    print(f"  {label}: {line}")
        print(f"[2/4] PyInstaller 打包失败 (exit {e.returncode})")
        if e.returncode == 2:
            raise BuildDependencyError(
                f"缺少 PyInstaller；请运行: {sys.executable} -m pip install PyInstaller"
            ) from e
        raise


def copy_portable_scripts(config: BuildConfig) -> None:
    if not PORTABLE_SCRIPT_DIR.is_dir():
        return
    config.app_dir.mkdir(parents=True, exist_ok=True)
    for fp in PORTABLE_SCRIPT_DIR.iterdir():
        if fp.is_file():
            shutil.copy2(fp, config.app_dir / fp.name)


# ═══════════════════════════════════════════════════════════════
# Inno Setup 安装包
# ═══════════════════════════════════════════════════════════════


def find_inno_compiler() -> Path | None:
    # 1. 注册表查询 — 覆盖官方安装程序和 winget/choco 安装方式
    try:
        import winreg

        for hive in (winreg.HKEY_LOCAL_MACHINE, winreg.HKEY_CURRENT_USER):
            try:
                key = winreg.OpenKey(
                    hive, r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall"
                )
            except FileNotFoundError:
                continue
            try:
                i = 0
                while True:
                    try:
                        subkey_name = winreg.EnumKey(key, i)
                        i += 1
                    except OSError:
                        break
                    try:
                        subkey = winreg.OpenKey(key, subkey_name)
                    except OSError:
                        continue
                    try:
                        try:
                            display_name, _ = winreg.QueryValueEx(subkey, "DisplayName")
                        except FileNotFoundError:
                            continue
                        if "Inno Setup" not in display_name:
                            continue
                        install_loc, _ = winreg.QueryValueEx(subkey, "InstallLocation")
                        if install_loc:
                            iscc = Path(install_loc) / "ISCC.exe"
                            if iscc.is_file():
                                return iscc
                    finally:
                        winreg.CloseKey(subkey)
            finally:
                winreg.CloseKey(key)
    except Exception:
        pass

    # 2. 常见安装路径兜底
    #    %LOCALAPPDATA%\Programs\Inno Setup 6\ISCC.exe  (winget)
    #    %ProgramFiles(x86)%\Inno Setup 6\ISCC.exe       (官方安装)
    #    %ProgramFiles%\Inno Setup 6\ISCC.exe
    candidates = [
        Path.home() / "AppData" / "Local" / "Programs" / "Inno Setup 6" / "ISCC.exe",
        Path.home() / "AppData" / "Local" / "Programs" / "Inno Setup 7" / "ISCC.exe",
    ]
    pf86 = os.environ.get("ProgramFiles(x86)")
    if pf86:
        candidates.append(Path(pf86) / "Inno Setup 6" / "ISCC.exe")
        candidates.append(Path(pf86) / "Inno Setup 7" / "ISCC.exe")
    pf = os.environ.get("ProgramFiles")
    if pf:
        candidates.append(Path(pf) / "Inno Setup 6" / "ISCC.exe")
        candidates.append(Path(pf) / "Inno Setup 7" / "ISCC.exe")
    for p in candidates:
        if p.is_file():
            return p
    return None


def generate_iss_file(config: BuildConfig) -> Path:
    print("[3/4] 生成 Inno Setup 安装脚本...")
    exe_path = config.app_dir / APP_EXE_NAME
    if not exe_path.exists():
        raise FileNotFoundError(f"未找到可执行文件: {exe_path}")

    template_path = config.script_dir / "setup.template.iss"
    if not template_path.is_file():
        raise FileNotFoundError(f"找不到 ISS 模板: {template_path}")

    iss_content = template_path.read_text(encoding="utf-8")
    app_dir_relative = os.path.relpath(config.app_dir, config.script_dir)
    substitutions = {
        "@APP_NAME@": APP_NAME,
        "@APP_VERSION@": VERSION,
        "@APP_EXE_NAME@": APP_EXE_NAME,
        "@APP_SERVICE_EXE_NAME@": APP_SERVICE_EXE_NAME,
        "@APP_ID@": APP_ID,
        "@APP_DIR_RELATIVE@": app_dir_relative,
    }
    for placeholder, value in substitutions.items():
        iss_content = iss_content.replace(placeholder, value)

    iss_file = config.script_dir / "setup.iss"
    iss_file.write_text(iss_content, encoding="utf-8")
    return iss_file


def build_installer(config: BuildConfig):
    if not config.app_dir.exists():
        print("[4/4] 错误：构建输出目录不存在，请先执行 PyInstaller 打包")
        return False
    copy_portable_scripts(config)

    inno_compiler = find_inno_compiler()
    if not inno_compiler:
        print("[4/4] 未找到 Inno Setup 编译器 (ISCC.exe)")
        print("      请安装 Inno Setup 6: https://jrsoftware.org/isinfo.php")
        print("      或使用 --skip-installer 跳过")
        return False

    print(f"[4/4] 找到 Inno Setup: {inno_compiler}")
    iss_file = generate_iss_file(config)

    try:
        output_dir = config.script_dir / "Output"
        output_dir.mkdir(exist_ok=True)

        print("[4/4] 正在编译安装包...")
        result = subprocess.run(
            [str(inno_compiler), str(iss_file)],
            check=True,
            capture_output=True,
            text=True,
            cwd=str(config.script_dir),
            encoding="utf-8",
            errors="replace",
            env=_subprocess_env(),
        )

        try:
            iss_file.unlink()
        except Exception:
            pass

        setup_name = f"{APP_ID}_Setup_v{VERSION}.exe"
        setup_src = output_dir / setup_name
        if setup_src.exists():
            setup_dst = config.project_root / setup_name
            shutil.move(str(setup_src), str(setup_dst))
            print(f"[4/4] 安装包已生成: {setup_dst}")

            shutil.rmtree(output_dir, ignore_errors=True)
            create_portable_zip(config)
            return True
        else:
            print(f"[4/4] 警告：未找到生成的安装包 {setup_name}")
            return False

    except subprocess.CalledProcessError as e:
        print(f"[4/4] Inno Setup 编译失败 (exit {e.returncode})")
        if e.stdout:
            print(e.stdout[-2000:])
        if e.stderr:
            print(e.stderr[-2000:])
        return False


# ═══════════════════════════════════════════════════════════════
# 便携版 ZIP
# ═══════════════════════════════════════════════════════════════


def create_portable_zip(config: BuildConfig):
    if not config.app_dir.is_dir():
        return
    copy_portable_scripts(config)
    zip_name = f"{APP_ID}_Portable_v{VERSION}.zip"
    zip_path = config.project_root / zip_name
    zip_path.unlink(missing_ok=True)
    with zipfile.ZipFile(zip_path, "w", zipfile.ZIP_DEFLATED) as zf:
        for fp in config.app_dir.rglob("*"):
            if fp.is_file():
                arcname = str(fp.relative_to(config.app_dir.parent))
                zf.write(fp, arcname)
    size_mb = zip_path.stat().st_size / (1024 * 1024)
    print(f"   [+] 便携版 ZIP: {zip_name} ({size_mb:.1f} MB)")


# ═══════════════════════════════════════════════════════════════
# 收尾清理
# ═══════════════════════════════════════════════════════════════


def cleanup_temp_directories(config: BuildConfig):
    for p in [
        config.project_root / "build",
        config.script_dir / "Output",
    ]:
        if p.exists():
            try:
                shutil.rmtree(p)
            except Exception:
                pass
    for f in [
        config.script_dir / "setup.iss",
        config.project_root / "sysu_netauth.spec",
    ]:
        if f.exists():
            try:
                f.unlink()
            except Exception:
                pass


# ═══════════════════════════════════════════════════════════════
# 主流程
# ═══════════════════════════════════════════════════════════════


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="SYSU NetAuth 打包 & 安装程序制作",
    )
    parser.add_argument("--clean", action="store_true", help="强制清理后重来")
    parser.add_argument(
        "--skip-installer", action="store_true", help="跳过安装程序制作"
    )
    parser.add_argument(
        "--skip-pyinstaller", action="store_true", help="跳过打包（仅生成安装包）"
    )
    parser.add_argument(
        "--force", action="store_true", help="强制重新打包（忽略 build_exe 缓存）"
    )
    return parser.parse_args()


def main():
    args = parse_args()
    global _force_build, _clean_build
    _force_build = args.force
    _clean_build = args.clean
    config = BuildConfig()

    try:
        if args.clean:
            clean_build_cache(config)

        if not args.skip_pyinstaller:
            run_pyinstaller_build(config)
        else:
            print("[跳过] PyInstaller 打包")

        if not args.skip_installer:
            if not build_installer(config):
                raise RuntimeError("安装程序制作失败")
        else:
            print("[跳过] Inno Setup 安装程序制作")
            create_portable_zip(config)

        cleanup_temp_directories(config)

        print()
        print("=" * 60)
        print(f"  [OK] 构建成功 - {APP_NAME} v{VERSION}")
        print("=" * 60)

        exe_path = config.app_dir / APP_EXE_NAME
        if exe_path.exists():
            size_mb = exe_path.stat().st_size / (1024 * 1024)
            print(f"  可执行文件: {exe_path} ({size_mb:.1f} MB)")
        else:
            print(f"  输出目录:   {config.app_dir}")
        service_path = config.app_dir / APP_SERVICE_EXE_NAME
        if service_path.exists():
            size_mb = service_path.stat().st_size / (1024 * 1024)
            print(f"  服务程序:   {service_path} ({size_mb:.1f} MB)")

        setup_path = config.project_root / f"{APP_ID}_Setup_v{VERSION}.exe"
        if setup_path.exists():
            size_mb = setup_path.stat().st_size / (1024 * 1024)
            print(f"  安装包:     {setup_path} ({size_mb:.1f} MB)")

        port_zip = config.project_root / f"{APP_ID}_Portable_v{VERSION}.zip"
        if port_zip.exists():
            size_mb = port_zip.stat().st_size / (1024 * 1024)
            print(f"  便携版:     {port_zip} ({size_mb:.1f} MB)")

        print()
        print("  [*] 使用说明:")
        print(f"     安装包: 双击 {APP_ID}_Setup_v{VERSION}.exe")
        print(
            f"     便携版: 解压后右键管理员运行 Install-Service.cmd，再运行 Start-GUI.cmd"
        )
        print(f"     CLI:    sysu_netauth -i '以太网 4' -u 'NetID' -p 'your_password'")
        print()

    except BuildDependencyError as e:
        print(f"\n[FAIL] 构建失败: {e}")
        try:
            cleanup_temp_directories(config)
        except Exception:
            pass
        sys.exit(2)
    except Exception as e:
        print(f"\n[FAIL] 构建失败: {e}")
        import traceback

        traceback.print_exc()
        try:
            cleanup_temp_directories(config)
        except Exception:
            pass
        sys.exit(1)


if __name__ == "__main__":
    main()
