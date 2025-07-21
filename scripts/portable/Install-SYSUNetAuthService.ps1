$ErrorActionPreference = "Stop"

$serviceName = "SYSUNetAuth"
$displayName = "SYSU NetAuth"
$scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
$serviceExe = Join-Path $scriptDir "sysu_netauth_service.exe"
$programDataDir = Join-Path $env:ProgramData "SYSUNetAuth"

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

if (-not (Test-Path $serviceExe)) {
    throw "Missing service executable: $serviceExe"
}

New-Item -ItemType Directory -Force -Path $programDataDir | Out-Null
$acl = Get-Acl $programDataDir
$rule = [System.Security.AccessControl.FileSystemAccessRule]::new(
    "Users",
    "Modify",
    "ContainerInherit,ObjectInherit",
    "None",
    "Allow"
)
$acl.SetAccessRule($rule)
Set-Acl -Path $programDataDir -AclObject $acl

sc.exe stop $serviceName | Out-Null
Start-Sleep -Milliseconds 800
sc.exe delete $serviceName | Out-Null
Start-Sleep -Milliseconds 800

$binPath = '"' + $serviceExe + '"'
sc.exe create $serviceName binPath= $binPath start= auto depend= npcap DisplayName= $displayName | Write-Host
sc.exe description $serviceName "SYSU wired campus network 802.1X authentication service" | Out-Null
sc.exe failure $serviceName reset= 86400 actions= restart/60000/none/0/none/0 | Out-Null
sc.exe start $serviceName | Write-Host

Write-Host
sc.exe query $serviceName
Write-Host
Write-Host "Shared data: $programDataDir"
Write-Host "Open sysu_netauth.exe to configure NetID and view status."
