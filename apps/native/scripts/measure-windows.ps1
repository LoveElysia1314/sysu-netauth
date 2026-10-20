param([string]$ProcessName = 'sysu_netauth_service', [int]$Seconds = 600)
$ErrorActionPreference = 'Stop'
$process = Get-Process -Name $ProcessName | Select-Object -First 1
$startCpu = $process.TotalProcessorTime.TotalSeconds
$watch = [Diagnostics.Stopwatch]::StartNew()
Start-Sleep -Seconds $Seconds
$process.Refresh()
[pscustomobject]@{
    Process = $ProcessName
    Seconds = $watch.Elapsed.TotalSeconds
    CpuPercentOneCore = 100 * ($process.TotalProcessorTime.TotalSeconds - $startCpu) / $watch.Elapsed.TotalSeconds
    WorkingSetBytes = $process.WorkingSet64
    PrivateBytes = $process.PrivateMemorySize64
    Threads = $process.Threads.Count
} | ConvertTo-Json
