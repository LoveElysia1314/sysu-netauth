#!/usr/bin/env python3
"""Offline GUI fixture. Never opens sockets or changes network state."""
import json
import os
import sys

adapter = {
    "id": "00:01:02:03:04:05", "name": "en5 (模拟)", "ipv4": "192.0.2.10", "up": True,
    "ethernet": True,
    "driver": "USB 10/100/1000 LAN Controller", "gateway": "192.0.2.1", "dns": "192.0.2.53", "mac": [0, 1, 2, 3, 4, 5],
}
update = {"available": True, "latest_version": "99.0.0", "release_url": "https://github.com/LoveElysia1314/sysu-netauth/releases/tag/v99.0.0", "summary": "离线预览"}
if os.environ.get("NETAUTH_PREVIEW_UPDATE_STATE") == "latest":
    update.update(available=False, latest_version="0.7.0", checked_at=1)
action = sys.argv[-1]
if action == "interfaces":
    print(json.dumps([adapter]))
elif action in ("status", "authenticate", "logoff"):
    print(json.dumps({
        "state": "authenticated", "internet": True, "adapter": adapter, "adapters": [adapter], "suspended": False, "manual_disconnect": False,
        "message": "已认证，互联网连通",
        "update": update,
    }))
else:
    sys.exit("unsupported mock action")
