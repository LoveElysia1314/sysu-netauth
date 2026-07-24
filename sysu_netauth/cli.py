from __future__ import annotations

import argparse
import getpass
import sys

from .core.eapol import AuthOptions, authenticate, send_logoff
from .core.interfaces import (
    list_auth_candidate_interfaces,
    list_candidates,
    probe_eapol,
)
from .core.npcap import explain_npcap_requirement, has_npcap


def main() -> None:
    parser = argparse.ArgumentParser(
        description="SYSU 802.1X/EAPOL authentication client."
    )
    parser.add_argument("-i", "--iface", default=None)
    parser.add_argument("-u", "--username", default=None)
    parser.add_argument(
        "-p", "--password", default=None, help="password; omit to prompt securely"
    )
    parser.add_argument("--timeout", type=int, default=30)
    parser.add_argument(
        "--logoff", action="store_true", help="send EAPOL-Logoff and exit"
    )
    parser.add_argument(
        "--check-npcap", action="store_true", help="check Npcap availability and exit"
    )
    parser.add_argument(
        "--list-ifaces",
        action="store_true",
        help="list candidate network interfaces and exit",
    )
    parser.add_argument(
        "--probe-iface",
        action="store_true",
        help="probe selected interface for EAPOL server and exit",
    )
    parser.add_argument(
        "--client-ip", default=None, help="override detected client IPv4"
    )
    args = parser.parse_args()

    try:
        if args.check_npcap:
            if has_npcap():
                print("Npcap available")
                return
            raise RuntimeError(explain_npcap_requirement())

        if args.list_ifaces:
            for item in list_candidates():
                print(
                    f"{item.score:>4}  {item.interface_type.value:<8}  "
                    f"{item.name}  mac={item.mac or '-'}  {item.reason}"
                )
            return

        if args.probe_iface:
            if not args.iface:
                raise RuntimeError("--iface is required for --probe-iface")
            result = probe_eapol(args.iface)
            print(f"{result.iface}: {result.status.value} - {result.message}")
            return

        if args.logoff:
            if not args.iface:
                raise RuntimeError("--iface is required for --logoff")
            mac = send_logoff(args.iface)
            print(f"iface={args.iface} mac={mac}")
            print("EAPOL logoff sent")
            return

        if not args.username:
            raise RuntimeError("--username is required for authentication")

        password = args.password or getpass.getpass("Password: ")
        if args.iface:
            candidate_ifaces = [args.iface]
        else:
            candidate_ifaces = [
                candidate.name for candidate in list_auth_candidate_interfaces()
            ]
            if not candidate_ifaces:
                raise RuntimeError(
                    "--iface is required when no Ethernet interface is detected"
                )
            print("Auto candidate interfaces: " + ", ".join(candidate_ifaces))

        last_error = ""
        for index, iface in enumerate(candidate_ifaces, start=1):
            if len(candidate_ifaces) > 1:
                print(f"Trying interface {index}/{len(candidate_ifaces)}: {iface}")
            result = authenticate(
                AuthOptions(
                    iface=iface,
                    username=args.username,
                    password=password,
                    timeout=args.timeout,
                    client_ip=args.client_ip,
                ),
                progress=lambda _status, message: print(message),
            )
            if result.status.value == "auth_success":
                return
            last_error = result.message
            if result.status.value == "no_npcap":
                break
            if len(candidate_ifaces) > 1:
                print(f"Interface {iface} failed: {result.message}", file=sys.stderr)

        raise RuntimeError(last_error or "authentication failed")
    except KeyboardInterrupt:
        print("Interrupted", file=sys.stderr)
        raise SystemExit(130) from None
    except Exception as exc:
        print(f"ERROR: {exc}", file=sys.stderr)
        raise SystemExit(1) from exc


if __name__ == "__main__":
    main()
