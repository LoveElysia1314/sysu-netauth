"""Low-frequency update checker owned by the Windows service."""

from __future__ import annotations

import hashlib
import logging
import platform
import threading
import time
from dataclasses import replace

from sysu_netauth.core.config import (
    APP_DISPLAY_NAME,
    APP_VERSION,
    UpdateState,
    load_update_state,
    save_update_state,
)
from sysu_netauth.core.update import fetch_release_info, is_newer_version

SUCCESS_INTERVAL_SECONDS = 24 * 60 * 60
INITIAL_DELAY_SECONDS = 2 * 60
INITIAL_JITTER_SECONDS = 3 * 60
DAILY_JITTER_SECONDS = 30 * 60
FAILURE_INTERVALS = (30 * 60, 2 * 60 * 60, 6 * 60 * 60, 24 * 60 * 60)
MANUAL_COOLDOWN_SECONDS = 60
MAX_FUTURE_SCHEDULE_SECONDS = 7 * 24 * 60 * 60


def _stable_jitter(limit: int) -> int:
    if limit <= 0:
        return 0
    seed = f"{platform.node()}|{APP_DISPLAY_NAME}".encode("utf-8", errors="replace")
    value = int.from_bytes(hashlib.sha256(seed).digest()[:4], "big")
    return value % (limit + 1)


class UpdateChecker:
    def __init__(
        self,
        stop_event: threading.Event,
        logger: logging.Logger,
    ) -> None:
        self.stop_event = stop_event
        self.logger = logger
        self._lock = threading.Lock()
        self._thread: threading.Thread | None = None
        self._last_manual_request_at = 0.0

    def ensure_initial_delay(self, now: float | None = None) -> None:
        now = time.time() if now is None else now
        with self._lock:
            state = load_update_state()
            if state.checked_at > 0 or state.next_check_at > now:
                return
            save_update_state(
                replace(
                    state,
                    status="waiting",
                    current_version=APP_VERSION,
                    next_check_at=(
                        now
                        + INITIAL_DELAY_SECONDS
                        + _stable_jitter(INITIAL_JITTER_SECONDS)
                    ),
                    error="",
                )
            )

    def due(self, now: float | None = None) -> bool:
        now = time.time() if now is None else now
        state = load_update_state()
        return (
            state.next_check_at <= now
            or state.next_check_at > now + MAX_FUTURE_SCHEDULE_SECONDS
        )

    def request_check(self, *, force: bool = False) -> bool:
        with self._lock:
            if self._thread is not None and self._thread.is_alive():
                return False
            now = time.monotonic()
            if (
                force
                and self._last_manual_request_at
                and now - self._last_manual_request_at < MANUAL_COOLDOWN_SECONDS
            ):
                state = load_update_state()
                save_update_state(
                    replace(
                        state,
                        status="error",
                        checked_at=time.time(),
                        error="检查过于频繁，请稍后再试",
                    )
                )
                return False
            if force:
                self._last_manual_request_at = now
            if not force and not self.due():
                return False
            previous = load_update_state()
            save_update_state(
                replace(
                    previous,
                    status="checking",
                    current_version=APP_VERSION,
                    error="",
                )
            )
            self._thread = threading.Thread(
                target=self._run_check,
                args=(previous,),
                name="UpdateChecker",
                daemon=True,
            )
            self._thread.start()
            return True

    def _run_check(self, previous: UpdateState) -> None:
        now = time.time()
        try:
            release = fetch_release_info()
            available = is_newer_version(release.version, APP_VERSION)
            state = UpdateState(
                status="success",
                current_version=APP_VERSION,
                latest_version=release.version,
                available=available,
                release_url=release.release_url,
                summary=release.summary,
                source=release.source,
                checked_at=now,
                next_check_at=(
                    now
                    + SUCCESS_INTERVAL_SECONDS
                    + _stable_jitter(DAILY_JITTER_SECONDS)
                ),
                failure_count=0,
                error="",
            )
            save_update_state(state)
            if available and (
                not previous.available or previous.latest_version != release.version
            ):
                self._log_new_release(release.version)
        except Exception as exc:
            failure_count = previous.failure_count + 1
            interval = FAILURE_INTERVALS[
                min(failure_count - 1, len(FAILURE_INTERVALS) - 1)
            ]
            save_update_state(
                replace(
                    previous,
                    status="error",
                    current_version=APP_VERSION,
                    checked_at=now,
                    next_check_at=now + interval,
                    failure_count=failure_count,
                    error=str(exc)[:500],
                )
            )
            self.logger.warning("update check failed: %s", exc)

    def _log_new_release(self, version: str) -> None:
        message = f"{APP_DISPLAY_NAME} v{version} 可用"
        self.logger.info(message)
        try:
            import servicemanager

            servicemanager.LogInfoMsg(message)
        except Exception:
            pass

    def shutdown(self, timeout: float = 2.0) -> None:
        thread = self._thread
        if thread is not None and thread.is_alive():
            thread.join(timeout=timeout)
