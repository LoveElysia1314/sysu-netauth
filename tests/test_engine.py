from __future__ import annotations

import unittest
from dataclasses import replace
from unittest.mock import patch

from sysu_netauth.core.config import AppConfig, ServiceCache
from sysu_netauth.core.eapol import AuthResult, AuthStatus
from sysu_netauth.service.engine import AuthServiceEngine


class ServiceConfigResilienceTests(unittest.TestCase):
    def _engine(self, config: AppConfig | None = None) -> AuthServiceEngine:
        with (
            patch(
                "sysu_netauth.service.engine.load_config",
                return_value=config or AppConfig(),
            ),
            patch(
                "sysu_netauth.service.engine.load_service_cache",
                return_value=ServiceCache(),
            ),
        ):
            return AuthServiceEngine()

    def test_startup_survives_temporarily_unreadable_config(self) -> None:
        with (
            patch(
                "sysu_netauth.service.engine.load_config",
                side_effect=PermissionError("temporarily locked"),
            ),
            patch(
                "sysu_netauth.service.engine.load_service_cache",
                return_value=ServiceCache(),
            ),
        ):
            engine = AuthServiceEngine()

        self.assertEqual(engine.config.username, "")
        self.assertTrue(engine._startup_config_pending)

    def test_reload_keeps_current_config_after_io_error(self) -> None:
        engine = self._engine()
        engine.config = replace(engine.config, username="keep-me")

        with patch(
            "sysu_netauth.service.engine.load_config",
            side_effect=PermissionError("temporarily locked"),
        ):
            engine.reload_config()

        self.assertEqual(engine.config.username, "keep-me")

    def test_reload_resumes_deferred_startup_authentication(self) -> None:
        engine = self._engine()
        engine._startup_config_pending = True
        engine._service_started = True
        recovered = AppConfig(username="user", password="secret")

        with (
            patch(
                "sysu_netauth.service.engine.load_config",
                return_value=recovered,
            ),
            patch("sysu_netauth.service.engine.has_npcap", return_value=True),
            patch.object(engine, "_schedule_startup_auth") as schedule,
        ):
            self.assertTrue(engine.reload_config())

        schedule.assert_called_once_with()
        self.assertFalse(engine._startup_config_pending)

    def test_service_cache_update_does_not_modify_user_config(self) -> None:
        engine = self._engine(AppConfig(username="user", password="secret"))
        before = engine.config

        with patch("sysu_netauth.service.engine.save_service_cache") as save:
            engine._save_cache_safely(
                iface="Ethernet",
                last_success_mac="00:11",
            )

        self.assertEqual(engine.config, before)
        self.assertEqual(engine.cache.iface, "Ethernet")
        save.assert_called_once_with(engine.cache)

    def test_disabling_auto_auth_cancels_pending_retry(self) -> None:
        engine = self._engine(
            AppConfig(username="user", password="secret", auto_auth=True)
        )
        engine._service_started = True
        engine._next_retry_at = 123.0

        with (
            patch(
                "sysu_netauth.service.engine.load_config",
                return_value=replace(engine.config, auto_auth=False),
            ),
            patch.object(engine, "_set_status") as set_status,
        ):
            engine.reload_config()

        self.assertEqual(engine._next_retry_at, 0.0)
        set_status.assert_called_once()

    def test_enabling_auto_auth_starts_when_not_manually_disconnected(self) -> None:
        engine = self._engine(
            AppConfig(username="user", password="secret", auto_auth=False)
        )
        engine._service_started = True

        with (
            patch(
                "sysu_netauth.service.engine.load_config",
                return_value=replace(engine.config, auto_auth=True),
            ),
            patch.object(engine, "_start_auth_flow") as start,
        ):
            engine.reload_config()

        start.assert_called_once_with(manual=False, force=True)

    def test_stale_connectivity_result_is_ignored(self) -> None:
        engine = self._engine()
        engine._auth_generation = 2
        result = AuthResult(AuthStatus.AUTH_SUCCESS, "ok", "Ethernet")

        with patch.object(engine, "_set_status") as set_status:
            engine._on_connectivity_result(True, result, generation=1)

        set_status.assert_not_called()

    def test_scheduled_update_check_does_not_require_authentication(self) -> None:
        engine = self._engine()
        checker = unittest.mock.MagicMock()
        engine._update_checker = checker

        engine._maybe_check_for_update()
        checker.request_check.assert_called_once_with()

    def test_manual_update_command_does_not_require_authentication(self) -> None:
        engine = self._engine()
        checker = unittest.mock.MagicMock()
        engine._update_checker = checker

        with patch(
            "sysu_netauth.service.engine.read_command",
            return_value="check_update",
        ):
            engine._handle_command()

        checker.request_check.assert_called_once_with(force=True)

    def test_service_start_defers_first_update_check(self) -> None:
        engine = self._engine()
        engine.stop_event.set()
        checker = unittest.mock.MagicMock()
        engine._update_checker = checker

        with (
            patch.object(engine, "_set_status"),
            patch.object(engine, "_schedule_startup_auth"),
            patch("sysu_netauth.service.engine.has_npcap", return_value=True),
        ):
            engine.run()

        checker.ensure_initial_delay.assert_called_once_with()


if __name__ == "__main__":
    unittest.main()
