$serviceName = "SYSUNetAuth"
$dataDir = Join-Path $env:ProgramData "SYSUNetAuth"
$statusPath = Join-Path $dataDir "status.json"
$bootstrapLog = Join-Path $dataDir "service_bootstrap.log"
$serviceLog = Join-Path $dataDir "service.log"

Write-Host "== Windows Service =="
sc.exe query $serviceName

Write-Host
Write-Host "== Status JSON =="
if (Test-Path $statusPath) {
    Get-Content $statusPath
} else {
    Write-Host "No status file: $statusPath"
}

Write-Host
Write-Host "== Bootstrap Log =="
if (Test-Path $bootstrapLog) {
    Get-Content $bootstrapLog -Tail 40
} else {
    Write-Host "No bootstrap log: $bootstrapLog"
}

Write-Host
Write-Host "== Service Log =="
if (Test-Path $serviceLog) {
    Get-Content $serviceLog -Tail 40
} else {
    Write-Host "No service log: $serviceLog"
}
