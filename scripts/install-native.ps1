param([switch]$NoLaunch)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$meta = Get-Content (Join-Path $repo 'native/matra/tauri.conf.json') -Raw | ConvertFrom-Json
$destination = Join-Path $env:LOCALAPPDATA ('Programs\MatraNotch\' + $meta.version)
$source = Join-Path $repo 'native/target/release'
foreach ($binary in @('matra.exe', 'matra-hook.exe')) {
    if (!(Test-Path -LiteralPath (Join-Path $source $binary))) { throw "Build first: npm run native:build" }
}
New-Item -ItemType Directory -Path $destination -Force | Out-Null
$executable = Join-Path $destination 'matra.exe'
$prepare = Start-Process -FilePath (Join-Path $source 'matra-hook.exe') -ArgumentList ('--prepare-install "' + $destination + '"') -PassThru -Wait
if ($prepare.ExitCode -ne 0) { throw 'Matra did not exit; installation stopped.' }
Copy-Item -LiteralPath (Join-Path $source 'matra.exe') -Destination $destination -Force
Copy-Item -LiteralPath (Join-Path $source 'matra-hook.exe') -Destination $destination -Force
Copy-Item -LiteralPath (Join-Path $repo 'native/LICENSE') -Destination $destination -Force
Copy-Item -LiteralPath (Join-Path $repo 'native/UPSTREAM.md') -Destination $destination -Force
# Refresh only an already-enabled Matra hook. Never enable or remove foreign hooks.
$migration = Start-Process -FilePath $executable -ArgumentList 'setup-migrate' -WindowStyle Hidden -PassThru -Wait
if ($migration.ExitCode -ne 0) { throw 'Migration failed; see matra-notch/install.log. Settings backup retained.' }
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'Matra Desktop.lnk'
$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = $executable
$shortcut.WorkingDirectory = $destination
$displayName = 'dy/dx.f(M' + [char]0x0101 + 'tr' + [char]0x0101 + ')'
$shortcut.Description = $displayName
$shortcut.Save()
# Windows shortcut filenames cannot contain '/'; only the filename uses a hyphen.
$asciiShortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'dy-dx.f(Matra).lnk'
$brandedShortcut = $shell.CreateShortcut($asciiShortcutPath)
$brandedShortcut.TargetPath = $executable
$brandedShortcut.WorkingDirectory = $destination
$brandedShortcut.Description = $displayName
$brandedShortcut.Save()
# WScript.Shell normalizes some non-ASCII path characters. Rename through Unicode APIs.
$unicodeShortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) ($displayName.Replace('/', '-') + '.lnk')
Move-Item -LiteralPath $asciiShortcutPath -Destination $unicodeShortcutPath -Force
Write-Output "Installed: $executable"
Write-Output "Start Menu: Matra Desktop. Launch again to open Settings."
if (!$NoLaunch) {
    $launched = Start-Process -FilePath $executable -WindowStyle Hidden -PassThru
    Start-Sleep -Seconds 3
    $verified = Get-Process -Id $launched.Id -ErrorAction SilentlyContinue
    if (!$verified -or $verified.Path -ne $executable) { throw 'Launch verification failed: new version is not running.' }
    $configPath=Join-Path $env:APPDATA 'matra-notch\config.json'
    $eventPort=48676
    if(Test-Path -LiteralPath $configPath){$currentConfig=Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json;if($currentConfig.port){$eventPort=[int]$currentConfig.port}}
    $listener=Get-NetTCPConnection -LocalPort $eventPort -State Listen -ErrorAction SilentlyContinue
    if (!($listener | Where-Object OwningProcess -EQ $verified.Id)) {throw "New version does not own its event port $eventPort."}
    $sourceHash=(Get-FileHash -LiteralPath (Join-Path $source 'matra.exe')).Hash
    if ((Get-FileHash -LiteralPath $verified.Path).Hash -ne $sourceHash) {throw 'Running executable hash does not match build.'}
    Write-Output "Verified running: PID=$($verified.Id) $($verified.Path)"
}
