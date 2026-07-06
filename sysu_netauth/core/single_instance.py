"""
Single-instance manager via QLocalServer / QLocalSocket.

Architecture:
  - First instance: creates a QLocalServer listening on a named pipe.
  - Second instance: connects to the pipe, sends "activate", then exits.
  - First instance: upon receiving "activate", emits activate_requested signal
    so the main window can restore itself.

This is more reliable than Mutex (no stale locks) or EnumWindows (no HWND
hunting), because the active instance knows best how to restore its own window.
"""

from __future__ import annotations

import getpass

from PySide6.QtCore import QObject, QTimer, Signal
from PySide6.QtNetwork import QLocalServer, QLocalSocket

_PIPE_TIMEOUT_MS = 1000
_ACTIVATE_MSG = b"activate"


class SingleInstanceManager(QObject):
    """Manages single-instance enforcement via named pipe IPC.

    Connect to ``activate_requested`` to handle window restoration when a
    second instance is launched.
    """

    activate_requested = Signal()

    def __init__(self, app_id: str, parent: QObject | None = None) -> None:
        super().__init__(parent)
        user = getpass.getuser()
        self._server_name = f"{app_id}_{user}"
        self._server: QLocalServer | None = None

    # ── public API ────────────────────────────────────────────────

    def notify_existing(self, timeout_ms: int = _PIPE_TIMEOUT_MS) -> bool:
        """Try to connect to an existing instance and send an activate request.

        Returns True if an existing instance was found (caller should exit),
        False if no instance exists (caller should become the primary).
        """
        sock = QLocalSocket()
        sock.connectToServer(self._server_name)
        if not sock.waitForConnected(timeout_ms):
            sock.abort()
            sock.deleteLater()
            return False

        sock.write(_ACTIVATE_MSG)
        sock.flush()
        sock.waitForBytesWritten(timeout_ms)
        sock.disconnectFromServer()
        sock.deleteLater()
        return True

    def start_server(self) -> bool:
        """Become the primary instance by starting the named-pipe server.

        Returns True on success, False if the server name is already taken
        (a rare race — caller should retry ``notify_existing``).
        """
        self._server = QLocalServer(self)
        self._server.newConnection.connect(self._on_new_connection)

        if self._server.listen(self._server_name):
            return True

        # listen() failed → clean up any stale pipe and retry once
        QLocalServer.removeServer(self._server_name)
        if self._server.listen(self._server_name):
            return True

        return False

    # ── internal ──────────────────────────────────────────────────

    def _on_new_connection(self) -> None:
        while self._server and self._server.hasPendingConnections():
            sock = self._server.nextPendingConnection()
            sock.readyRead.connect(lambda s=sock: self._on_ready_read(s))
            sock.disconnected.connect(sock.deleteLater)
            # Data may already be buffered; schedule a read
            QTimer.singleShot(0, lambda s=sock: self._on_ready_read(s))

    def _on_ready_read(self, sock: QLocalSocket) -> None:
        data = bytes(sock.readAll()).strip()
        if data == _ACTIVATE_MSG:
            self.activate_requested.emit()
        sock.disconnectFromServer()
