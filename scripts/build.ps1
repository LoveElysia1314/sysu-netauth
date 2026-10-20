param([switch]$SkipInstaller)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')
cargo build --locked --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed' }
$output = 'dist\sysu_netauth'
New-Item -ItemType Directory -Force $output | Out-Null
Copy-Item 'target\x86_64-pc-windows-msvc\release\sysu_netauth.exe', 'target\x86_64-pc-windows-msvc\release\sysu_netauth_service.exe', 'target\x86_64-pc-windows-msvc\release\sysu_netauth_gui.exe' $output
Copy-Item 'assets\*.ico' $output
if (-not $SkipInstaller) {
    $compiler = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $compiler) { $compiler = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
    & $compiler 'scripts\setup.template.iss'
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup build failed' }
    Compress-Archive -Force -Path 'dist\SYSUNetAuth_Setup_v0.7.0.exe' -DestinationPath 'dist\SYSUNetAuth_Setup_v0.7.0.zip'
}
