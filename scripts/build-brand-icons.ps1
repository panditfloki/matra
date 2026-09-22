param([string]$Source)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
$repo = Split-Path $PSScriptRoot -Parent
$brand = Join-Path $repo 'media/brand'
$icons = Join-Path $repo 'native/codenotch/icons'
$ui = Join-Path $repo 'native/codenotch/ui'
New-Item -ItemType Directory -Force -Path $brand | Out-Null
$master = Join-Path $brand 'matra-logo-18.png'
if ($Source) { Copy-Item -LiteralPath $Source -Destination $master }
if (-not (Test-Path -LiteralPath $master)) { throw 'Supply the approved Logo-18 PNG with -Source.' }
$original = [Drawing.Bitmap]::FromFile($master)
try {
    $frames = @()
    foreach ($size in @(16, 20, 24, 32, 48, 64, 128, 256)) {
        $bitmap = New-Object Drawing.Bitmap($size, $size, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        $graphics = [Drawing.Graphics]::FromImage($bitmap)
        try {
            $graphics.Clear([Drawing.Color]::Transparent)
            $graphics.InterpolationMode = [Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.DrawImage($original, [Drawing.Rectangle]::new(0, 0, $size, $size))
            $stream = New-Object IO.MemoryStream
            try {
                $bitmap.Save($stream, [Drawing.Imaging.ImageFormat]::Png)
                $frames += ,@{ Size = $size; Bytes = $stream.ToArray() }
                if ($size -eq 32) { $bitmap.Save((Join-Path $icons 'tray-color.png'), [Drawing.Imaging.ImageFormat]::Png) }
                if ($size -eq 256) { $bitmap.Save((Join-Path $ui 'matra-logo.png'), [Drawing.Imaging.ImageFormat]::Png) }
            } finally { $stream.Dispose() }
        } finally { $graphics.Dispose(); $bitmap.Dispose() }
    }
    $file = [IO.File]::Create((Join-Path $icons 'icon.ico'))
    $writer = New-Object IO.BinaryWriter($file)
    try {
        $writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$frames.Count)
        $offset = 6 + 16 * $frames.Count
        foreach ($frame in $frames) {
            $dimension = if ($frame.Size -eq 256) { 0 } else { $frame.Size }
            $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
            $writer.Write([byte]0); $writer.Write([byte]0)
            $writer.Write([uint16]1); $writer.Write([uint16]32)
            $writer.Write([uint32]$frame.Bytes.Length); $writer.Write([uint32]$offset)
            $offset += $frame.Bytes.Length
        }
        foreach ($frame in $frames) { $writer.Write([byte[]]$frame.Bytes) }
    } finally { $writer.Dispose(); $file.Dispose() }
    Write-Output 'Logo-18 packaged unchanged: master PNG, settings PNG, tray PNG and eight ICO sizes.'
} finally { $original.Dispose() }
