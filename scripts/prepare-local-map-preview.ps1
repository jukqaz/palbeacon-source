[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string]$InputJpeg,

    [Parameter(Mandatory = $true)]
    [string]$OutputBmp,

    [ValidateRange(512, 4096)]
    [int]$Size = 2048,

    [string]$ExpectedSourceSha256 = "32FDD36DD39C8E57C38FCC0B138A0315C4F350EDE5579A467BACFC0A4FDB4350"
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$sourcePath = (Resolve-Path -LiteralPath $InputJpeg).Path
$sourceHash = (Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256).Hash
if ($ExpectedSourceSha256 -and $sourceHash -ne $ExpectedSourceSha256) {
    throw "Map source hash mismatch. Expected $ExpectedSourceSha256 but found $sourceHash."
}

$outputPath = [System.IO.Path]::GetFullPath($OutputBmp)
$outputDirectory = [System.IO.Path]::GetDirectoryName($outputPath)
if (-not [string]::IsNullOrWhiteSpace($outputDirectory)) {
    [System.IO.Directory]::CreateDirectory($outputDirectory) | Out-Null
}

Add-Type -AssemblyName System.Drawing
$source = [System.Drawing.Image]::FromFile($sourcePath)
try {
    if ($source.Width -ne $source.Height) {
        throw "Expected a square map image, found $($source.Width)x$($source.Height)."
    }

    $destination = [System.Drawing.Bitmap]::new(
        $Size,
        $Size,
        [System.Drawing.Imaging.PixelFormat]::Format24bppRgb
    )
    try {
        $graphics = [System.Drawing.Graphics]::FromImage($destination)
        try {
            $graphics.CompositingMode = [System.Drawing.Drawing2D.CompositingMode]::SourceCopy
            $graphics.CompositingQuality = [System.Drawing.Drawing2D.CompositingQuality]::HighQuality
            $graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
            $graphics.PixelOffsetMode = [System.Drawing.Drawing2D.PixelOffsetMode]::HighQuality
            $graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::HighQuality
            $graphics.DrawImage($source, 0, 0, $Size, $Size)
        }
        finally {
            $graphics.Dispose()
        }
        $destination.Save($outputPath, [System.Drawing.Imaging.ImageFormat]::Bmp)
    }
    finally {
        $destination.Dispose()
    }
}
finally {
    $source.Dispose()
}

$outputHash = (Get-FileHash -LiteralPath $outputPath -Algorithm SHA256).Hash
[pscustomobject]@{
    input = $sourcePath
    input_sha256 = $sourceHash
    output = $outputPath
    output_sha256 = $outputHash
    width = $Size
    height = $Size
    format = "BMP/24bpp/BGR"
    usage = "local-development-preview-only"
} | ConvertTo-Json -Depth 3
