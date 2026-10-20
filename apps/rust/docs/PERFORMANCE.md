# Rust performance measurements

Linux x64, Rust 1.97 nightly, Linux 6.12.111; measured on 2026-10-20. Each sample ran for 30 seconds after initialization in an isolated network namespace.

| Scenario | RSS | CPU / one core | Disk writes | Main-thread voluntary context switches |
| --- | ---: | ---: | ---: | ---: |
| Idle, automatic authentication disabled | 2.75 MiB | 0.0% | 0 bytes | 0 |
| Authenticated, persistent capture, one watcher | 2.94 MiB | 0.0% | 0 bytes | 0 |

The authenticated sample used the service and a simulated EAP-MD5 authenticator. CPU was below the sampler's clock-tick resolution. Startup, authentication and scheduled update activity were outside the sampling window. Windows measurements and a same-machine Python comparison remain pending.

Reproduce with iproute2 and user/network namespaces enabled:

```sh
sh apps/rust/scripts/benchmark-linux.sh 600
sh apps/rust/scripts/benchmark-linux-authenticated.sh 600
```

Windows sampling (run separately for service and GUI):

```powershell
./apps/rust/scripts/measure-windows.ps1 -ProcessName sysu_netauth_service -Seconds 600
./apps/rust/scripts/measure-windows.ps1 -ProcessName sysu_netauth_gui -Seconds 600
```
