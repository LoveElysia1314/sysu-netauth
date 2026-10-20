# Native performance measurements

Measured locally on 2026-10-20 using the optimized Linux x64 build (Rust 1.97 nightly, Linux 6.12.111). Each measurement ran for 30 seconds after initialization, inside a private user/network namespace. Neither run installed a service or altered host networking.

| Scenario | Resident memory | Process CPU / one core | Disk writes during sample | Main-thread voluntary context switches |
| --- | ---: | ---: | ---: | ---: |
| Idle, automatic authentication disabled | 2,879,488 bytes (2.75 MiB) | 0.0% | 0 bytes | 0 |
| Authenticated, persistent capture, one watcher | 3,080,192 bytes (2.94 MiB) | 0.0% | 0 bytes | 0 |

The authenticated run used the actual service executable and an isolated veth authenticator. The test verified GBK/EAP-MD5, Internet verification through an isolated ICMP responder, EAP replies during verification, three renewal handshakes, and 32 watcher reconnects before sampling. It also verified direct/service exclusion, CLI status output, and graceful shutdown afterward.

Zero CPU means no additional CPU clock ticks were observed at the sampler's resolution; it does not mean every operation has zero cost. Context-switch counts refer to the main thread. Startup writes, authentication traffic and periodic update checks fall outside these stable sampling windows.

Reproduce with iproute2 installed and user/network namespaces enabled:

```sh
sh apps/native/scripts/benchmark-linux.sh 600
sh apps/native/scripts/benchmark-linux-authenticated.sh 600
```

Use `apps/native/scripts/measure-windows.ps1` separately for the Windows service and GUI. Windows memory/startup measurements, the same-machine Python comparison, and real campus-network acceptance remain outstanding. These Linux results do not establish the planned 50% Windows memory reduction.

The Linux-to-Windows MSVC release build has also been validated. CLI, service and GUI executable sizes were 1,141,760, 764,928 and 987,136 bytes respectively (file sizes, not resident memory). PE inspection confirmed static CRT linkage, Windows OS DLL imports only, correct executable subsystems, and the GUI's embedded elevation manifest and six icon groups. The Windows CLI's `--help` ran successfully under Wine without Npcap. This is a startup smoke test, not a native Windows GUI/service or performance test. Cross-builds use the same optimization profile as native Windows builds.
