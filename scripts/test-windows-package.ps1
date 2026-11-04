param(
    [Parameter(Mandatory=$true)][string]$Setup,
    [string]$OutputDirectory = ''
)
$ErrorActionPreference = 'Stop'
if (-not ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { throw 'Run native installer checks as administrator' }
if (Get-Service SYSUNetAuth -ErrorAction SilentlyContinue) { throw 'An authentication service is already installed; isolated checks will not replace it' }
$root = Split-Path $PSScriptRoot -Parent
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $root 'dist/windows-package-checks' }
[IO.Directory]::CreateDirectory($OutputDirectory) | Out-Null
$OutputDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$Setup = [IO.Path]::GetFullPath($Setup)
$install = Join-Path $env:ProgramFiles ('SYSU NetAuth Windows Check ' + [guid]::NewGuid().ToString('N'))
$private = Join-Path $env:ProgramData ('SYSUNetAuth-Package-Test-' + [guid]::NewGuid().ToString('N'))
$shared = Join-Path $env:ProgramData 'SYSUNetAuth'
$config = Join-Path $shared 'config.json'
$backup = Join-Path $private 'config.saved'
$before = Get-Service dot3svc | Select-Object Status,StartType
function Assert($Value, [string]$Message) { if (-not $Value) { throw $Message } }
function Install-Preview {
    $process = Start-Process -FilePath $Setup -ArgumentList @('/SP-', '/VERYSILENT', '/SUPPRESSMSGBOXES', '/NORESTART', ('/DIR="' + $install + '"'), ('/LOG="' + $OutputDirectory + '\install.log"')) -Wait -PassThru
    Assert ($process.ExitCode -eq 0) "Native installer failed: $($process.ExitCode)"
}
try {
    [IO.Directory]::CreateDirectory($private) | Out-Null
    $acl = New-Object Security.AccessControl.DirectorySecurity
    $acl.SetAccessRuleProtection($true, $false)
    foreach ($sid in @('S-1-5-18','S-1-5-32-544')) {
        $identity = New-Object Security.Principal.SecurityIdentifier($sid)
        $acl.AddAccessRule((New-Object Security.AccessControl.FileSystemAccessRule($identity,'FullControl','ContainerInherit,ObjectInherit','None','Allow')))
    }
    Set-Acl -LiteralPath $private -AclObject $acl
    if (Test-Path -LiteralPath $config) { Move-Item -LiteralPath $config -Destination $backup }
    [IO.Directory]::CreateDirectory($shared) | Out-Null
    Set-Acl -LiteralPath $shared -AclObject $acl
    [IO.File]::WriteAllText($config, '{"auto_auth":false}')
    Install-Preview
    $native = Join-Path $install 'sysu_netauth.exe'
    foreach ($name in @('sysu_netauth.exe','sysu_netauth_service.exe','sysu_netauth_gui.exe','sysu_netauth_notify.exe')) {
        $text = [Text.Encoding]::ASCII.GetString([IO.File]::ReadAllBytes((Join-Path $install $name)))
        Assert ($text -notmatch '(?i)wpcap\.dll|pcap_open_live|pcap_sendpacket|npcap-1\.88\.exe') "$name contains the legacy packet backend"
    }
    $service = Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Services\SYSUNetAuth'
    Assert (@($service.DependOnService) -contains 'ndisuio') 'Native service lacks its inbox driver dependency'
    & $native --json status > (Join-Path $OutputDirectory 'status.json')
    Assert ($LASTEXITCODE -eq 0) 'Native background service did not answer'
    $status = Get-Content (Join-Path $OutputDirectory 'status.json') -Raw | ConvertFrom-Json
    Assert ($status.state -eq 'idle') 'Offline installation unexpectedly authenticated'
    $task = Get-ScheduledTask -TaskName 'SYSUNetAuth Notifications' -ErrorAction Stop
    Assert ($task.Actions.Execute -like '*sysu_netauth_notify.exe*') 'Notification logon task targets an unexpected executable'
    'PASS: native-only payload, inbox driver dependency, service startup and idle status'
    Install-Preview
    & $native --json status > (Join-Path $OutputDirectory 'upgraded-status.json')
    Assert ($LASTEXITCODE -eq 0) 'Upgraded native service did not answer'
    Assert (-not (Compare-Object $before (Get-Service dot3svc | Select-Object Status,StartType) -Property Status,StartType)) 'Installer changed Windows wired authentication settings'
    'PASS: native installer upgrade and unchanged Wired AutoConfig settings'
} finally {
    if (Test-Path -LiteralPath (Join-Path $install 'unins000.exe')) {
        $process = Start-Process -FilePath (Join-Path $install 'unins000.exe') -ArgumentList @('/VERYSILENT','/SUPPRESSMSGBOXES','/NORESTART',('/LOG="'+$OutputDirectory+'\uninstall.log"')) -Wait -PassThru
        Assert ($process.ExitCode -eq 0) "Native uninstaller failed: $($process.ExitCode)"
        Assert (-not (Get-Service SYSUNetAuth -ErrorAction SilentlyContinue)) 'Uninstall left the preview service'
        Assert (-not (Get-ScheduledTask -TaskName 'SYSUNetAuth Notifications' -ErrorAction SilentlyContinue)) 'Uninstall left the notification task'
        Assert (-not (Test-Path -LiteralPath (Join-Path $install 'sysu_netauth_service.exe'))) 'Uninstall left the preview executable'
        'PASS: native preview uninstall removes its service and executables'
    }
    if (Test-Path -LiteralPath $backup) { Move-Item -LiteralPath $backup -Destination $config -Force }
    elseif (Test-Path -LiteralPath $config) { Remove-Item -LiteralPath $config -Force }
    $resolved = [IO.Path]::GetFullPath($private)
    $prefix = [IO.Path]::GetFullPath($env:ProgramData) + '\SYSUNetAuth-Package-Test-'
    if (-not $resolved.StartsWith($prefix,[StringComparison]::OrdinalIgnoreCase)) { throw 'Unsafe private test directory' }
    if (Test-Path -LiteralPath $resolved) { Remove-Item -LiteralPath $resolved -Recurse -Force }
}
