#Requires -Version 5.1

[CmdletBinding()]
param(
    [string]$AssetRoot = '',
    [Parameter(Mandatory)]
    [string]$PublicOutput
)

$ErrorActionPreference = 'Stop'
$repositoryRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($AssetRoot)) {
    $AssetRoot = Join-Path $repositoryRoot 'assets\palbeacon'
}
$sourceRoot = [IO.Path]::GetFullPath($AssetRoot)
$destinationRoot = [IO.Path]::GetFullPath($PublicOutput)
$sourceCatalog = Join-Path $sourceRoot 'game\catalog'
$destinationCatalog = Join-Path (
    $destinationRoot
) 'assets\public\catalog'

if (-not (Test-Path -LiteralPath $sourceCatalog -PathType Container)) {
    throw "Canonical PalBeacon catalog does not exist: $sourceCatalog"
}

$catalogFiles = @(
    'pals.json',
    'active_skills.json',
    'passive_skills.json',
    'items.v1.json',
    'world.v1.json',
    'humans.v1.json',
    'pals_breeding.v1.json',
    'l10n\ko\pals.json',
    'l10n\ko\active_skills.json',
    'l10n\ko\passive_skills.json'
)

$excludedFields = [Collections.Generic.HashSet[string]]::new(
    [StringComparer]::OrdinalIgnoreCase
)
@(
    'description',
    'description_ko',
    'description_en',
    'sources',
    'package_path',
    'icon_package_path',
    'icon_source_path',
    'icon'
) | ForEach-Object {
    [void]$excludedFields.Add($_)
}

function Remove-PrivateCatalogFields {
    param([AllowNull()]$Value)

    if ($null -eq $Value) {
        return
    }
    if ($Value -is [pscustomobject]) {
        foreach ($property in @($Value.PSObject.Properties)) {
            if ($excludedFields.Contains($property.Name)) {
                $Value.PSObject.Properties.Remove($property.Name)
                continue
            }
            Remove-PrivateCatalogFields -Value $property.Value
        }
        return
    }
    if (
        $Value -is [Collections.IEnumerable] -and
        $Value -isnot [string]
    ) {
        foreach ($entry in $Value) {
            Remove-PrivateCatalogFields -Value $entry
        }
    }
}

function Write-Utf8Json {
    param(
        [Parameter(Mandatory)]
        [string]$Path,
        [Parameter(Mandatory)]
        $Value
    )

    $parent = Split-Path -Parent $Path
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $json = $Value | ConvertTo-Json -Depth 100 -Compress
    [IO.File]::WriteAllText(
        $Path,
        $json,
        [Text.UTF8Encoding]::new($false)
    )
}

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory)]
        [string]$Path
    )

    $stream = [IO.File]::OpenRead($Path)
    try {
        $sha256 = [Security.Cryptography.SHA256]::Create()
        try {
            $bytes = $sha256.ComputeHash($stream)
            return -join ($bytes | ForEach-Object { $_.ToString('x2') })
        }
        finally {
            $sha256.Dispose()
        }
    }
    finally {
        $stream.Dispose()
    }
}

$documents = @{}
foreach ($relativePath in $catalogFiles) {
    $sourcePath = Join-Path $sourceCatalog $relativePath
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "Required catalog input is missing: $sourcePath"
    }
    $document = Get-Content -LiteralPath $sourcePath -Raw -Encoding utf8 |
        ConvertFrom-Json
    Remove-PrivateCatalogFields -Value $document
    $targetPath = Join-Path $destinationCatalog $relativePath
    Write-Utf8Json -Path $targetPath -Value $document
    $documents[$relativePath] = $document
}

$breeding = $documents['pals_breeding.v1.json']
$items = $documents['items.v1.json']
$world = $documents['world.v1.json']
$humans = $documents['humans.v1.json']
$counts = [ordered]@{
    pals = @($breeding.pals).Count
    breeding_species = @($breeding.breeding_species).Count
    special_breeding = @($breeding.paldex_special_breeding).Count
    active_skills = @(
        $documents['active_skills.json'].PSObject.Properties
    ).Count
    passive_skills = @(
        $documents['passive_skills.json'].PSObject.Properties
    ).Count
    items = @($items.items).Count
    recipes = @($items.recipes).Count
    pal_drops = @($items.pal_drops).Count
    buildings = @($world.buildings).Count
    technologies = @($world.technologies).Count
    shop_groups = @($world.shop_groups).Count
    humans = @($humans.humans).Count
}
foreach ($requiredCount in @(
    'pals',
    'breeding_species',
    'active_skills',
    'passive_skills',
    'items',
    'buildings',
    'technologies',
    'humans'
)) {
    if ($counts[$requiredCount] -le 0) {
        throw "Public catalog projection is empty: $requiredCount"
    }
}

$fileEntries = foreach ($relativePath in $catalogFiles) {
    $path = Join-Path $destinationCatalog $relativePath
    [ordered]@{
        path = $relativePath.Replace('\', '/')
        bytes = (Get-Item -LiteralPath $path).Length
        sha256 = Get-Sha256Hex -Path $path
    }
}
$manifest = [ordered]@{
    schema = 'pal-public-web-catalog-v1'
    policy = 'public-metadata-policy-v1'
    dataset_version = 'public-web-catalog-{0}-v1' -f (
        ([string]$breeding.game_build_id).Replace(':', '-')
    )
    game_build_id = $breeding.game_build_id
    verified = $breeding.verified -eq $true
    included_content = @(
        'exact_korean_names',
        'structural_numeric_fields',
        'typed_relations',
        'breeding_rules'
    )
    excluded_content = @(
        'game_artwork',
        'localized_descriptions',
        'package_paths',
        'personal_data'
    )
    counts = $counts
    files = @($fileEntries)
}
$manifestPath = Join-Path $destinationCatalog 'public-web-manifest.v1.json'
Write-Utf8Json -Path $manifestPath -Value $manifest

Write-Output ([pscustomobject]@{
    projected_files = $catalogFiles.Count + 1
    game_build_id = $manifest.game_build_id
    verified = $manifest.verified
    counts = $counts
    manifest = $manifestPath
})
