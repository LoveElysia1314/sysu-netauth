from __future__ import annotations

import logging
import threading
import unittest
from unittest.mock import MagicMock, patch

from sysu_netauth.core.config import UpdateState
from sysu_netauth.core.update import (
    ReleaseInfo,
    UPDATE_MANIFEST_SOURCES,
    UpdateManifestError,
    fetch_release_info,
    is_newer_version,
    parse_release_manifest,
    parse_version,
)
from sysu_netauth.service.update_checker import UpdateChecker


class UpdateManifestTests(unittest.TestCase):
    def test_semantic_versions_are_compared_numerically(self) -> None:
        self.assertEqual(parse_version("v1.2.3"), (1, 2, 3))
        self.assertTrue(is_newer_version("0.10.0", "0.9.9"))
        self.assertFalse(is_newer_version("0.6.2", "0.6.2"))

    def test_manifest_rejects_untrusted_release_host(self) -> None:
        payload = """
        {
          "schema_version": 1,
          "version": "0.7.0",
          "release_url": "https://example.com/update.exe",
          "channel": "stable"
        }
        """
        with self.assertRaises(UpdateManifestError):
            parse_release_manifest(payload)

    def test_manifest_accepts_project_release_page(self) -> None:
        payload = """
        {
          "schema_version": 1,
          "version": "v0.7.0",
          "published_at": "2026-07-24T00:00:00+08:00",
          "release_url": "https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0",
          "summary": "test",
          "channel": "stable"
        }
        """
        result = parse_release_manifest(payload)
        self.assertEqual(result.version, "0.7.0")

    def test_fetch_rejects_redirect_to_untrusted_host(self) -> None:
        response = MagicMock()
        response.__enter__.return_value = response
        response.geturl.return_value = "https://example.com/release.json"
        response.read.return_value = b"{}"
        with self.assertRaises(UpdateManifestError):
            fetch_release_info(opener=MagicMock(return_value=response))

    def test_gitee_is_the_first_update_source(self) -> None:
        self.assertEqual(UPDATE_MANIFEST_SOURCES[0].name, "gitee")
        self.assertEqual(UPDATE_MANIFEST_SOURCES[1].name, "github")

    def test_fetch_falls_back_from_gitee_to_github(self) -> None:
        payload = b"""
        {
          "schema_version": 1,
          "version": "0.7.0",
          "release_url": "https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v0.7.0"
        }
        """
        response = MagicMock()
        response.__enter__.return_value = response
        response.geturl.return_value = UPDATE_MANIFEST_SOURCES[1].url
        response.read.return_value = payload
        opener = MagicMock(side_effect=[OSError("gitee unavailable"), response])

        result = fetch_release_info(opener=opener)

        self.assertEqual(result.source, "github")
        requested_urls = [call.args[0].full_url for call in opener.call_args_list]
        self.assertEqual(
            requested_urls,
            [source.url for source in UPDATE_MANIFEST_SOURCES],
        )


class UpdateCheckerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.checker = UpdateChecker(threading.Event(), logging.getLogger("test"))

    def test_initial_check_is_deferred(self) -> None:
        saved: list[UpdateState] = []
        with (
            patch(
                "sysu_netauth.service.update_checker.load_update_state",
                return_value=UpdateState(),
            ),
            patch(
                "sysu_netauth.service.update_checker.save_update_state",
                side_effect=saved.append,
            ),
            patch(
                "sysu_netauth.service.update_checker._stable_jitter",
                return_value=0,
            ),
        ):
            self.checker.ensure_initial_delay(now=1000)

        self.assertEqual(saved[0].status, "waiting")
        self.assertEqual(saved[0].next_check_at, 1120)

    def test_success_preserves_current_version_and_marks_newer_release(self) -> None:
        previous = UpdateState(status="checking")
        saved: list[UpdateState] = []
        release = ReleaseInfo(
            version="0.7.0",
            published_at="",
            release_url=(
                "https://github.com/LoveElysia1314/" "sysu-netauth/releases/tag/v0.7.0"
            ),
            summary="test",
        )
        with (
            patch(
                "sysu_netauth.service.update_checker.fetch_release_info",
                return_value=release,
            ),
            patch(
                "sysu_netauth.service.update_checker.save_update_state",
                side_effect=saved.append,
            ),
            patch.object(self.checker, "_log_new_release") as event_log,
            patch(
                "sysu_netauth.service.update_checker._stable_jitter",
                return_value=0,
            ),
        ):
            self.checker._run_check(previous)

        self.assertTrue(saved[0].available)
        self.assertEqual(saved[0].current_version, "0.6.2")
        event_log.assert_called_once_with("0.7.0")

    def test_failure_uses_backoff_and_keeps_previous_release(self) -> None:
        previous = UpdateState(
            latest_version="0.7.0",
            available=True,
            release_url="https://github.com/example",
        )
        saved: list[UpdateState] = []
        with (
            patch(
                "sysu_netauth.service.update_checker.fetch_release_info",
                side_effect=OSError("offline"),
            ),
            patch(
                "sysu_netauth.service.update_checker.save_update_state",
                side_effect=saved.append,
            ),
            patch(
                "sysu_netauth.service.update_checker.time.time",
                return_value=1000,
            ),
        ):
            self.checker._run_check(previous)

        self.assertEqual(saved[0].status, "error")
        self.assertTrue(saved[0].available)
        self.assertEqual(saved[0].next_check_at, 2800)

    def test_manual_requests_have_a_cooldown(self) -> None:
        state = UpdateState(status="success")
        saved: list[UpdateState] = []
        finished_thread = MagicMock()
        finished_thread.is_alive.return_value = False
        self.checker._thread = finished_thread
        self.checker._last_manual_request_at = 100
        with (
            patch(
                "sysu_netauth.service.update_checker.load_update_state",
                return_value=state,
            ),
            patch(
                "sysu_netauth.service.update_checker.save_update_state",
                side_effect=saved.append,
            ),
            patch(
                "sysu_netauth.service.update_checker.time.monotonic",
                return_value=120,
            ),
        ):
            started = self.checker.request_check(force=True)

        self.assertFalse(started)
        self.assertEqual(saved[0].status, "error")
        self.assertIn("频繁", saved[0].error)


if __name__ == "__main__":
    unittest.main()
