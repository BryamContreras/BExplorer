param(
    [string]$Source = "assets/icons/appicon-source.png",
    [double]$ContentScale = 0.92
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$sourcePath = Join-Path $root $Source
if (!(Test-Path -LiteralPath $sourcePath)) {
    $sourcePath = Join-Path $root "assets/icons/appicon.png"
}
if (!(Test-Path -LiteralPath $sourcePath)) {
    throw "No icon source found."
}

$assetDir = Join-Path $root "assets/icons"
$windowsDir = Join-Path $root "assets/windows"
$linuxRoot = Join-Path $root "assets/linux/hicolor"
New-Item -ItemType Directory -Force -Path $assetDir, $windowsDir | Out-Null

Add-Type -AssemblyName System.Drawing

function New-ScaledBitmap {
    param(
        [System.Drawing.Image]$SourceImage,
        [System.Drawing.Rectangle]$SourceRect,
        [int]$Size,
        [double]$Fill
    )

    $bitmap = New-Object System.Drawing.Bitmap $Size, $Size, ([System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
        $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
        $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
        $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
        $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
        $graphics.Clear([System.Drawing.Color]::Transparent)

        $target = [Math]::Max(1, [Math]::Round($Size * $Fill))
        $scale = $target / [Math]::Max($SourceRect.Width, $SourceRect.Height)
        $destW = [Math]::Round($SourceRect.Width * $scale)
        $destH = [Math]::Round($SourceRect.Height * $scale)
        $destX = [Math]::Floor(($Size - $destW) / 2)
        $destY = [Math]::Floor(($Size - $destH) / 2)
        $destRect = New-Object System.Drawing.Rectangle $destX, $destY, $destW, $destH
        $graphics.DrawImage($SourceImage, $destRect, $SourceRect, [System.Drawing.GraphicsUnit]::Pixel)
    } finally {
        $graphics.Dispose()
    }

    $bitmap
}

function Get-AlphaBounds {
    param([System.Drawing.Bitmap]$Bitmap)

    $minX = $Bitmap.Width
    $minY = $Bitmap.Height
    $maxX = -1
    $maxY = -1
    for ($y = 0; $y -lt $Bitmap.Height; $y++) {
        for ($x = 0; $x -lt $Bitmap.Width; $x++) {
            if ($Bitmap.GetPixel($x, $y).A -gt 8) {
                if ($x -lt $minX) { $minX = $x }
                if ($y -lt $minY) { $minY = $y }
                if ($x -gt $maxX) { $maxX = $x }
                if ($y -gt $maxY) { $maxY = $y }
            }
        }
    }

    if ($maxX -lt $minX -or $maxY -lt $minY) {
        return New-Object System.Drawing.Rectangle 0, 0, $Bitmap.Width, $Bitmap.Height
    }
    New-Object System.Drawing.Rectangle $minX, $minY, ($maxX - $minX + 1), ($maxY - $minY + 1)
}

$sourceImage = [System.Drawing.Bitmap]::FromFile($sourcePath)
try {
    $bounds = Get-AlphaBounds $sourceImage

    $normalized = New-ScaledBitmap $sourceImage $bounds 1024 $ContentScale
    try {
        $normalized.Save((Join-Path $assetDir "appicon.png"), [System.Drawing.Imaging.ImageFormat]::Png)
    } finally {
        $normalized.Dispose()
    }

    $sizes = @(16, 24, 32, 48, 64, 128, 256, 512)
    $icoBuffers = @{}
    foreach ($size in $sizes) {
        $bitmap = New-ScaledBitmap $sourceImage $bounds $size $ContentScale
        try {
            $linuxDir = Join-Path $linuxRoot "${size}x${size}/apps"
            New-Item -ItemType Directory -Force -Path $linuxDir | Out-Null
            $bitmap.Save((Join-Path $linuxDir "bexplorer.png"), [System.Drawing.Imaging.ImageFormat]::Png)

            if ($size -le 256) {
                $stream = New-Object System.IO.MemoryStream
                try {
                    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
                    $icoBuffers[$size] = $stream.ToArray()
                } finally {
                    $stream.Dispose()
                }
            }
        } finally {
            $bitmap.Dispose()
        }
    }

    $icoSizes = @(16, 24, 32, 48, 64, 128, 256)
    $icoPath = Join-Path $windowsDir "bexplorer.ico"
    $file = [System.IO.File]::Open($icoPath, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write)
    try {
        $writer = New-Object System.IO.BinaryWriter $file
        try {
            $writer.Write([UInt16]0)
            $writer.Write([UInt16]1)
            $writer.Write([UInt16]$icoSizes.Count)
            $offset = 6 + (16 * $icoSizes.Count)
            foreach ($size in $icoSizes) {
                $bytes = [byte[]]$icoBuffers[$size]
                if ($size -eq 256) { $dim = [byte]0 } else { $dim = [byte]$size }
                $writer.Write($dim)
                $writer.Write($dim)
                $writer.Write([byte]0)
                $writer.Write([byte]0)
                $writer.Write([UInt16]1)
                $writer.Write([UInt16]32)
                $writer.Write([UInt32]$bytes.Length)
                $writer.Write([UInt32]$offset)
                $offset += $bytes.Length
            }
            foreach ($size in $icoSizes) {
                $writer.Write([byte[]]$icoBuffers[$size])
            }
        } finally {
            $writer.Dispose()
        }
    } finally {
        $file.Dispose()
    }

    Write-Output ("Source bounds: x={0} y={1} w={2} h={3}" -f $bounds.X, $bounds.Y, $bounds.Width, $bounds.Height)
    Write-Output ("Generated app icons with {0:P0} content fill." -f $ContentScale)
} finally {
    $sourceImage.Dispose()
}
