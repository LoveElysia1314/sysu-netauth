from __future__ import annotations

import ctypes.util
import os
import urllib.request

NPCAP_DOWNLOAD_URL = "https://npcap.com/#download"
# 固定最新版本号。有新版本时只需改这个常量即可
NPCAP_VERSION = "1.88"
NPCAP_INSTALLER_URL = f"https://npcap.com/dist/npcap-{NPCAP_VERSION}.exe"


def has_npcap() -> bool:
    if ctypes.util.find_library("wpcap"):
        return True
    windir = os.environ.get("WINDIR", r"C:\Windows")
    for base in (
        os.path.join(windir, "System32", "Npcap"),
        os.path.join(windir, "SysWOW64", "Npcap"),
    ):
        if os.path.exists(os.path.join(base, "wpcap.dll")) and os.path.exists(
            os.path.join(base, "Packet.dll")
        ):
            return True
    return False


def explain_npcap_requirement() -> str:
    return (
        "本程序需要 Npcap 才能进行 802.1X/EAPOL 网络认证。\n\n"
        "安装方式：\n"
        "  1. 点击「下载并安装」，程序会自动下载安装程序\n"
        "  2. 以管理员权限启动安装向导\n"
        "  3. 安装程序保持默认选项，一路点击 Next/下一步即可\n"
        "  4. 程序会在安装向导启动后自动检测安装结果\n\n"
        "三种默认选项说明（保持默认即可）：\n"
        "  - Restrict to Administrators only：保持默认关闭\n"
        "  - Support raw 802.11：保持默认关闭\n"
        "  - WinPcap API-compatible Mode：保持默认开启\n"
        f"\n官网下载：{NPCAP_DOWNLOAD_URL}"
    )


def _check_file_integrity(path: str) -> tuple[bool, str]:
    """快速检查下载文件是否完整（仅大小校验，不自带签名验证）。"""
    if not os.path.exists(path):
        return False, "文件不存在"
    size = os.path.getsize(path)
    if size <= 1_000_000:
        return False, f"文件过小（{size:,} bytes），下载可能不完整"
    return True, f"文件大小 {size:,} bytes，通过完整性检查"


def download_npcap_installer(progress_cb=None) -> tuple[bool, str]:
    """下载 Npcap 安装程序到临时目录。返回 (成功, 路径或错误消息)。"""
    dest = os.path.join(
        os.environ.get("TEMP", os.path.expanduser("~")),
        f"npcap-{NPCAP_VERSION}.exe",
    )

    if os.path.exists(dest):
        ok, msg = _check_file_integrity(dest)
        if ok:
            if progress_cb:
                progress_cb("安装程序已就绪")
            return True, dest
        if progress_cb:
            progress_cb(f"{msg}，重新下载...")
        try:
            os.remove(dest)
        except OSError:
            pass

    if progress_cb:
        progress_cb(f"正在下载 Npcap {NPCAP_VERSION}（约 1.3 MB）...")

    try:
        urllib.request.urlretrieve(NPCAP_INSTALLER_URL, dest)
        ok, msg = _check_file_integrity(dest)
        if not ok:
            try:
                os.remove(dest)
            except OSError:
                pass
            return False, msg
        if progress_cb:
            progress_cb("下载完成")
        return True, dest
    except Exception as exc:
        return False, f"下载失败：{exc}"


def launch_npcap_installer(installer_path: str) -> tuple[bool, str]:
    """以管理员权限启动 Npcap GUI 安装程序。返回 (成功, 消息)。"""
    if not os.path.exists(installer_path):
        return False, "安装程序不存在，请先下载"

    # ── 方案 A: ShellExecuteW "runas"（标准提权方式） ──
    try:
        import ctypes
        from ctypes import wintypes

        ctypes.windll.shell32.ShellExecuteW.argtypes = [
            wintypes.HWND,
            wintypes.LPCWSTR,
            wintypes.LPCWSTR,
            wintypes.LPCWSTR,
            wintypes.LPCWSTR,
            ctypes.c_int,
        ]
        ctypes.windll.shell32.ShellExecuteW.restype = ctypes.c_void_p

        SW_SHOW = 5
        ret = ctypes.windll.shell32.ShellExecuteW(
            None, "runas", installer_path, "", None, SW_SHOW
        )
        ret_int = ret & 0xFFFFFFFF
        if ret_int > 32:
            return True, f"Npcap 安装向导已启动（ShellExecuteW 返回 {ret_int}）"
        err_msg = f"ShellExecuteW 启动失败（错误码 {ret_int}）"
        if ret_int == 5:
            err_msg += " 权限不足，请以管理员身份运行本程序。"
        return False, err_msg
    except Exception as exc:
        return False, f"安装向导启动失败：{exc}"
