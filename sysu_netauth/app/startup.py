from __future__ import annotations

import os
import sys
from pathlib import Path

from sysu_netauth.core.config import APP_DISPLAY_NAME, APP_ID

STARTUP_TASK_NAME = f"{APP_ID} GUI"


def legacy_startup_shortcut_path() -> Path:
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
    """创建或删除以最高权限运行的当前用户登录任务。"""
    # 迁移旧版 Startup 快捷方式；requireAdministrator 程序不应从这里启动。
    legacy_startup_shortcut_path().unlink(missing_ok=True)
    target = Path(sys.executable)
    try:
        import win32com.client  # type: ignore[import-untyped]

        scheduler = win32com.client.Dispatch("Schedule.Service")
        scheduler.Connect()
        root = scheduler.GetFolder("\\")
        if not enabled:
            try:
                root.DeleteTask(STARTUP_TASK_NAME, 0)
            except Exception:
                # 任务不存在时删除是幂等操作。
                pass
            return

        task = scheduler.NewTask(0)
        task.RegistrationInfo.Description = f"{APP_DISPLAY_NAME} 登录启动"
        task.Settings.Enabled = True
        task.Settings.StartWhenAvailable = True
        task.Settings.DisallowStartIfOnBatteries = False
        task.Settings.StopIfGoingOnBatteries = False
        task.Settings.ExecutionTimeLimit = "PT0S"
        task.Settings.MultipleInstances = 2  # TASK_INSTANCES_IGNORE_NEW
        task.Principal.LogonType = 3  # TASK_LOGON_INTERACTIVE_TOKEN
        task.Principal.RunLevel = 1  # TASK_RUNLEVEL_HIGHEST

        task.Triggers.Create(9)  # TASK_TRIGGER_LOGON
        action = task.Actions.Create(0)  # TASK_ACTION_EXEC
        action.Path = str(target)
        action.Arguments = "--startup"
        action.WorkingDirectory = str(target.parent)

        root.RegisterTaskDefinition(
            STARTUP_TASK_NAME,
            task,
            6,  # TASK_CREATE_OR_UPDATE
            None,
            None,
            3,  # TASK_LOGON_INTERACTIVE_TOKEN
        )
    except Exception as exc:
        action = "创建" if enabled else "删除"
        raise RuntimeError(f"无法{action}登录启动任务：{exc}") from exc
