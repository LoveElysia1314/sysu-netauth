from __future__ import annotations

import time
import unittest
from unittest.mock import Mock, patch

from sysu_netauth.app.tray import CampusTray
from sysu_netauth.core.config import AppConfig, ServiceStatus


class TrayStatusTests(unittest.TestCase):
    def _tray(self) -> CampusTray:
        tray = CampusTray.__new__(CampusTray)
        tray.config = AppConfig()
        tray._last_service_state = None
        tray._last_stale_state = False
        tray.status_window = Mock()
        tray.set_state = Mock()
        tray._notify_state = Mock()
        tray._poll_update_state = Mock()
        return tray

    def _poll(self, tray: CampusTray, *statuses: ServiceStatus) -> None:
        with patch("sysu_netauth.app.tray.read_status", side_effect=statuses):
            for _ in statuses:
                tray._poll_service_status()

    def test_unreadable_status_does_not_repeat_success_notification(self) -> None:
        tray = self._tray()
        connected = ServiceStatus(state="authenticated", updated_at=time.time())
        # read_status 在读取失败时返回这个无时间戳的 stopped 状态。
        unavailable = ServiceStatus(state="stopped", message="服务状态不可用")
        self._poll(tray, connected, unavailable, connected, unavailable, connected)

        tray._notify_state.assert_not_called()
        self.assertEqual(tray._last_service_state, "authenticated")

    def test_initial_unreadable_status_does_not_count_as_disconnection(self) -> None:
        tray = self._tray()
        self._poll(
            tray,
            ServiceStatus(state="stopped"),
            ServiceStatus(state="authenticated", updated_at=time.time()),
        )

        tray._notify_state.assert_not_called()

    def test_real_service_restart_still_notifies_on_reconnection(self) -> None:
        tray = self._tray()
        connected = ServiceStatus(state="authenticated", updated_at=time.time())
        self._poll(
            tray,
            connected,
            ServiceStatus(state="stopped", updated_at=time.time()),
            connected,
        )

        tray._notify_state.assert_called_once_with("authenticated", "已认证")

    def test_status_freshness_uses_persistent_wall_clock(self) -> None:
        now = time.time()
        self.assertFalse(CampusTray._status_is_stale(now))
        self.assertTrue(CampusTray._status_is_stale(now - 16))
        self.assertTrue(CampusTray._status_is_stale(now + 16))
        self.assertTrue(CampusTray._status_is_stale(0))


if __name__ == "__main__":
    unittest.main()
