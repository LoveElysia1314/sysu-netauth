from __future__ import annotations

import time
import unittest

from sysu_netauth.app.tray import CampusTray


class TrayStatusTests(unittest.TestCase):
    def test_status_freshness_uses_persistent_wall_clock(self) -> None:
        now = time.time()
        self.assertFalse(CampusTray._status_is_stale(now))
        self.assertTrue(CampusTray._status_is_stale(now - 16))
        self.assertTrue(CampusTray._status_is_stale(now + 16))
        self.assertTrue(CampusTray._status_is_stale(0))


if __name__ == "__main__":
    unittest.main()
