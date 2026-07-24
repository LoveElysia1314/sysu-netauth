from __future__ import annotations

import tempfile
import unittest
from pathlib import Path
from unittest.mock import MagicMock, patch

from sysu_netauth.app.startup import STARTUP_TASK_NAME, set_gui_launch_on_login


class StartupTaskTests(unittest.TestCase):
    def test_enable_creates_highest_privilege_logon_task(self) -> None:
        scheduler = MagicMock()
        root = scheduler.GetFolder.return_value
        task = scheduler.NewTask.return_value
        legacy = Path(tempfile.gettempdir()) / "missing-sysu-netauth-startup.lnk"

        with (
            patch("win32com.client.Dispatch", return_value=scheduler),
            patch(
                "sysu_netauth.app.startup.legacy_startup_shortcut_path",
                return_value=legacy,
            ),
        ):
            set_gui_launch_on_login(True)

        scheduler.Connect.assert_called_once_with()
        task.Triggers.Create.assert_called_once_with(9)
        task.Actions.Create.assert_called_once_with(0)
        self.assertEqual(task.Principal.LogonType, 3)
        self.assertEqual(task.Principal.RunLevel, 1)
        args = root.RegisterTaskDefinition.call_args.args
        self.assertEqual(args[0], STARTUP_TASK_NAME)
        self.assertEqual(args[2], 6)
        self.assertEqual(args[5], 3)

    def test_disable_deletes_task_idempotently(self) -> None:
        scheduler = MagicMock()
        root = scheduler.GetFolder.return_value
        legacy = Path(tempfile.gettempdir()) / "missing-sysu-netauth-startup.lnk"

        with (
            patch("win32com.client.Dispatch", return_value=scheduler),
            patch(
                "sysu_netauth.app.startup.legacy_startup_shortcut_path",
                return_value=legacy,
            ),
        ):
            set_gui_launch_on_login(False)

        root.DeleteTask.assert_called_once_with(STARTUP_TASK_NAME, 0)
        scheduler.NewTask.assert_not_called()


if __name__ == "__main__":
    unittest.main()
