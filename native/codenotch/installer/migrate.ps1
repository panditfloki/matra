param(
    [Parameter(Mandatory)][ValidateSet('Pre','Post')][string]$Phase,
    [Parameter(Mandatory)][string]$InstallDir
)
$ErrorActionPreference = 'Stop'
try {
    $destination = [IO.Path]::GetFullPath($InstallDir).TrimEnd('\')
    $legacyRoot = [IO.Path]::GetFullPath((Join-Path $env:LOCALAPPDATA 'Programs\MatraNotch'))
    $exe = Join-Path $destination 'matra.exe'
    if ($Phase -eq 'Pre') {
        # Never kill upstream CodeNotch, or an executable outside this product's paths.
        $candidates = @(Get-Process matra,codenotch -ErrorAction SilentlyContinue)
        foreach ($candidate in $candidates) {
            if (!$candidate.Path) { continue }
            $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($candidate.Path))
            $legacy = [IO.Path]::GetDirectoryName($parent) -eq $legacyRoot
            if ($legacy -or ($parent -eq $destination -and $candidate.ProcessName -eq 'matra')) {
                $oldId = $candidate.Id
                Stop-Process -InputObject $candidate -Force
                $candidate.Dispose()
                $deadline = [DateTime]::UtcNow.AddSeconds(30)
                do {
                    # Process objects held by PowerShell can outlive termination.
                    # Query the OS process table, not that cached .NET object.
                    $remaining = Get-CimInstance Win32_Process -Filter "ProcessId = $oldId"
                    if (!$remaining) { break }
                    Start-Sleep -Milliseconds 100
                } while ([DateTime]::UtcNow -lt $deadline)
                if ($remaining) { throw 'The old Matra process did not exit.' }
            }
        }
    } else {
        if (!(Test-Path -LiteralPath $exe)) { throw 'Installed matra.exe is missing.' }
        if (!(Test-Path -LiteralPath (Join-Path $destination 'matra-hook.exe'))) { throw 'Installed hook is missing.' }
        # Preserve enabled preferences. No new hooks or startup registrations are opted in.
        $settings = Join-Path ([Environment]::GetFolderPath('UserProfile')) '.claude\settings.json'
        if ((Test-Path -LiteralPath $settings) -and ((Get-Content -LiteralPath $settings -Raw) -match 'matra-hook\.exe')) {
            $migration = Start-Process -FilePath $exe -ArgumentList 'install-hooks' -WindowStyle Hidden -PassThru -Wait
            if ($migration.ExitCode -ne 0) { throw 'Claude hook migration failed; settings backup is retained.' }
        }
        $runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
        if (Get-ItemProperty -LiteralPath $runKey -Name MatraNotch -ErrorAction SilentlyContinue) {
            Set-ItemProperty -LiteralPath $runKey -Name MatraNotch -Value ('"' + $exe + '" --silent')
        }
        # Retarget only shortcuts created by the former Matra preview installer.
        $shell = New-Object -ComObject WScript.Shell
        $programs = [Environment]::GetFolderPath('Programs')
        foreach ($link in (Get-ChildItem -LiteralPath $programs -Filter '*.lnk' -File)) {
            $shortcut = $shell.CreateShortcut($link.FullName)
            $target = $shortcut.TargetPath
            if (!$target) { continue }
            # Unrelated third-party shortcuts can contain shell namespace paths.
            # They are not filesystem targets and must not block this installer.
            try { $parent = [IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($target)) }
            catch { continue }
            if ([IO.Path]::GetDirectoryName($parent) -eq $legacyRoot) {
                $shortcut.TargetPath = $exe
                $shortcut.WorkingDirectory = $destination
                $shortcut.IconLocation = "$exe,0"
                $shortcut.Save()
            }
        }
    }
    Write-Output "Matra $Phase migration complete."
    exit 0
} catch {
    $logDir = Join-Path $env:APPDATA 'matra-notch'
    New-Item -ItemType Directory -Force -Path $logDir | Out-Null
    "$Phase migration failed: $($_.Exception.Message)" | Out-File -LiteralPath (Join-Path $logDir 'setup-migration.log') -Encoding UTF8 -Append
    Write-Output $_.Exception.Message
    exit 1
}
