param(
    [switch]$SkipTests,
    [switch]$Offline,
    [string]$TargetDir,
    [string]$TauriCli
)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$native = Join-Path $repo 'native'
$app = Join-Path $native 'matra'
$artifactDir = Join-Path $repo 'artifacts'
$targetPath = if ($TargetDir) { $TargetDir } elseif ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $native 'target' }
if (![IO.Path]::IsPathRooted($targetPath)) { $targetPath = Join-Path $repo $targetPath }
$buildTarget = [IO.Path]::GetFullPath($targetPath)
$hookTarget = Join-Path $native 'target/hook' # The installer resource map and hooks.nsh expect this local static build.
$cargoArgs = @('--release', '--locked', '--manifest-path', (Join-Path $native 'Cargo.toml'))
if ($Offline) { $cargoArgs += '--offline' }
if ($TauriCli) {
    $TauriCli = (Resolve-Path -LiteralPath $TauriCli).Path
    $cliMeta = Get-Content -LiteralPath (Join-Path (Split-Path $TauriCli -Parent) 'package.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($cliMeta.name -ne '@tauri-apps/cli' -or $cliMeta.version -ne '2.11.4') { throw 'The cached Tauri CLI must be @tauri-apps/cli 2.11.4.' }
}

$oldTargetDir = $env:CARGO_TARGET_DIR
$oldOffline = $env:CARGO_NET_OFFLINE
try {
    $env:CARGO_TARGET_DIR = $buildTarget
    if ($Offline) { $env:CARGO_NET_OFFLINE = 'true' }
    & node (Join-Path $native 'scripts/test-installer-contract.cjs')
    if ($LASTEXITCODE -ne 0) { throw 'Installer contract failed.' }
    if (!$SkipTests) {
        & node (Join-Path $native 'test-codex-details.cjs')
        if ($LASTEXITCODE -ne 0) { throw 'Codex details UI tests failed.' }
        & cargo test @cargoArgs
        if ($LASTEXITCODE -ne 0) { throw 'Native tests failed.' }
    }

    $oldRustFlags = $env:RUSTFLAGS
    try {
        $env:RUSTFLAGS = '-C target-feature=+crt-static'
        & cargo build @cargoArgs -p matra-hook --target-dir $hookTarget
        if ($LASTEXITCODE -ne 0) { throw 'Hook build failed.' }
    } finally {
        $env:RUSTFLAGS = $oldRustFlags
    }

    $tauriArgs = @('build', '--config', 'tauri.bundle.conf.json', '--', '--locked')
    if ($Offline) { $tauriArgs += '--offline' }
    Push-Location $app
    try {
        if ($TauriCli) {
            & node $TauriCli @tauriArgs
        } else {
            $npxArgs = @('--yes')
            if ($Offline) { $npxArgs += '--offline' }
            $npxArgs += '@tauri-apps/cli@2.11.4'
            & npx.cmd @npxArgs @tauriArgs
        }
        if ($LASTEXITCODE -ne 0) { throw 'Tauri installer build failed.' }
    } finally {
        Pop-Location
    }
} finally {
    $env:CARGO_TARGET_DIR = $oldTargetDir
    $env:CARGO_NET_OFFLINE = $oldOffline
}

& node (Join-Path $native 'scripts/test-installer-contract.cjs') (Join-Path $buildTarget 'release/nsis/x64/installer.nsi')
if ($LASTEXITCODE -ne 0) { throw 'Generated installer contract failed.' }

$meta = Get-Content (Join-Path $app 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$setups = @(Get-ChildItem (Join-Path $buildTarget 'release/bundle/nsis') -Filter ('*_' + $meta.version + '_x64-setup.exe') -File)
if ($setups.Count -ne 1) { throw "Expected one NSIS installer, found $($setups.Count)." }
New-Item -ItemType Directory -Path $artifactDir -Force | Out-Null
$output = Join-Path $artifactDir 'Matra-Setup.exe'
Copy-Item -LiteralPath $setups[0].FullName -Destination $output -Force

Write-Output "Installer: $output"
Get-FileHash -LiteralPath $output -Algorithm SHA256
$checksum = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $artifactDir 'SHA256SUMS.txt'), "$checksum  Matra-Setup.exe`r`n", (New-Object Text.UTF8Encoding($false)))
& (Join-Path $PSScriptRoot 'prepare-share-folder.ps1') -Installer $output -Version $meta.version
