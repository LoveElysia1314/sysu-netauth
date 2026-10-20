# Windows sleep and hibernate recovery

The service can recover without a GUI. Automatic recovery requires a running service, saved credentials, `auto_auth: true`, and no intentional manual disconnect. Use `service_mode: true` to keep the service running after exiting the GUI.

## Diagnosis of the Python implementation

Inspection of the original Python source at commit `bc1f8cb` found a concrete missed-resume path:

1. `sysu_netauth/service/win_service.py` handles service start/stop but does not accept or process Windows power notifications.
2. `AuthServiceEngine._check_iface_status` compares current media state with a cached previous value. When they match, it returns without reauthenticating.
3. The engine cannot observe intermediate link changes while the OS is suspended. The adapter can appear up immediately before hibernation and up again after wake, even though the switch's authenticated session has expired. A capture handle can also need reopening after a driver power transition.
4. With no observed down/up transition, the service can retain an authenticated state and old capture session instead of starting a fresh handshake. Reconnecting from the GUI forces that work, which explains why the GUI appears necessary.

This is an identified code defect consistent with the reported symptom. Without the affected machine's service status and logs, it does not establish whether its service stayed alive or stopped. Opening the GUI also starts a stopped service, so those two cases need distinguishing. The initial Rust implementation retained the missing power-event handling; the fix is in the current Rust service. The original Python app is preserved under `apps/python` and this fix has not been backported to it.

## Recovery behavior

The service now advertises `SERVICE_ACCEPT_POWEREVENT`. The SCM callback forwards suspend and automatic resume to the authentication engine without waiting for networking work. A separate atomic latch preserves the most recent power state if the bounded event queue is full.

Suspend cancels capture and reachability checks and clears online status. Resume independently invalidates the old generation, including when suspend was missed. After a two-second settling delay, the engine re-enumerates adapters, opens a fresh capture session, sends EAPOL-Start, completes authentication, and verifies Internet reachability. Address/route notifications during the settling delay cannot bypass it.

Resume starts a new 120-second recovery window. Temporary adapter absence and capture/setup errors use delays of 3, 5, 8, 13, then 21 seconds. Missing-adapter retries cease at the first check after the window expires and thereafter depend on OS network notifications. Authentication/capture failures continue with the configured normal retry interval, limited to five normal retries. Successful recovery removes these timers; there is no steady-state resume polling or extra resident thread.

The recovery event is `PBT_APMRESUMEAUTOMATIC`, which applies to unattended wakes as well. The later user-presence notification is ignored to avoid restarting an already recovered connection. See Microsoft's [resume event documentation](https://learn.microsoft.com/en-us/windows/win32/power/pbt-apmresumeautomatic) and [service handler contract](https://learn.microsoft.com/en-us/windows/win32/api/winsvc/nc-winsvc-lphandler_function_ex).

Manual disconnect and disabled auto-authentication prevent wake reconnection. Disconnecting or disabling auto-authentication during pending recovery cancels its retries. This Windows service change does not add Linux system-suspend notification support, and foreground Windows `--console` mode does not receive SCM power controls.

## Validation on Windows

Install the rebuilt service binary and restart the service once to load it. In an administrator terminal, with valid saved credentials and independent service mode enabled:

```powershell
sc.exe query SYSUNetAuth
.\sysu_netauth.exe status --json
Get-Content "$env:ProgramData\SYSUNetAuth\service.log" -Tail 40
```

After authenticating, fully exit the GUI and verify the service remains `RUNNING`. Hibernate from the Windows power menu, wake the computer, and check status again without opening the GUI or requesting authentication. The service log should show the sleep/resume state changes, a new authentication attempt and `authenticated; Internet reachable` after success. The status must lose its stale online flag during recovery.

Repeat with sleep, several consecutive cycles, delayed cable/USB adapter availability, and unattended wake if available. Repeat after intentional logoff and with `auto_auth: false`; neither should reconnect automatically. If SCM reports `STOPPED`, investigate service startup/failure logs and `service_mode` separately. If SCM is running but authentication fails, retain `service.log` and check Npcap, adapter selection and EAP/Internet verification errors.

Automated coverage includes full-queue delivery and state/retry tests, plus a private Linux network namespace exercising actual capture, EAPOL and Internet verification through the same engine. The integration scenario keeps the link up throughout and never opens a GUI or sends Authenticate. Windows cross-compilation and PE validation cover the service's Windows API bindings and release packaging. Real Windows SCM delivery, Npcap suspend behavior and campus-switch recovery still require this hardware check.
