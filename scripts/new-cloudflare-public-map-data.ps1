#Requires -Version 5.1

[CmdletBinding()]
param(
    [string]$AssetRoot = '',

    [Parameter(Mandatory)]
    [string]$PublicOutput,

    [string]$ContractManifest = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($AssetRoot)) {
    $AssetRoot = Join-Path $repositoryRoot 'assets\palbeacon'
}
$sourceRoot = [IO.Path]::GetFullPath($AssetRoot)
$destinationRoot = [IO.Path]::GetFullPath($PublicOutput)
if ([string]::IsNullOrWhiteSpace($ContractManifest)) {
    $ContractManifest = Join-Path $PSScriptRoot `
        '..\contracts\map\v1\manifest.json'
}
$contractPath = [IO.Path]::GetFullPath($ContractManifest)
$sourceMapRoot = Join-Path $sourceRoot 'game\map'
if (-not (Test-Path -LiteralPath $sourceMapRoot -PathType Container)) {
    throw "Canonical PalBeacon map data does not exist: $sourceMapRoot"
}
if (-not (Test-Path -LiteralPath $contractPath -PathType Leaf)) {
    throw "Generated map contract manifest does not exist: $contractPath"
}

$contract = Get-Content -LiteralPath $contractPath -Raw -Encoding utf8 |
    ConvertFrom-Json
if (
    $contract.schema_version -ne 1 -or
    [string]$contract.language -ne 'ko' -or
    [string]$contract.contract_sha256 -notmatch '^[a-f0-9]{64}$'
) {
    throw 'Generated map contract manifest is invalid.'
}

$sources = @(
    [ordered]@{
        source = 'pois.v1.json'
        public_name = 'pois.v1.json'
        expected_sha256 = [string]$contract.poi_sha256
    },
    [ordered]@{
        source = 'poi-terminology.ko.v1.json'
        public_name = 'poi-terminology.ko.v1.json'
        expected_sha256 = [string]$contract.taxonomy_sha256
    },
    [ordered]@{
        source = 'search-index.ko.v1.json'
        public_name = 'search-index.ko.v1.json'
        expected_sha256 = ''
    }
)

$contractHash = [string]$contract.contract_sha256
$objectRoot = Join-Path $destinationRoot "data\map\v1\$contractHash"
if (Test-Path -LiteralPath $objectRoot) {
    throw "Content-addressed map data already exists: $objectRoot"
}
[void](New-Item -ItemType Directory -Path $objectRoot -Force)

$files = @()
foreach ($entry in $sources) {
    $source = Join-Path $sourceMapRoot ([string]$entry.source)
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Required map data input is missing: $source"
    }
    $actualHash = (Get-FileHash -LiteralPath $source -Algorithm SHA256).
        Hash.ToLowerInvariant()
    if (
        -not [string]::IsNullOrWhiteSpace([string]$entry.expected_sha256) -and
        $actualHash -ne [string]$entry.expected_sha256
    ) {
        throw "Map contract source hash mismatch: $($entry.source)"
    }
    if ([string]$entry.source -eq 'search-index.ko.v1.json') {
        $searchIndex = Get-Content -LiteralPath $source -Raw -Encoding utf8 |
            ConvertFrom-Json
        if ([string]$searchIndex.contract_sha256 -ne $contractHash) {
            throw 'Map search index references a different contract.'
        }
    }
    $destination = Join-Path $objectRoot ([string]$entry.public_name)
    Copy-Item -LiteralPath $source -Destination $destination
    $files += [ordered]@{
        name = [string]$entry.public_name
        request_path = "/data/map/v1/$contractHash/$($entry.public_name)"
        content_type = 'application/json; charset=utf-8'
        bytes = (Get-Item -LiteralPath $destination).Length
        sha256 = $actualHash
        cache_control = 'public, max-age=31536000, immutable'
    }
}

$manifest = [ordered]@{
    schema = 'pal-map-public-data-v1'
    game_build_id = [string]$contract.game_build_id
    language = 'ko'
    contract_sha256 = $contractHash
    delivery_compression = 'cloudflare-auto-brotli-gzip'
    immutable_objects = $true
    files = $files
}
$manifestRoot = Join-Path $destinationRoot 'data\map\v1'
$manifestPath = Join-Path $manifestRoot 'manifest.json'
$json = ($manifest | ConvertTo-Json -Depth 8 -Compress) + "`n"
[IO.File]::WriteAllText(
    $manifestPath,
    $json,
    [Text.UTF8Encoding]::new($false)
)

Write-Output ([pscustomobject]@{
    manifest = $manifestPath
    contract_sha256 = $contractHash
    file_count = $files.Count
    total_bytes = (
        $files |
            ForEach-Object { [int64]$_['bytes'] } |
            Measure-Object -Sum
    ).Sum
})
