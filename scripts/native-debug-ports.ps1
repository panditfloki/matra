param([Parameter(Mandatory)][int]$AppProcessId)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') { throw 'Disposable Windows CI only' }
# Separate WebView2 user-data folders have separate browser processes. Each
# owns an ephemeral debugger port; enumerate only this fixture app's children.
$processes = @(Get-CimInstance Win32_Process)
$owners = [System.Collections.Generic.HashSet[int]]::new()
[void]$owners.Add($AppProcessId)
do {
    $changed = $false
    foreach ($process in $processes) {
        if ($owners.Contains([int]$process.ParentProcessId) -and $owners.Add([int]$process.ProcessId)) { $changed = $true }
    }
} while ($changed)
$ports = @(Get-NetTCPConnection -State Listen -ErrorAction SilentlyContinue | Where-Object {
    $owners.Contains([int]$_.OwningProcess) -and $_.LocalAddress -in @('127.0.0.1','::1')
} | Select-Object -ExpandProperty LocalPort -Unique)
ConvertTo-Json -InputObject $ports -Compress
