"""Child process used by the Windows installer regression check."""
import sys
import time

if sys.argv[1] == "hang":
    time.sleep(60)
elif sys.argv[1] == "stdin":
    if sys.stdin is not None and sys.stdin.read(1):
        raise SystemExit(3)
elif sys.argv[1] == "exit":
    raise SystemExit(7)
else:
    raise SystemExit(4)
