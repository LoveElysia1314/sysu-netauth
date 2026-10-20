# Rust architecture

The `netauth` library serves three binaries: CLI, service and Windows GUI. The service owns authentication; direct CLI operations acquire the same adapter lock.

## Authentication engine

`Engine` owns configuration, authentication state, adapter selection and retry deadlines. It waits on a bounded event channel until an event or deadline. Each selected adapter retains one capture session through authentication, reachability verification and renewal.

- Linux: EtherType-bound AF_PACKET socket and rtnetlink notifications.
- Windows: dynamically loaded Npcap, EAPOL capture filter, `pcap_getevent` and `NotifyIpInterfaceChange`.
- Credentials: GBK; protocol: EAP-MD5. The engine validates frame lengths, destinations and authenticator MAC.
- Configuration changes, adapter changes and shutdown cancel stale work through generation counters.
- EAP-Success starts Internet verification while capture continues. ICMP uses the system route to `119.29.29.29` / `114.114.114.114`; success may use another interface.

Windows SCM power events reset capture and trigger authentication after wake. See [resume recovery](RESUME.md).

## Local control

| Platform | Endpoint | Access |
| --- | --- | --- |
| Windows | `\\.\pipe\SYSUNetAuth-v1` | SYSTEM / Administrators, local clients |
| Linux | `/run/sysu-netauth/control.sock` | root |

Frames contain a four-byte big-endian length followed by UTF-8 JSON, limited to 65,536 bytes. Requests contain `version: 1`, `id` and `action`; responses contain `version`, `id`, `ok`, optional `error` and `data`.

Actions: `status`, `watch`, `interfaces`, `authenticate`, `logoff`, `get_config`, `set_config`, `reload_config`, `check_update`. Each connection serves one request; `watch` then streams changed snapshots. Limits: 16 clients, eight queued snapshots per subscriber; slow subscribers disconnect.

## Storage

The service writes configuration and caches atomically. Windows data is restricted to SYSTEM/Administrators; Linux directories use 0700 and files 0600. Python configuration migration preserves `config.json.python-backup`.

## Network integration tests

Install util-linux and iproute2. Tests create isolated network namespaces and verify authentication, renewal, IPC and resume recovery.

```sh
cargo test --locked --test linux_network -- --ignored --nocapture
```

On x86-64 Linux hosts that restrict unprivileged user namespaces, run the test executable through sudo:

```sh
CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER='sudo -n --' cargo test --locked --test linux_network -- --ignored --nocapture
```

Windows SCM delivery, Npcap and campus-switch behavior require Windows hardware verification.
