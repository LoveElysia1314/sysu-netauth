param([switch]$SkipInstaller)
$ErrorActionPreference = 'Stop'
$rustRoot = Split-Path $PSScriptRoot -Parent
Set-Location (Join-Path $rustRoot '../..')
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
$releaseDir = Join-Path $targetRoot 'x86_64-pc-windows-msvc/release'
cargo build --locked --release --target x86_64-pc-windows-msvc
if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed' }
cargo run --locked --quiet --example verify_windows -- $releaseDir
if ($LASTEXITCODE -ne 0) { throw 'Windows artifact verification failed' }
$output = Join-Path $rustRoot 'dist/sysu_netauth'
New-Item -ItemType Directory -Force $output | Out-Null
foreach ($executable in 'sysu_netauth', 'sysu_netauth_service', 'sysu_netauth_gui') {
    Copy-Item (Join-Path $releaseDir "$executable.exe") $output
}
Copy-Item (Join-Path $rustRoot 'assets/*.ico') $output
if (-not $SkipInstaller) {
    $versionLine = & (Join-Path $releaseDir 'sysu_netauth.exe') --version
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read version from the built CLI' }
    $version = ($versionLine -split '\s+')[-1]
    $fileVersion = [regex]::Match($version, '^[0-9]+\.[0-9]+\.[0-9]+').Value
    if (-not $fileVersion) { throw 'Cannot read installer version from the built CLI' }
    $setupBase = "SYSUNetAuth_Rust_msvc_x64_Setup_v$version"
    $compiler = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $compiler) { $compiler = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" }
    & $compiler "/DMyAppVersion=$version" "/DMyAppFileVersion=$fileVersion" (Join-Path $PSScriptRoot 'setup.template.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup build failed' }
    Compress-Archive -Force -Path (Join-Path $rustRoot "dist/$setupBase.exe") -DestinationPath (Join-Path $rustRoot "dist/$setupBase.zip")
}
