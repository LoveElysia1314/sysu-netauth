#!/usr/bin/env python3
"""Offline GUI fixture. Never opens sockets or changes network state."""
import json
import sys

adapter = {
    "name": "en5 (模拟)", "ipv4": "192.0.2.10", "up": True,
    "ethernet": True,
}
action = sys.argv[-1]
if action == "interfaces":
    print(json.dumps([adapter]))
elif action in ("status", "authenticate", "logoff"):
    print(json.dumps({
        "state": "idle", "internet": False, "adapter": adapter,
        "message": "离线模拟：仅验证界面与调用流程，未进行校园网认证。",
    }))
else:
    sys.exit("unsupported mock action")
