param(
    [string]$Installer,
    [string]$Version,
    [string]$Status = 'Local build; not a published release',
    [string]$Destination
)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
if (!$Installer) { $Installer = Join-Path $repo 'artifacts\Matra-Setup.exe' }
if (!$Destination) { $Destination = Join-Path $repo 'Matra Notch' }
$source = (Resolve-Path -LiteralPath $Installer).Path
$dest = [IO.Path]::GetFullPath($Destination).TrimEnd('\')
if ([IO.Path]::GetPathRoot($dest).TrimEnd('\') -eq $dest -or $dest -eq $repo) {
    throw 'Destination must be a dedicated distribution folder.'
}
if (!$Version) { $Version = (Get-Item -LiteralPath $source).VersionInfo.ProductVersion }
if (!$Version -or $Version -notmatch '^\d+\.\d+\.\d+(?:[-+][A-Za-z0-9.-]+)?$') { throw 'Supply a valid version.' }
$target = Join-Path $dest 'Matra-Setup.exe'
if ($source -eq $target) { throw 'Source installer must be outside the share folder.' }
$hash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).Hash.ToLowerInvariant()
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$archiveRoot = Join-Path $dest 'Archive'
New-Item -ItemType Directory -Force -Path $archiveRoot | Out-Null
# Stage the new binary before moving the current one. Never destroy the only copy.
$stage = Join-Path $dest ('setup-' + [guid]::NewGuid().ToString('N') + '.pending')
Copy-Item -LiteralPath $source -Destination $stage
if ((Get-FileHash -LiteralPath $stage).Hash.ToLowerInvariant() -ne $hash) { throw 'Staged installer checksum mismatch.' }
if (Test-Path -LiteralPath $target) {
    $oldHash = (Get-FileHash -LiteralPath $target).Hash.ToLowerInvariant()
    if ($oldHash -ne $hash) {
        $oldVersion = 'previous'
        $oldManifest = Join-Path $dest 'VERSION.json'
        if (Test-Path -LiteralPath $oldManifest) {
            try { $oldVersion = [string](Get-Content -LiteralPath $oldManifest -Raw | ConvertFrom-Json).version } catch {}
        }
        $safeVersion = $oldVersion -replace '[^A-Za-z0-9.-]', '_'
        $archive = [IO.Path]::GetFullPath((Join-Path $archiveRoot ($safeVersion + '-' + (Get-Date -Format 'yyyyMMdd-HHmmss-fff') + '-' + $oldHash.Substring(0,8))))
        if (!$archive.StartsWith($archiveRoot + '\', [StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid archive path.' }
        New-Item -ItemType Directory -Path $archive | Out-Null
        foreach ($name in @('Matra-Setup.exe','SHA256SUMS.txt','VERSION.json','READ-ME.txt','LICENSE.txt','THIRD-PARTY-NOTICES.txt','PROVIDER-NOTICES.txt')) {
            $item = Join-Path $dest $name
            if (Test-Path -LiteralPath $item) { Move-Item -LiteralPath $item -Destination (Join-Path $archive $name) }
        }
        Write-Output "Previous installer retained: $archive"
    }
}
Move-Item -LiteralPath $stage -Destination $target -Force
Copy-Item -LiteralPath (Join-Path $repo 'LICENSE') -Destination (Join-Path $dest 'LICENSE.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'native\LICENSE') -Destination (Join-Path $dest 'THIRD-PARTY-NOTICES.txt') -Force
Copy-Item -LiteralPath (Join-Path $repo 'native\codenotch\glyphs\NOTICE.md') -Destination (Join-Path $dest 'PROVIDER-NOTICES.txt') -Force
$utf8 = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText((Join-Path $dest 'SHA256SUMS.txt'), "$hash  Matra-Setup.exe`r`n", $utf8)
$manifest = @{product='Matra';version=$Version;sha256=$hash;status=$Status;prepared_at=(Get-Date).ToUniversalTime().ToString('o')} | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path $dest 'VERSION.json'), $manifest, $utf8)
$readme = @"
MATRA NOTCH - WINDOWS INSTALLER
Version: $Version
Status: $Status

Double-click Matra-Setup.exe to INSTALL or UPDATE the app.
After installation, use the Start menu shortcut to RUN Matra.
The setup is not a portable app. Running it repeatedly opens installation again.

Share this current folder's files together (not the source repository or Archive).
Archive retains previous installers when a different build is prepared here.
No source code, developer tools, accounts or credentials are included.
The installer is unsigned; verify SHA256SUMS.txt against your trusted release.

UNINSTALL TEST
Windows Settings > Apps > Installed apps. Locate each app by its exact name.
Uninstall Matra and upstream CodeNotch separately; they are different products.
Keep your personal settings unless you explicitly want to reset them.
If an old preview has no uninstall entry, do not delete random project folders.
Then double-click this setup and launch from Start.

Required third-party license notices are retained. They are not product branding.
"@
[IO.File]::WriteAllText((Join-Path $dest 'READ-ME.txt'), $readme, $utf8)
Write-Output "Share installer: $target"
Write-Output "Version: $Version | SHA256: $hash"
