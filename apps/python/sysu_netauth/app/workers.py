"""Background worker thread for Npcap download.

Previously contained AuthWorker, LogoffWorker, and RenewListener;
those responsibilities have been migrated to service/engine.py.
"""

from __future__ import annotations

from PySide6.QtCore import QThread, Signal

from sysu_netauth.core.npcap import download_npcap_installer


class NpcapDownloadWorker(QThread):
    """Download Npcap installer in a background thread."""

    progress = Signal(str)
    finished = Signal(bool, str)

    def run(self) -> None:
        result = download_npcap_installer(
            progress_cb=lambda msg: self.progress.emit(msg)
        )
        self.finished.emit(*result)
