from __future__ import annotations

import sys

from .win_service import debug_run, handle_command_line


def main() -> None:
    if len(sys.argv) > 1 and sys.argv[1] == "debug":
        debug_run()
        return
    handle_command_line(sys.argv)


if __name__ == "__main__":
    main()
