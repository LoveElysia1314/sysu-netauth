param(
    [switch]$SkipInstaller,
    [ValidateSet('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')]
    [string]$Target = ''
)
$ErrorActionPreference = 'Stop'
$rustRoot = Split-Path $PSScriptRoot -Parent
Set-Location $rustRoot
$targetRoot = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { 'target' }
if (-not $Target) {
    $Target = ((rustc -vV | Select-String '^host: ').ToString() -split '\s+')[1]
    if ($LASTEXITCODE -ne 0 -or $Target -notin @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')) {
        throw 'Use an x64 or ARM64 MSVC Rust toolchain, or pass -Target explicitly'
    }
}
$arch = if ($Target -eq 'aarch64-pc-windows-msvc') { 'arm64' } else { 'x64' }
$releaseDir = Join-Path $targetRoot "$Target/release"
cargo build --locked --release --target $Target
if ($LASTEXITCODE -ne 0) { throw 'Cargo build failed' }
cargo run --locked --quiet --example verify_windows -- $releaseDir $arch
if ($LASTEXITCODE -ne 0) { throw 'Windows artifact verification failed' }
$output = Join-Path $rustRoot $(if ($arch -eq 'arm64') { 'dist/arm64/sysu_netauth' } else { 'dist/sysu_netauth' })
$output = [IO.Path]::GetFullPath($output)
$distRoot = [IO.Path]::GetFullPath((Join-Path $rustRoot 'dist'))
if (-not $output.StartsWith($distRoot + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'Distribution output must stay inside this repository''s dist directory'
}
if (Test-Path -LiteralPath $output) { Remove-Item -LiteralPath $output -Recurse -Force }
New-Item -ItemType Directory -Force $output | Out-Null
foreach ($executable in 'sysu_netauth', 'sysu_netauth_service', 'sysu_netauth_gui') {
    Copy-Item (Join-Path $releaseDir "$executable.exe") $output
}
if (-not $SkipInstaller) {
    # Run the host CLI to read the version even when cross-compiling to ARM64.
    $versionLine = cargo run --locked --quiet --bin sysu_netauth -- --version
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read version from the built CLI' }
    $version = ($versionLine -split '\s+')[-1]
    $fileVersion = [regex]::Match($version, '^[0-9]+\.[0-9]+\.[0-9]+').Value
    if (-not $fileVersion) { throw 'Cannot read installer version from the built CLI' }
    $compiler = (Get-Command ISCC.exe -ErrorAction SilentlyContinue).Source
    if (-not $compiler) {
        foreach ($directory in @(${env:ProgramFiles(x86)}, $env:ProgramFiles, (Join-Path $env:LOCALAPPDATA 'Programs'))) {
            if (-not $directory) { continue }
            $candidate = Join-Path $directory 'Inno Setup 6/ISCC.exe'
            if (Test-Path -LiteralPath $candidate) { $compiler = $candidate; break }
        }
    }
    if (-not $compiler) { throw 'Install Inno Setup 6, or use -SkipInstaller to build the application binaries only' }
    & $compiler "/DMyAppVersion=$version" "/DMyAppFileVersion=$fileVersion" "/DMyAppArch=$arch" "/DAppSourceDir=$output" (Join-Path $PSScriptRoot 'setup.template.iss')
    if ($LASTEXITCODE -ne 0) { throw 'Inno Setup build failed' }
}
