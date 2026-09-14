#Requires -Version 5.1

[CmdletBinding()]
param(
    [string]$PolicyPath = '',
    [string]$OutputDirectory = ''
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($PolicyPath)) {
    $PolicyPath = Join-Path $repositoryRoot 'cloudflare\public-media-policy.json'
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repositoryRoot 'artifacts\cloudflare\public-media'
}
$policyFile = [IO.Path]::GetFullPath($PolicyPath)
$outputRoot = [IO.Path]::GetFullPath($OutputDirectory)
$approvedRoot = [IO.Path]::GetFullPath((
    Join-Path $repositoryRoot (
        'assets\palbeacon\brand\generated\v2'
    )
))

if (-not (Test-Path -LiteralPath $policyFile -PathType Leaf)) {
    throw "Public media policy does not exist: $policyFile"
}
if (Test-Path -LiteralPath $outputRoot) {
    throw "Public media output already exists; refusing to overwrite: $outputRoot"
}

$policy = Get-Content -LiteralPath $policyFile -Raw -Encoding utf8 |
    ConvertFrom-Json
if ($policy.schema -ne 'pal-public-media-policy-v1') {
    throw 'Unsupported public media policy schema.'
}
if (
    [string]::IsNullOrWhiteSpace([string] $policy.bucket_name) -or
    [string] $policy.key_prefix -notmatch '^public/v[0-9]+$' -or
    [string] $policy.route_prefix -notmatch '^/media/v[0-9]+$'
) {
    throw 'Public media bucket or prefix configuration is invalid.'
}

$seenLogicalIds = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
$seenPublicNames = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::Ordinal
)
$entries = @()
foreach ($asset in @($policy.assets)) {
    $logicalId = [string] $asset.logical_id
    $relativeSource = [string] $asset.source_path
    $publicName = [string] $asset.public_name
    $contentType = [string] $asset.content_type
    $expectedSha256 = ([string] $asset.sha256).ToLowerInvariant()
    $distributionScope = [string] $asset.distribution_scope

    if (
        [string]::IsNullOrWhiteSpace($logicalId) -or
        -not $seenLogicalIds.Add($logicalId)
    ) {
        throw "Public media logical ID is empty or duplicated: $logicalId"
    }
    if (
        $publicName -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$' -or
        -not $seenPublicNames.Add($publicName)
    ) {
        throw "Public media file name is unsafe or duplicated: $publicName"
    }
    if ($distributionScope -ne 'public_web_approved') {
        throw "Public media is not approved for Web distribution: $logicalId"
    }
    if ($expectedSha256 -notmatch '^[a-f0-9]{64}$') {
        throw "Public media SHA-256 is invalid: $logicalId"
    }
    if ($contentType -notin @('image/png', 'image/webp')) {
        throw "Public media content type is not approved: $logicalId"
    }

    $sourcePath = [IO.Path]::GetFullPath((
        Join-Path $repositoryRoot $relativeSource
    ))
    if (
        -not $sourcePath.StartsWith(
            $approvedRoot + [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase
        )
    ) {
        throw "Public media source is outside the owned brand root: $logicalId"
    }
    if (
        $sourcePath.IndexOf(
            [IO.Path]::DirectorySeparatorChar + 'game' +
            [IO.Path]::DirectorySeparatorChar,
            [StringComparison]::OrdinalIgnoreCase
        ) -ge 0
    ) {
        throw "Game-derived assets cannot enter public media: $logicalId"
    }
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Public media source does not exist: $sourcePath"
    }
    $actualSha256 = (
        Get-FileHash -LiteralPath $sourcePath -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    if ($actualSha256 -ne $expectedSha256) {
        throw "Public media hash changed without policy review: $logicalId"
    }

    $objectKey = '{0}/{1}/{2}' -f (
        [string] $policy.key_prefix
    ), $actualSha256, $publicName
    $objectPath = Join-Path (
        Join-Path $outputRoot 'objects'
    ) $objectKey.Replace('/', '\')
    $objectParent = Split-Path -Parent $objectPath
    if (-not (Test-Path -LiteralPath $objectParent -PathType Container)) {
        New-Item -ItemType Directory -Path $objectParent -Force | Out-Null
    }
    Copy-Item -LiteralPath $sourcePath -Destination $objectPath

    $entries += [ordered]@{
        logical_id = $logicalId
        source_path = $relativeSource.Replace('\', '/')
        public_name = $publicName
        content_type = $contentType
        bytes = (Get-Item -LiteralPath $sourcePath).Length
        sha256 = $actualSha256
        distribution_scope = $distributionScope
        object_key = $objectKey
        request_path = '{0}/{1}/{2}' -f (
            [string] $policy.route_prefix
        ), $actualSha256, $publicName
        cache_control = 'public, max-age=31536000, immutable'
    }
}

if ($entries.Count -eq 0) {
    throw 'Public media policy did not select any assets.'
}

$manifest = [ordered]@{
    schema = 'pal-public-media-manifest-v1'
    policy_schema = [string] $policy.schema
    bucket_name = [string] $policy.bucket_name
    key_prefix = [string] $policy.key_prefix
    route_prefix = [string] $policy.route_prefix
    immutable = $true
    asset_count = $entries.Count
    total_bytes = (
        $entries |
            ForEach-Object { [int64] $_['bytes'] } |
            Measure-Object -Sum
    ).Sum
    assets = $entries
}
$manifestJson = $manifest | ConvertTo-Json -Depth 10
$manifestPath = Join-Path $outputRoot 'public-media-manifest.v1.json'
[IO.File]::WriteAllText(
    $manifestPath,
    $manifestJson,
    [Text.UTF8Encoding]::new($false)
)

Write-Output ([pscustomobject]@{
    output_directory = $outputRoot
    manifest = $manifestPath
    bucket_name = $manifest.bucket_name
    asset_count = $manifest.asset_count
    total_bytes = $manifest.total_bytes
})
