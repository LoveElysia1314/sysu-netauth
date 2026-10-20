# Native architecture

The Cargo workspace builds three programs around the `netauth` library. Linux ships the CLI and daemon; Windows also ships a native Win32 configuration/tray process. Authentication belongs exclusively to the daemon while it is running. Direct CLI packet operations acquire the same instance lock.

## Event ownership

`Engine` owns configuration, authentication state, generation counters, adapter candidates, retry deadlines, and subscriptions. Worker messages enter a bounded channel. It blocks on that channel until the next real deadline; there is no recurring service tick.

Each selected adapter has one capture session, retained across initial authentication, Internet verification, and renewal. Capture waits are interruptible by an OS cancellation event. Linux uses an EtherType-bound AF_PACKET socket with PAE multicast membership; Windows dynamically loads Npcap from the Windows system directory, installs an EAPOL capture filter, and waits on `pcap_getevent`. No promiscuous mode is enabled.

The capture thread does not mutate engine state. It drops excess packets when the engine queue is full. The engine validates Ethernet destination, excludes its own transmissions, pins the authenticator MAC, checks nested lengths, and handles Identity/MD5 requests. Credentials use GBK. Initial responses target the requesting switch; renewal responses preserve the prior PAE-group destination behavior.

EAP-Success starts a cancellable reachability worker, while capture remains active. Internet access is required before publishing `authenticated`. Verification uses the existing system-route ICMP targets and timing. Switching adapters, logoff, configuration changes, and shutdown invalidate old generations and cancel outstanding work. Normal retry limits and startup Fibonacci delays match the prior client. Physical link recovery clears manual disconnect, matching prior behavior.

OS network notifications replace adapter scans. Linux consumes rtnetlink link/address/route events; Windows uses `NotifyIpInterfaceChange`. Metadata is re-enumerated on changes or explicit requests.

## Local control protocol

Windows endpoint: `\\.\pipe\SYSUNetAuth-v1`, local clients only, SYSTEM/Administrators ACL. Linux endpoint: `/run/sysu-netauth/control.sock`, directory 0700/socket 0600 with root peer verification. Windows clients verify that the pipe server is elevated before sending credentials.

Each frame is a four-byte big-endian byte length followed by UTF-8 JSON, limited to 65,536 bytes. Protocol version 1 requests have `version`, `id`, and `action`; responses have `version`, `id`, `ok`, optional `error`, and `data`.

Actions: `status`, `watch`, `interfaces`, `authenticate`, `logoff`, `get_config`, `set_config` (with `config`), `reload_config`, and `check_update`. Connections serve one request. `watch` returns a normal response containing the initial snapshot, followed by framed snapshot objects on changes. Reconnecting always receives fresh state. There is no offline durable command queue.

Up to 16 client connections are accepted; subscriber queues contain at most eight snapshots, and slow subscribers are disconnected. Request reads and writes have timeouts. A client connection failure has no effect on the EAPOL session. The IPC reader can explicitly request credentials for the administrative settings UI; normal CLI config output redacts the password.

## Persistent storage and migration

The service alone writes configuration and adapter/update caches, using unique temporary files, durable writes, and atomic replacement. Reads do not rewrite native configuration. A missing configuration starts with defaults; an unreadable configuration leaves the service available for administrative repair.

Legacy Windows configuration is copied to `config.json.python-backup` before one-time migration. Field types/defaults are normalized, successful adapter hints move to the service cache, and `native_schema: 1` marks the new file. Keep the backup protected as it contains credentials. Native files remain JSON. Status and IPC commands are no longer files.

Windows directories restrict credentials to SYSTEM and Administrators. Linux directories are 0700 and files 0600. Windows login-task creation runs in the administrative GUI/CLI user context rather than the Session 0 service. Windows service startup type follows `service_mode`; GUI exit stops it when independent mode is disabled.

Update checks run independently of wired authentication, using any available route. Gitee precedes GitHub; HTTP redirects are rejected, HTTPS hosts are restricted, response sizes are bounded, and release URLs are validated. Initial checks are delayed, success schedules daily checks, and failure backs off. Only one temporary download thread exists per check. Windows uses WinHTTP, Linux uses blocking ureq/rustls.

## Build and release acceptance

Release builds use LTO, one codegen unit, stripped symbols, and static MSVC CRT linkage. Python, Qt, Scapy, and PyInstaller are absent. MSVC builds embed Windows resources with the SDK resource compiler. Windows packaging uses the established Inno Setup application ID and service name; Npcap is downloaded separately and Authenticode-verified before interactive installation. Linux packaging installs a hardened systemd unit with only CAP_NET_RAW.

`scripts/build-windows.sh` cross-compiles the same `x86_64-pc-windows-msvc` release from Linux using cargo-xwin. The resource source is header-free; native Windows uses rc.exe and Linux uses llvm-rc (`NETAUTH_RC` overrides either). Both use the packaging directory as the working directory so embedded resource paths are independent of the caller's directory. The SDK/CRT download cache defaults to `target/xwin`. Cross-building adds no application dependencies and does not change CPU tuning or event-loop behavior.

Both Windows build paths run the host-side `verify_windows` example against their output. It checks actual PE machine/subsystem headers, rejects dynamic compiler/CRT DLL imports and unconditional Npcap imports, and requires the GUI's elevation manifest and six icon groups. The Linux release ZIP contains only the three EXEs, with no compiler, SDK, Wine, or runtime DLLs. CI builds this ZIP on Ubuntu in addition to the native Windows build.

Shared tests exercise frames/encoding, malformed inputs, IPC framing, migration, candidate order, retries, stale results, and subscriber backpressure. The namespace integration test brings up isolated veth adapters, runs a simulated authenticator and service, verifies the MD5 response, answers renewal during Internet verification, checks repeated online renewals, rejects direct/service contention, exercises CLI IPC, and terminates through SIGTERM.

Before release on Windows, verify fresh install and Python upgrade, Npcap absence/install, SCM boot before user login, GUI reopen/reconnect, login task account/session, hidden startup, notification cooldown, independent-service preferences, uninstall/config retention, and real adapter hotplug/failover. Observe repeated EAP exchanges on a campus switch. Cross-compilation checks do not substitute for those runtime checks.

Measure startup, working set/private bytes, CPU and disk writes on the same hardware against the old Python release. The Linux example samples RSS, CPU time, context switches and writes; the Windows script samples CPU, working set/private bytes and thread count. No idle heartbeat/config scans/status writes or capture reopening are permitted. Scheduled update activity and network changes are legitimate wakeups. Target combined memory reduction is 50%, and steady CPU below 0.1% of one core; report measurements rather than assuming these targets were met.
