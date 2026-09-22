param([switch]$SkipTests)

$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$native = Join-Path $repo 'native'
$app = Join-Path $native 'codenotch'
$artifactDir = Join-Path $repo 'artifacts'

if (!$SkipTests) {
    & cargo test --release --locked --manifest-path (Join-Path $native 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'Native tests failed.' }
}

$oldRustFlags = $env:RUSTFLAGS
try {
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    & cargo build --release --locked --manifest-path (Join-Path $native 'Cargo.toml') -p codenotch-hook --target-dir (Join-Path $native 'target/hook')
    if ($LASTEXITCODE -ne 0) { throw 'Hook build failed.' }
} finally {
    $env:RUSTFLAGS = $oldRustFlags
}

Push-Location $app
try {
    & npx.cmd --yes '@tauri-apps/cli@2.11.4' build --config tauri.bundle.conf.json -- --locked
    if ($LASTEXITCODE -ne 0) { throw 'Tauri installer build failed.' }
} finally {
    Pop-Location
}

$meta = Get-Content (Join-Path $app 'tauri.conf.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$setups = @(Get-ChildItem (Join-Path $native 'target/release/bundle/nsis') -Filter ('*_' + $meta.version + '_x64-setup.exe') -File)
if ($setups.Count -ne 1) { throw "Expected one NSIS installer, found $($setups.Count)." }
New-Item -ItemType Directory -Path $artifactDir -Force | Out-Null
$output = Join-Path $artifactDir 'Matra-Setup.exe'
Copy-Item -LiteralPath $setups[0].FullName -Destination $output -Force

Write-Output "Installer: $output"
Get-FileHash -LiteralPath $output -Algorithm SHA256
$checksum = (Get-FileHash -LiteralPath $output -Algorithm SHA256).Hash.ToLowerInvariant()
[IO.File]::WriteAllText((Join-Path $artifactDir 'SHA256SUMS.txt'), "$checksum  Matra-Setup.exe`r`n", (New-Object Text.UTF8Encoding($false)))
& (Join-Path $PSScriptRoot 'prepare-share-folder.ps1') -Installer $output -Version $meta.version
