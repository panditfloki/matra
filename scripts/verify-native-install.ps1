# Installs the built setup for the current user and verifies the real upgrade.
# Explicit invocation only: this stops/replaces the running Matra instance.
param([string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\Matra'))
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$config = Join-Path $env:APPDATA 'matra-notch\config.json'
$settings = Join-Path $env:USERPROFILE '.claude\settings.json'
function Get-ForeignHooks {
    if (!(Test-Path -LiteralPath $settings)) { return '' }
    $json = Get-Content -LiteralPath $settings -Raw | ConvertFrom-Json
    @($json.hooks.PSObject.Properties | ForEach-Object {
        $eventName = $_.Name
        foreach ($entry in $_.Value) {
            foreach ($hook in $entry.hooks) {
                if ($hook.command -notmatch 'matra-hook\.exe|Programs[\\/]MatraNotch') {
                    "$eventName|$($hook | ConvertTo-Json -Compress -Depth 20)"
                }
            }
        }
    }) | Sort-Object | Out-String
}
$configBefore = if (Test-Path -LiteralPath $config) { (Get-FileHash -LiteralPath $config).Hash } else { 'absent' }
$foreignBefore = Get-ForeignHooks
$setup = Join-Path $repo 'artifacts\Matra-Setup.exe'
$result = Start-Process -FilePath $setup -ArgumentList "/S /D=$InstallDir" -WindowStyle Hidden -PassThru -Wait
if ($result.ExitCode -ne 0) { throw "Setup failed: $($result.ExitCode)" }
$configAfter = if (Test-Path -LiteralPath $config) { (Get-FileHash -LiteralPath $config).Hash } else { 'absent' }
if ($configBefore -ne $configAfter) { throw 'Preferences changed during installation.' }
if ($foreignBefore -ne (Get-ForeignHooks)) { throw 'Foreign hooks changed during installation.' }
$exe = Join-Path $InstallDir 'matra.exe'
foreach ($file in @($exe, (Join-Path $InstallDir 'matra-hook.exe'), (Join-Path $InstallDir 'uninstall.exe'))) {
    if (!(Test-Path -LiteralPath $file)) { throw "Missing installed file: $file" }
}
# Tauri patches only its bundle marker to NSS while packaging, then restores UNK.
# Reproduce that documented byte change in memory, not in the executable on disk.
$buildBytes = [IO.File]::ReadAllBytes((Join-Path $repo 'native\target\release\matra.exe'))
$marker = '__TAURI_BUNDLE_TYPE_VAR_UNK'
$offset = [Text.Encoding]::ASCII.GetString($buildBytes).IndexOf($marker, [StringComparison]::Ordinal)
if ($offset -lt 0) { throw 'Tauri bundle marker not found.' }
$replacement = [Text.Encoding]::ASCII.GetBytes('__TAURI_BUNDLE_TYPE_VAR_NSS')
[Array]::Copy($replacement, 0, $buildBytes, $offset, $replacement.Length)
$sha = [Security.Cryptography.SHA256]::Create()
try { $expected = [BitConverter]::ToString($sha.ComputeHash($buildBytes)).Replace('-', '') }
finally { $sha.Dispose() }
if ((Get-FileHash -LiteralPath $exe).Hash -ne $expected) { throw 'Installed binary differs from packaged build.' }
$doctor = Start-Process -FilePath $exe -ArgumentList 'doctor' -WindowStyle Hidden -PassThru -Wait
if ($doctor.ExitCode -ne 0) { throw 'Doctor command failed.' }
$running = Start-Process -FilePath $exe -WindowStyle Hidden -PassThru
Start-Sleep -Seconds 4
$verified = Get-Process -Id $running.Id -ErrorAction SilentlyContinue
if (!$verified -or $verified.Path -ne $exe) { throw 'New installed app did not stay running.' }
$port = 48676
if (Test-Path -LiteralPath $config) {
    $prefs = Get-Content -LiteralPath $config -Raw | ConvertFrom-Json
    if ($prefs.port) { $port = [int]$prefs.port }
}
if (!(Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue | Where-Object OwningProcess -EQ $verified.Id)) {
    throw 'New app does not own its hook event port.'
}
Write-Output "PASS: setup, files, binary hash, doctor, running PID=$($verified.Id), event port=$port."
Write-Output 'PASS: existing preferences and foreign hooks preserved.'
Write-Output "Installed: $exe"
