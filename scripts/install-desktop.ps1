$ErrorActionPreference = 'Stop'
$repo = Split-Path -Parent $PSScriptRoot
$version = (Get-Content -LiteralPath (Join-Path $repo 'desktop\version.json') -Raw | ConvertFrom-Json).version
$build = Join-Path $repo "dist\Matra-$version-win32-x64"
$target = Join-Path $env:LOCALAPPDATA "Programs\Matra\$version"
if (-not (Test-Path -LiteralPath (Join-Path $build 'Matra.exe'))) { throw 'Run npm run desktop:build first.' }
if (Test-Path -LiteralPath $target) { throw "Existing installation preserved: $target" }
New-Item -ItemType Directory -Path (Split-Path -Parent $target) -Force | Out-Null
Copy-Item -LiteralPath $build -Destination $target -Recurse
$shell = New-Object -ComObject WScript.Shell
$shortcutPath = Join-Path ([Environment]::GetFolderPath('Programs')) 'Matra Desktop.lnk'
$shortcut = $shell.CreateShortcut($shortcutPath)
$shortcut.TargetPath = Join-Path $target 'Matra.exe'
$shortcut.WorkingDirectory = $target
$shortcut.Description = 'Mātrā desktop usage widget'
$shortcut.Save()
Write-Output "Installed: $target"
Write-Output "Start Menu: $shortcutPath"
# Hidden startup avoids a console flash; the app displays its own widget.
Start-Process -FilePath (Join-Path $target 'Matra.exe') -WorkingDirectory $target -WindowStyle Hidden
