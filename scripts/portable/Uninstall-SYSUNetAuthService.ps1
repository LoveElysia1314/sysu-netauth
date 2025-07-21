$ErrorActionPreference = "Stop"

$serviceName = "SYSUNetAuth"

function Test-Admin {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

if (-not (Test-Admin)) {
    Start-Process powershell.exe -Verb RunAs -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy", "Bypass",
        "-File", "`"$PSCommandPath`""
    )
    exit
}

sc.exe stop $serviceName | Out-Null
Start-Sleep -Milliseconds 800
sc.exe delete $serviceName | Write-Host

Write-Host
Write-Host "Service removed. Shared data remains at:"
Write-Host "  $env:ProgramData\SYSUNetAuth"
Write-Host "Delete that folder manually only if you want to remove saved NetID/password."
