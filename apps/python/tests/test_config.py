from __future__ import annotations

import json
import os
import tempfile
import unittest
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path
from unittest.mock import patch

from sysu_netauth.core.config import (
    AppConfig,
    ServiceCache,
    ServiceStatus,
    UpdateState,
    UpdateUiState,
    load_config,
    load_service_cache,
    load_update_state,
    load_update_ui_state,
    read_command,
    read_status,
    save_config,
    save_service_cache,
    save_update_state,
    save_update_ui_state,
    write_command,
    write_status,
)


class ConfigStoreTests(unittest.TestCase):
    def test_load_config_does_not_rewrite_file(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            original = '{"username": "test", "unknown": true}\n'
            path.write_text(original, encoding="utf-8")

            config = load_config(path)

            self.assertEqual(config.username, "test")
            self.assertEqual(path.read_text(encoding="utf-8"), original)

    def test_non_utf8_config_is_backed_up_and_defaults_are_returned(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            path.write_bytes(b"\xff\xfeinvalid")

            config = load_config(path)

            self.assertEqual(config, AppConfig())
            self.assertFalse(path.exists())
            self.assertEqual(
                len(list(Path(directory).glob("config.json.invalid-*"))),
                1,
            )

    def test_concurrent_saves_leave_valid_json_without_temp_files(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"

            def write(index: int) -> None:
                save_config(AppConfig(username=f"user-{index}"), path)

            with ThreadPoolExecutor(max_workers=8) as executor:
                list(executor.map(write, range(40)))

            data = json.loads(path.read_text(encoding="utf-8"))
            self.assertTrue(data["username"].startswith("user-"))
            self.assertEqual(list(Path(directory).glob("*.tmp")), [])

    def test_save_retries_transient_permission_error(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "config.json"
            real_replace = os.replace
            attempts = 0

            def flaky_replace(
                source: str | os.PathLike[str],
                target: str | os.PathLike[str],
            ) -> None:
                nonlocal attempts
                attempts += 1
                if attempts < 3:
                    raise PermissionError("temporarily locked")
                real_replace(source, target)

            with patch(
                "sysu_netauth.core.config.os.replace", side_effect=flaky_replace
            ):
                save_config(AppConfig(username="test"), path)

            self.assertEqual(attempts, 3)
            self.assertEqual(
                json.loads(path.read_text(encoding="utf-8"))["username"],
                "test",
            )

    def test_status_write_skips_fsync_but_config_write_is_durable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with patch("sysu_netauth.core.config.os.fsync") as fsync:
                save_config(AppConfig(username="test"), root / "config.json")
                fsync.assert_called_once()
                fsync.reset_mock()
                with patch(
                    "sysu_netauth.core.config.STATUS_PATH",
                    root / "status.json",
                ):
                    write_status(ServiceStatus(state="idle"))
                fsync.assert_not_called()

    def test_invalid_status_timestamp_is_treated_as_stale(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "status.json"
            path.write_text(
                '{"state": "authenticated", "updated_at": "invalid"}',
                encoding="utf-8",
            )
            with patch("sysu_netauth.core.config.STATUS_PATH", path):
                status = read_status()

            self.assertEqual(status.state, "authenticated")
            self.assertEqual(status.updated_at, 0.0)

    def test_service_cache_is_stored_separately_from_user_config(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            config_path = root / "config.json"
            cache_path = root / "service_cache.json"
            save_config(AppConfig(username="user", password="secret"), config_path)
            with patch("sysu_netauth.core.config.SERVICE_CACHE_PATH", cache_path):
                save_service_cache(
                    ServiceCache(iface="Ethernet", last_success_mac="00:11")
                )
                cache = load_service_cache()

            self.assertEqual(cache.iface, "Ethernet")
            cache_data = json.loads(cache_path.read_text(encoding="utf-8"))
            self.assertEqual(cache_data["last_success_mac"], "00:11")
            user_data = json.loads(config_path.read_text(encoding="utf-8"))
            self.assertEqual(user_data["username"], "user")
            self.assertNotIn("last_success_mac", user_data)

    def test_command_queue_preserves_rapid_actions(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with (
                patch("sysu_netauth.core.config.COMMAND_PATH", root / "command.json"),
                patch("sysu_netauth.core.config.COMMAND_DIR", root / "commands"),
                patch(
                    "sysu_netauth.core.config.time.time_ns",
                    side_effect=[1, 2, 3],
                ),
            ):
                write_command("authenticate")
                write_command("logoff")
                write_command("reload_config")
                self.assertEqual(read_command(), "authenticate")
                self.assertEqual(read_command(), "logoff")
                self.assertEqual(read_command(), "reload_config")
                self.assertIsNone(read_command())

    def test_update_service_and_ui_state_have_separate_writers(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            service_path = root / "update_state.json"
            ui_path = root / "ui_state.json"
            save_update_state(
                UpdateState(
                    status="success",
                    latest_version="0.7.0",
                    available=True,
                    source="gitee",
                ),
                service_path,
            )
            save_update_ui_state(
                UpdateUiState(notified_version="0.7.0"),
                ui_path,
            )

            self.assertTrue(load_update_state(service_path).available)
            self.assertEqual(load_update_state(service_path).source, "gitee")
            self.assertEqual(
                load_update_ui_state(ui_path).notified_version,
                "0.7.0",
            )
            self.assertNotIn(
                "notified_version",
                json.loads(service_path.read_text(encoding="utf-8")),
            )


if __name__ == "__main__":
    unittest.main()
