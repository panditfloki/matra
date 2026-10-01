param(
    [Parameter(Mandatory)][string]$Installer,
    [Parameter(Mandatory)][string]$PreviousInstaller
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows') {
    throw 'This lifecycle test is restricted to a disposable GitHub Windows runner.'
}
$repo = Split-Path $PSScriptRoot -Parent
$expectedVersion = (Get-Content -LiteralPath (Join-Path $repo 'native/matra/tauri.conf.json') -Raw | ConvertFrom-Json).version
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\Matra CI Mātrā पंडित'
$exe = Join-Path $installDir 'matra.exe'
$uninstaller = Join-Path $installDir 'uninstall.exe'
$dataDir = Join-Path $env:APPDATA 'matra-notch'
$configFile = Join-Path $dataDir 'config.json'
$claudeDir = Join-Path $env:USERPROFILE '.claude'
$claudeFile = Join-Path $claudeDir 'settings.json'
$runKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$approvedKey = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'

if ((Test-Path $installDir) -or (Test-Path $claudeFile) -or (Test-Path $configFile)) {
    throw 'Refusing to overwrite pre-existing runner app data.'
}
function Assert-True([bool]$Condition, [string]$Message) {
    if (!$Condition) { throw $Message }
    Write-Output "PASS: $Message"
}
function Run-Checked([string]$File, [string]$Arguments) {
    $process = Start-Process -FilePath $File -ArgumentList $Arguments -PassThru
    if (!$process.WaitForExit(180000)) { throw "Timed out: $File $Arguments" }
    Assert-True ($process.ExitCode -eq 0) "Exit 0: $Arguments"
}
function Wait-Removed([string]$Path) {
    for ($attempt = 0; $attempt -lt 100 -and (Test-Path $Path); $attempt++) { Start-Sleep -Milliseconds 100 }
    Assert-True (!(Test-Path $Path)) 'Uninstaller removed installed executable'
}
function Run-Value {
    return (Get-ItemProperty -Path $runKey -Name MatraNotch -ErrorAction SilentlyContinue).MatraNotch
}
function Get-HookCommands {
    $settings = Get-Content -LiteralPath $claudeFile -Raw | ConvertFrom-Json
    return @($settings.hooks.PSObject.Properties.Value | ForEach-Object { $_ } | ForEach-Object { $_.hooks } | ForEach-Object { $_.command })
}
function Assert-ForeignHook {
    Assert-True ((Get-HookCommands) -contains '"D:\Foreign\matra-hook.exe" running') 'Foreign hook preserved exactly'
}

New-Item -ItemType Directory -Path $claudeDir, $dataDir -Force | Out-Null
$fixture = @{ hooks = @{ PreToolUse = @(@{ matcher = '*'; hooks = @(@{ type = 'command'; command = '"D:\Foreign\matra-hook.exe" running'; timeout = 7 }) }) }; sentinel = 'preserve-me' }
$fixture | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $claudeFile -Encoding utf8NoBOM

# Fresh installation must not enable startup or hooks without consent.
Run-Checked $Installer "/S /D=$installDir"
Assert-True (Test-Path $exe) 'Fresh installation completed at a Unicode path'
Assert-True ((Get-Item $exe).VersionInfo.ProductVersion -like "$expectedVersion*") "Installed product version is $expectedVersion"
Assert-True (!(Run-Value)) 'Fresh installation did not enable startup'
Assert-True ((Get-HookCommands).Count -eq 1) 'Fresh installation did not enable hooks'
Run-Checked $exe 'doctor'
Assert-True ((Get-Content (Join-Path $dataDir 'doctor.log') -Raw) -match 'Matra doctor') 'Installed binary ran doctor'

# Explicit hook opt-in remains idempotent and does not remove another product.
Run-Checked $exe 'install-hooks'
Run-Checked $exe 'install-hooks'
Assert-True ((Get-HookCommands).Count -eq 8) 'Seven owned hooks plus one foreign hook, no duplicates'
Assert-ForeignHook
Run-Checked $exe 'autostart on'
Assert-True ((Run-Value) -ceq "`"$exe`" --silent") 'Unicode startup command round-tripped'

# Windows-disabled startup must remain disabled, not silently re-enabled.
New-Item -Path $approvedKey -Force | Out-Null
New-ItemProperty -Path $approvedKey -Name MatraNotch -PropertyType Binary -Value ([byte[]](3,0,0,0,0,0,0,0,0,0,0,0)) -Force | Out-Null
$blocked = Start-Process -FilePath $exe -ArgumentList 'autostart on' -PassThru -Wait
Assert-True ($blocked.ExitCode -ne 0) 'Windows-disabled startup rejects enabling with a real failure'
Remove-ItemProperty -Path $approvedKey -Name MatraNotch

# Run the packaged WebView2 with no signed-in accounts. Exercise real IPC and
# capture screenshots, never claiming these fixtures are real provider usage.
# WebView2 150+ ignores environment debug arguments in an elevated runner.
# Use Microsoft's documented app-specific debugger configuration only in this
# disposable CI VM. Never ship a debug flag or change a user's Windows policy.
# https://github.com/MicrosoftEdge/WebView2Feedback/issues/5645
$debugKey = 'HKLM:\SOFTWARE\Policies\Microsoft\Edge\WebView2\AdditionalBrowserArguments'
if ((Get-ItemProperty -Path $debugKey -Name 'matra.exe' -ErrorAction SilentlyContinue)) {
    throw 'Refusing to overwrite existing WebView2 debugger configuration.'
}
New-Item -Path $debugKey -Force | Out-Null
New-ItemProperty -Path $debugKey -Name 'matra.exe' -PropertyType String -Value '--remote-debugging-port=9337' | Out-Null
$env:WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS = '--remote-debugging-port=9337'
try {
    $native = Start-Process -FilePath $exe -ArgumentList '--silent' -PassThru -RedirectStandardError (Join-Path $dataDir 'launch-stderr.log')
    & node (Join-Path $PSScriptRoot 'test-windows-native-ui.cjs')
    $uiFailed = $LASTEXITCODE -ne 0
    if (!$uiFailed) { & (Join-Path $PSScriptRoot 'test-windows-topmost.ps1') -AppProcessId $native.Id }
} finally {
    Remove-ItemProperty -Path $debugKey -Name 'matra.exe'
    Remove-Item Env:\WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS
}
if ($uiFailed) {
    # Keep the failed gate, but collect independent lifecycle evidence in this
    # disposable account before failing the job. Never upload personal logs.
    $native.Refresh()
    Write-Output "UI launch PID=$($native.Id), exited=$($native.HasExited), exit=$($native.ExitCode)"
    Get-CimInstance Win32_Process | Where-Object { $_.Name -match '^(matra|msedgewebview2)\.exe$' } | Select-Object Name, ProcessId, SessionId, CommandLine | Format-List
    foreach ($log in @('launch-stderr.log', 'run.log', 'install.log', 'doctor.log')) {
        $logPath = Join-Path $dataDir $log
        if (Test-Path $logPath) { Write-Output "Diagnostic: $log"; Get-Content -LiteralPath $logPath -Tail 80 }
    }
    Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = (Get-Date).AddMinutes(-5) } -ErrorAction SilentlyContinue | Where-Object { $_.Message -match 'matra|WebView2' } | Select-Object -First 5 TimeCreated, Message | Format-List
}

# Uninstall while running: no orphan process or owned hooks/startup; data stays.
Run-Checked $uninstaller '/S'
Wait-Removed $exe
Assert-True ($native.WaitForExit(30000)) 'Uninstall stopped the installed app'
Assert-True (!(Run-Value)) 'Uninstall removed owned startup entry'
Assert-True ((Get-HookCommands).Count -eq 1) 'Uninstall removed only owned hooks'
Assert-ForeignHook
Assert-True (Test-Path $configFile) 'Uninstall preserved saved preferences'

# Upgrade from the actual public 1.8.4 installer. No rollout or release creation.
Run-Checked $PreviousInstaller "/S /D=$installDir"
Assert-True ((Get-Item $exe).VersionInfo.ProductVersion -like '1.8.4*') 'Public 1.8.4 baseline installed'
Run-Checked $exe 'install-hooks'
# 1.8.4's console-registry readback cannot round-trip this Unicode path. Seed
# its existing opt-in directly; the new binary's real on/disabled paths were
# exercised above. The upgrade must retain this exact Windows Run entry.
New-ItemProperty -Path $runKey -Name MatraNotch -PropertyType String -Value "`"$exe`" --silent" -Force | Out-Null
Assert-True ((Run-Value) -ceq "`"$exe`" --silent") 'Baseline has an enabled Unicode startup fixture'
# The public baseline matches hooks by basename. Introduce the foreign hook
# after its opt-in so this upgrade gate tests 1.8.5, not the old install command.
if (!((Get-HookCommands) -contains '"D:\Foreign\matra-hook.exe" running')) {
    $baselineHooks = Get-Content -LiteralPath $claudeFile -Raw | ConvertFrom-Json
    $baselineHooks.hooks.PreToolUse = @($baselineHooks.hooks.PreToolUse) + $fixture.hooks.PreToolUse
    $baselineHooks | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $claudeFile -Encoding utf8NoBOM
}
Assert-True ((Get-HookCommands).Count -eq 8) 'Upgrade starts with seven owned hooks and one foreign hook'
Assert-ForeignHook
# Use only fields that actually existed in the public 1.8.4 Config struct.
# Accent, quota colours and automatic updates were added by this candidate.
$saved = @{ theme = 'glass'; notch_edge = 'left'; scale = 0.8; weekly_ring = 'inside'; glm_notch_fixed = $true; notch_on_hover = $true; notch_motion = $true; notch_visible = $true; tray_visible = $true }
$saved | ConvertTo-Json | Set-Content -LiteralPath $configFile -Encoding utf8NoBOM
$old = Start-Process -FilePath $exe -ArgumentList '--silent' -PassThru
Start-Sleep -Seconds 3
# The previous app normalises its config during launch. Compare the settled
# installed app's data immediately before and after Setup, not pre-launch bytes.
$settled = Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json
Assert-True ($settled.theme -eq 'glass' -and $settled.notch_edge -eq 'left' -and $settled.scale -eq 0.8 -and $settled.weekly_ring -eq 'inside') 'Baseline retained the selected preferences before upgrade'
$before = (Get-FileHash -LiteralPath $configFile -Algorithm SHA256).Hash
Run-Checked $Installer "/S /UPDATE /D=$installDir"
Assert-True ($old.WaitForExit(30000)) 'Upgrade stopped the previous installed process'
Assert-True ((Get-Item $exe).VersionInfo.ProductVersion -like "$expectedVersion*") "Upgrade installed $expectedVersion"
Assert-True ((Get-FileHash -LiteralPath $configFile -Algorithm SHA256).Hash -eq $before) 'Upgrade preserved preferences byte-for-byte'
Assert-True ((Run-Value) -ceq "`"$exe`" --silent") 'Upgrade preserved startup opt-in'
Assert-True ((Get-HookCommands).Count -eq 8) 'Upgrade preserved hook opt-in without duplicates'
Assert-ForeignHook
Run-Checked $exe 'doctor'
$upgraded = Start-Process -FilePath $exe -ArgumentList '--silent' -PassThru
Start-Sleep -Seconds 3
$upgraded.Refresh()
Assert-True (!$upgraded.HasExited) "Upgraded $expectedVersion app stayed running"
$reloaded = Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json
Assert-True ($reloaded.theme -eq 'glass' -and $reloaded.notch_edge -eq 'left' -and $reloaded.scale -eq 0.8 -and $reloaded.weekly_ring -eq 'inside') 'Upgraded app loaded the previous preferences'
Assert-True ($reloaded.automatic_updates -eq $false -and $reloaded.quota_colors.watch -eq 0.5 -and $reloaded.quota_colors.critical -eq 0.7) 'New preferences have safe backward-compatible defaults'

# Invalid settings must fail without rewriting a user's existing file.
$validHooks = Get-Content -LiteralPath $claudeFile -Raw
'{ invalid json' | Set-Content -LiteralPath $claudeFile -Encoding utf8NoBOM
$invalidHash = (Get-FileHash -LiteralPath $claudeFile -Algorithm SHA256).Hash
$bad = Start-Process -FilePath $exe -ArgumentList 'install-hooks' -PassThru -Wait
Assert-True ($bad.ExitCode -ne 0) 'Invalid settings produce nonzero exit status'
Assert-True ((Get-FileHash -LiteralPath $claudeFile -Algorithm SHA256).Hash -eq $invalidHash) 'Invalid settings were left untouched'
$validHooks | Set-Content -LiteralPath $claudeFile -Encoding utf8NoBOM

# A startup registration owned by another executable must survive uninstall.
New-ItemProperty -Path $runKey -Name MatraNotch -PropertyType String -Value '"D:\Foreign\matra.exe" --silent' -Force | Out-Null
Run-Checked $uninstaller '/S'
Wait-Removed $exe
Assert-True ($upgraded.WaitForExit(30000)) 'Final uninstall stopped the upgraded app'
Assert-True ((Run-Value) -ceq '"D:\Foreign\matra.exe" --silent') 'Uninstall preserved a foreign startup registration'
Assert-ForeignHook
Assert-True ((Get-HookCommands).Count -eq 1) 'Final uninstall removed only owned hooks'
Assert-True (Test-Path $configFile) 'Final uninstall retained user data'
if ($uiFailed) { throw 'Lifecycle completed, but the packaged native UI gate failed; see launch diagnostics above.' }
Write-Output 'PASS: Windows installation, native UI, upgrade, hook ownership and uninstall lifecycle.'
