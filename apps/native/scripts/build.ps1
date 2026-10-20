param([switch]$SkipInstaller)
$ErrorActionPreference = 'Stop'
$nativeRoot = Split-Path $PSScriptRoot -Parent
Set-Location (Join-Path $nativeRoot '../..')
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$releaseDir = Join-Path $targetRoot 'x86_64-pc-windows-msvc/release'
cargo build --locked --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed' }
cargo run --locked --quiet --example verify_windows -- $releaseDir
if ($LASTEXITCODE -ne 0) { throw 'Windows artifact verification failed' }
$output = Join-Path $nativeRoot 'dist/sysu_netauth'
New-Item -ItemType Directory -Force $output | Out-Null
foreach ($executable in 'sysu_netauth', 'sysu_netauth_service', 'sysu_netauth_gui') {
    Copy-Item (Join-Path $releaseDir "$executable.exe") $output
}
Copy-Item (Join-Path $nativeRoot 'assets/*.ico') $output
if (-not $SkipInstaller) {
    $compiler = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $compiler) { $compiler = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
    & $compiler (Join-Path $PSScriptRoot 'setup.template.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup build failed' }
    Compress-Archive -Force -Path (Join-Path $nativeRoot 'dist/SYSUNetAuth_Setup_v0.7.0.exe') -DestinationPath (Join-Path $nativeRoot 'dist/SYSUNetAuth_Setup_v0.7.0.zip')
}
