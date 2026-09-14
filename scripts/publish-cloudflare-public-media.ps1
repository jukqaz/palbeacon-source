#Requires -Version 5.1

[CmdletBinding()]
param(
    [string]$MediaDirectory = '',
    [string]$MisePath = ''
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($MediaDirectory)) {
    $MediaDirectory = Join-Path $repositoryRoot 'artifacts\cloudflare\public-media'
}
if ([string]::IsNullOrWhiteSpace($MisePath)) {
    $MisePath = Join-Path $repositoryRoot (
        '.tools\quality\mise\2026.7.16\mise\bin\mise.exe'
    )
}
$mediaRoot = [IO.Path]::GetFullPath($MediaDirectory)
$manifestPath = Join-Path $mediaRoot 'public-media-manifest.v1.json'
$mise = [IO.Path]::GetFullPath($MisePath)
$cloudflareRoot = Join-Path $repositoryRoot 'cloudflare'
$wrangler = Join-Path $cloudflareRoot 'node_modules\wrangler\bin\wrangler.js'

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Public media manifest does not exist: $manifestPath"
}
if (-not (Test-Path -LiteralPath $mise -PathType Leaf)) {
    throw "Managed mise runtime was not found: $mise"
}
if (-not (Test-Path -LiteralPath $wrangler -PathType Leaf)) {
    throw "Project-local Wrangler was not found: $wrangler"
}

$node = (
    & $mise exec node@lts -- node -p 'process.execPath'
).Trim()
if (
    $LASTEXITCODE -ne 0 -or
    -not (Test-Path -LiteralPath $node -PathType Leaf)
) {
    throw 'mise could not resolve the managed Node.js runtime.'
}

$manifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding utf8 |
    ConvertFrom-Json
if (
    $manifest.schema -ne 'pal-public-media-manifest-v1' -or
    $manifest.immutable -ne $true -or
    [string]::IsNullOrWhiteSpace([string] $manifest.bucket_name)
) {
    throw 'Public media manifest is invalid.'
}

$temporaryRoot = Join-Path (
    [IO.Path]::GetTempPath()
) ('pal-public-media-' + [guid]::NewGuid().ToString('N'))
[IO.Directory]::CreateDirectory($temporaryRoot) | Out-Null
$uploaded = 0
$unchanged = 0

Push-Location $cloudflareRoot
try {
    foreach ($asset in @($manifest.assets)) {
        if (
            $asset.distribution_scope -ne 'public_web_approved' -or
            [string] $asset.object_key -notmatch (
                '^public/v[0-9]+/[a-f0-9]{64}/' +
                '[A-Za-z0-9][A-Za-z0-9._-]{0,127}$'
            )
        ) {
            throw "Public media upload entry is not approved: $($asset.logical_id)"
        }
        $localPath = Join-Path (
            Join-Path $mediaRoot 'objects'
        ) ([string] $asset.object_key).Replace('/', '\')
        if (-not (Test-Path -LiteralPath $localPath -PathType Leaf)) {
            throw "Public media object is missing: $localPath"
        }
        $localSha256 = (
            Get-FileHash -LiteralPath $localPath -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        if ($localSha256 -ne [string] $asset.sha256) {
            throw "Public media object hash does not match: $($asset.logical_id)"
        }

        $remotePath = '{0}/{1}' -f (
            [string] $manifest.bucket_name
        ), ([string] $asset.object_key)
        $downloadPath = Join-Path $temporaryRoot (
            ([string] $asset.sha256) + '.remote'
        )
        $previousErrorPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        & $node $wrangler r2 object get `
            $remotePath --file $downloadPath --remote 2>$null | Out-Null
        $getExitCode = $LASTEXITCODE
        $ErrorActionPreference = $previousErrorPreference
        if (
            $getExitCode -eq 0 -and
            (Test-Path -LiteralPath $downloadPath -PathType Leaf) -and
            (
                Get-FileHash -LiteralPath $downloadPath -Algorithm SHA256
            ).Hash.ToLowerInvariant() -eq $localSha256
        ) {
            $unchanged++
            continue
        }

        $verifiedRemote = $false
        for ($uploadAttempt = 1; $uploadAttempt -le 3; $uploadAttempt++) {
            if ($uploadAttempt -gt 1) {
                $previousErrorPreference = $ErrorActionPreference
                $ErrorActionPreference = 'Continue'
                & $node $wrangler r2 object delete `
                    $remotePath --remote 2>$null | Out-Null
                $ErrorActionPreference = $previousErrorPreference
            }

            & $node $wrangler r2 object put `
                $remotePath `
                --file $localPath `
                --content-type ([string] $asset.content_type) `
                --remote `
                --experimental-provision=false `
                --experimental-auto-create=false
            if ($LASTEXITCODE -ne 0) {
                if ($uploadAttempt -eq 3) {
                    throw "R2 upload failed: $($asset.logical_id)"
                }
                continue
            }

            for ($verifyAttempt = 1; $verifyAttempt -le 3; $verifyAttempt++) {
                if ([IO.File]::Exists($downloadPath)) {
                    [IO.File]::Delete($downloadPath)
                }
                $previousErrorPreference = $ErrorActionPreference
                $ErrorActionPreference = 'Continue'
                & $node $wrangler r2 object get `
                    $remotePath --file $downloadPath --remote 2>$null | Out-Null
                $verifyExitCode = $LASTEXITCODE
                $ErrorActionPreference = $previousErrorPreference
                if (
                    $verifyExitCode -eq 0 -and
                    (Test-Path -LiteralPath $downloadPath -PathType Leaf) -and
                    (
                        Get-FileHash -LiteralPath $downloadPath -Algorithm SHA256
                    ).Hash.ToLowerInvariant() -eq $localSha256
                ) {
                    $verifiedRemote = $true
                    break
                }
                if ($verifyAttempt -lt 3) {
                    Start-Sleep -Seconds 1
                }
            }
            if ($verifiedRemote) {
                break
            }
        }
        if (-not $verifiedRemote) {
            throw "R2 uploaded object failed remote hash verification: $($asset.logical_id)"
        }
        $uploaded++
    }
}
finally {
    Pop-Location
    $resolvedTemporaryRoot = [IO.Path]::GetFullPath($temporaryRoot)
    $systemTemporaryRoot = [IO.Path]::GetFullPath([IO.Path]::GetTempPath())
    if (
        $resolvedTemporaryRoot.StartsWith(
            $systemTemporaryRoot,
            [StringComparison]::OrdinalIgnoreCase
        ) -and
        [IO.Directory]::Exists($resolvedTemporaryRoot)
    ) {
        [IO.Directory]::Delete($resolvedTemporaryRoot, $true)
    }
}

Write-Output ([pscustomobject]@{
    bucket_name = [string] $manifest.bucket_name
    uploaded = $uploaded
    unchanged = $unchanged
    total = @($manifest.assets).Count
})
