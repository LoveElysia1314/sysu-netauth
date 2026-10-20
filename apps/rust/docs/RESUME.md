# Windows sleep and hibernate recovery

Automatic recovery requires a running Rust service, saved credentials, `auto_auth: true` and `service_mode: true`. Manual disconnect suppresses automatic reconnection.

## Cause and fix

The Python service does not process Windows power events. Its adapter check skips authentication when the link state is unchanged. A link can appear up both before hibernation and after wake while the switch session has expired or the capture handle needs reopening.

The Rust service accepts SCM suspend and `PBT_APMRESUMEAUTOMATIC` events. It clears stale state, cancels capture and verification, then reopens capture and authenticates after a two-second delay. Temporarily unavailable adapters are retried during a 120-second recovery window. Recovery also works when the suspend event was missed.

This applies to the Windows service; foreground `--console` mode does not receive SCM power controls. Python has not received this fix.

## Troubleshooting

After authentication, fully exit the GUI, hibernate, then wake the machine. Check from an administrator terminal:

```powershell
sc.exe query SYSUNetAuth
.\sysu_netauth.exe status --json
Get-Content "$env:ProgramData\SYSUNetAuth\service.log" -Tail 40
```

Expect a fresh authentication attempt and `authenticated; Internet reachable`. If the service is stopped, check `service_mode` and startup logs. If it is running but authentication fails, check the log for Npcap, adapter selection and reachability errors.

Before release, verify sleep, hibernate, unattended wake, delayed adapter readiness and repeated cycles on Windows hardware. Manual disconnect and disabled automatic authentication must remain disconnected.
