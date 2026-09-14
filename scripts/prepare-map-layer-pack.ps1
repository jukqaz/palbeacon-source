param(
    [string]$OutputPath = "",
    [string]$GameBuildId = "24575825",
    [string]$SourceVersion = "v1.0.0",
    [string]$PalDbMapDataUrl = "https://paldb.cc/js/map_data_ko.js"
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $PSScriptRoot "..\assets\palbeacon\game\map\layers.v1.json"
}

$sourceRoot = "https://assets.palmods.gg/$SourceVersion/map"
$excludedCategories = [System.Collections.Generic.HashSet[string]]::new(
    [string[]]@("fast-travel", "dungeon", "bounty"),
    [System.StringComparer]::Ordinal
)

function New-Layer(
    [string]$Id,
    [string]$Label,
    [string]$Group,
    [bool]$DefaultEnabled,
    [double]$MinimumScale,
    [bool]$Dense
) {
    [pscustomobject][ordered]@{
        id = $Id
        label = $Label
        group = $Group
        default_enabled = $DefaultEnabled
        minimum_scale = $MinimumScale
        dense = $Dense
    }
}

$layerDefinitions = @(
    New-Layer "respawn" "Respawn point" "navigation" $false 0.20 $false
    New-Layer "tower" "Tower" "navigation" $true 0.20 $false
    New-Layer "sealed-realm" "Sealed Realm" "navigation" $false 0.20 $false
    New-Layer "biome-boss" "Boss lair" "navigation" $false 0.20 $false
    New-Layer "oil-rig" "Oil rig" "navigation" $true 0.20 $false
    New-Layer "map-unlock" "Map unlock" "navigation" $true 0.20 $false
    New-Layer "sky-warp" "Sky warp altar" "navigation" $false 0.20 $false
    New-Layer "dimensional-warp" "Dimensional warp" "navigation" $false 0.20 $false
    New-Layer "dimensional-distortion" "Dimensional distortion" "navigation" $false 0.20 $false
    New-Layer "enemy-camp" "Enemy camp" "people" $false 0.20 $false
    New-Layer "captured-pal" "Possible captured Pal cage" "collectible" $false 0.20 $false
    New-Layer "anti-air-turret" "Anti-air missile" "people" $false 0.20 $false
    New-Layer "pal-merchant" "Pal merchant" "people" $false 0.20 $false
    New-Layer "merchant" "Merchant" "people" $false 0.20 $false
    New-Layer "npc" "NPC" "people" $false 0.45 $true
    New-Layer "messenger-of-love" "Possible Messenger of Love encounter" "people" $false 0.20 $false
    New-Layer "effigy" "Effigy" "collectible" $false 0.20 $true
    New-Layer "medal" "Medal" "collectible" $false 0.45 $true
    New-Layer "memo" "Memo" "collectible" $false 0.20 $false
    New-Layer "egg" "Pal egg" "collectible" $false 0.75 $true
    New-Layer "skill-fruit" "Skill fruit tree" "collectible" $false 0.20 $false
    New-Layer "world-tree-fruit" "World Tree fruit" "collectible" $false 0.20 $false
    New-Layer "ancient-shrine" "Ancient Shrine" "collectible" $false 0.20 $true
    New-Layer "treasure-map" "Treasure map point" "collectible" $false 0.20 $false
    New-Layer "elemental-chest" "Elemental chest" "collectible" $false 0.55 $true
    New-Layer "chest" "Treasure chest" "collectible" $false 1.00 $true
    New-Layer "poi" "Statue of Power" "collectible" $false 0.20 $false
    New-Layer "kinship-peach" "Kinship Peach" "collectible" $false 0.20 $false
    New-Layer "fishing-spot" "Fishing spot" "resource" $false 0.55 $true
    New-Layer "salvage-rank-1" "Salvage Rank 1" "resource" $false 0.20 $true
    New-Layer "salvage-rank-2" "Salvage Rank 2" "resource" $false 0.20 $true
    New-Layer "oil-field" "Crude oil" "resource" $false 0.45 $true
    New-Layer "soralite" "Soralite" "resource" $false 0.55 $true
    New-Layer "chromite" "Chromite" "resource" $false 0.20 $true
    New-Layer "hexolite-quartz" "Hexolite Quartz" "resource" $false 0.20 $true
    New-Layer "ancient-bark" "Ancient Bark" "resource" $false 0.20 $false
    New-Layer "ancient-lava" "Ancient Lava" "resource" $false 0.20 $false
    New-Layer "ore-copper" "Ore" "resource" $false 0.75 $true
    New-Layer "ore-coal" "Coal" "resource" $false 0.75 $true
    New-Layer "ore-quartz" "Pure Quartz" "resource" $false 0.75 $true
    New-Layer "ore-sulfur" "Sulfur" "resource" $false 0.75 $true
    New-Layer "ore-crystal" "Paldium" "resource" $false 0.75 $true
    New-Layer "healing-spring" "Healing Spring" "resource" $false 0.20 $false
    New-Layer "paloxite" "Paloxite" "resource" $false 0.20 $true
)

$definitionsById = @{}
for ($index = 0; $index -lt $layerDefinitions.Count; $index++) {
    $definition = $layerDefinitions[$index]
    if ($definitionsById.ContainsKey($definition.id)) {
        throw "Duplicate layer definition: $($definition.id)"
    }
    $definitionsById[$definition.id] = [pscustomobject]@{
        index = $index
        definition = $definition
    }
}

$regions = @(
    [pscustomobject]@{
        source_id = "main"
        source_url = $sourceRoot
        map_id = "MainMap"
        region_id = "FirstRegion"
        world_min_x = -1099400.0
        world_min_y = -724400.0
        world_max_x = 349400.0
        world_max_y = 724400.0
    },
    [pscustomobject]@{
        source_id = "tree"
        source_url = "$sourceRoot/tree"
        map_id = "Tree"
        region_id = "DummyRegion"
        world_min_x = 347351.5
        world_min_y = -818197.0
        world_max_x = 689148.5
        world_max_y = -476400.0
    }
)

$regionDocuments = @()
$sourceCountByLayer = @{}
$outsideRegionCount = 0
$duplicatePointCount = 0
foreach ($definition in $layerDefinitions) {
    $sourceCountByLayer[$definition.id] = 0
}

foreach ($region in $regions) {
    $indexUrl = "$($region.source_url)/points-index.json"
    $indexRows = Invoke-RestMethod -Uri $indexUrl
    $indexRows = @($indexRows)
    if ($indexRows.Count -eq 0) {
        throw "Layer index is empty: $indexUrl"
    }

    $points = [System.Collections.Generic.List[object]]::new()
    $seenPointKeys = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($indexRow in $indexRows) {
        $category = [string]$indexRow.category
        if ($excludedCategories.Contains($category)) {
            continue
        }
        if (-not $definitionsById.ContainsKey($category)) {
            throw "Unmapped source layer '$category' in $indexUrl"
        }

        $sourcePoints = Invoke-RestMethod -Uri "$($region.source_url)/points/$category.json"
        $sourcePoints = @($sourcePoints)
        if ($sourcePoints.Count -ne [int]$indexRow.count) {
            throw "Layer count mismatch for $category in $($region.source_id)"
        }

        $definitionIndex = [int]$definitionsById[$category].index
        $includedSourcePointCount = 0
        foreach ($sourcePoint in $sourcePoints) {
            if ([string]$sourcePoint.category -ne $category) {
                throw "Layer category mismatch for $category"
            }
            $worldX = [double]$sourcePoint.location.x
            $worldY = [double]$sourcePoint.location.y
            if ([double]::IsNaN($worldX) -or [double]::IsInfinity($worldX) -or
                [double]::IsNaN($worldY) -or [double]::IsInfinity($worldY)) {
                throw "Layer contains a non-finite coordinate: $category"
            }
            if ($worldX -lt $region.world_min_x -or
                $worldX -gt $region.world_max_x -or
                $worldY -lt $region.world_min_y -or
                $worldY -gt $region.world_max_y) {
                $outsideRegionCount++
                continue
            }

            $roundedWorldX = [math]::Round($worldX, 3)
            $roundedWorldY = [math]::Round($worldY, 3)
            $dedupeKey = "$category|$roundedWorldX|$roundedWorldY"
            if (-not $seenPointKeys.Add($dedupeKey)) {
                $duplicatePointCount++
                continue
            }

            $mapX = (($worldY - $region.world_min_y) /
                ($region.world_max_y - $region.world_min_y)) * 2048.0
            $mapY = (($region.world_max_x - $worldX) /
                ($region.world_max_x - $region.world_min_x)) * 2048.0
            $points.Add(@(
                $definitionIndex,
                $roundedWorldX,
                $roundedWorldY,
                [math]::Round($mapX, 4),
                [math]::Round($mapY, 4)
            ))
            $includedSourcePointCount++
        }
        $sourceCountByLayer[$category] += $includedSourcePointCount
    }

    $regionDocuments += [pscustomobject][ordered]@{
        map_id = $region.map_id
        region_id = $region.region_id
        point_count = $points.Count
        points = $points
    }
}

$palDbLayerDefinitions = @(
    [pscustomobject]@{
        id = "chromite"
        source_type = "Chromite"
        expected_count = 257
    },
    [pscustomobject]@{
        id = "hexolite-quartz"
        source_type = "Hexolite Quartz"
        expected_count = 349
    },
    [pscustomobject]@{
        id = "salvage-rank-1"
        source_type = "Salvage Rank1"
        expected_count = 776
    },
    [pscustomobject]@{
        id = "salvage-rank-2"
        source_type = "Salvage Rank2"
        expected_count = 1987
    }
)

$palDbMapData = [string](Invoke-WebRequest -UseBasicParsing -Uri $PalDbMapDataUrl).Content
$fixedDungeonMatch = [regex]::Match(
    $palDbMapData,
    "(?s)var fixedDungeon = (?<data>.*?);var regionData"
)
if (-not $fixedDungeonMatch.Success) {
    throw "PalDB fixedDungeon payload was not found: $PalDbMapDataUrl"
}

$palDbFixedDungeonData = $fixedDungeonMatch.Groups["data"].Value
$mainRegion = @(
    $regions |
        Where-Object {
            $_.map_id -eq "MainMap" -and $_.region_id -eq "FirstRegion"
        }
) | Select-Object -First 1
$mainRegionDocument = @(
    $regionDocuments |
        Where-Object {
            $_.map_id -eq "MainMap" -and $_.region_id -eq "FirstRegion"
        }
) | Select-Object -First 1
if ($null -eq $mainRegion -or $null -eq $mainRegionDocument) {
    throw "MainMap/FirstRegion is required for PalDB supplemental layers."
}

$palDbSeenPointKeys = [System.Collections.Generic.HashSet[string]]::new(
    [System.StringComparer]::Ordinal
)
foreach ($palDbLayer in $palDbLayerDefinitions) {
    $sourceType = [regex]::Escape([string]$palDbLayer.source_type)
    $rowPattern = '\{"pos":\{"X":(?<x>-?[0-9.]+),"Y":(?<y>-?[0-9.]+),' +
        '"Z":(?<z>-?[0-9.]+)\},[^{}]*"type":"' + $sourceType +
        '"[^{}]*\}'
    $sourceRows = [regex]::Matches($palDbFixedDungeonData, $rowPattern)
    if ($sourceRows.Count -ne [int]$palDbLayer.expected_count) {
        throw "PalDB layer count mismatch for $($palDbLayer.id): " +
            "$($sourceRows.Count) / $($palDbLayer.expected_count)"
    }

    $definitionIndex = [int]$definitionsById[$palDbLayer.id].index
    $includedSourcePointCount = 0
    foreach ($sourceRow in $sourceRows) {
        $worldX = [double]$sourceRow.Groups["x"].Value
        $worldY = [double]$sourceRow.Groups["y"].Value
        if ($worldX -lt $mainRegion.world_min_x -or
            $worldX -gt $mainRegion.world_max_x -or
            $worldY -lt $mainRegion.world_min_y -or
            $worldY -gt $mainRegion.world_max_y) {
            throw "PalDB point is outside MainMap bounds: " +
                "$($palDbLayer.id) / $worldX / $worldY"
        }

        $roundedWorldX = [math]::Round($worldX, 3)
        $roundedWorldY = [math]::Round($worldY, 3)
        $dedupeKey = "$($palDbLayer.id)|$roundedWorldX|$roundedWorldY"
        if (-not $palDbSeenPointKeys.Add($dedupeKey)) {
            $duplicatePointCount++
            continue
        }

        $mapX = (($worldY - $mainRegion.world_min_y) /
            ($mainRegion.world_max_y - $mainRegion.world_min_y)) * 2048.0
        $mapY = (($mainRegion.world_max_x - $worldX) /
            ($mainRegion.world_max_x - $mainRegion.world_min_x)) * 2048.0
        $mainRegionDocument.points.Add(@(
            $definitionIndex,
            $roundedWorldX,
            $roundedWorldY,
            [math]::Round($mapX, 4),
            [math]::Round($mapY, 4)
        ))
        $includedSourcePointCount++
    }

    if ($includedSourcePointCount -ne [int]$palDbLayer.expected_count) {
        throw "PalDB layer contains duplicate coordinates for $($palDbLayer.id)."
    }
    $sourceCountByLayer[$palDbLayer.id] += $includedSourcePointCount
}
$mainRegionDocument.point_count = $mainRegionDocument.points.Count

$layers = for ($index = 0; $index -lt $layerDefinitions.Count; $index++) {
    $definition = $layerDefinitions[$index]
    [pscustomobject][ordered]@{
        index = $index
        id = $definition.id
        label = $definition.label
        group = $definition.group
        default_enabled = $definition.default_enabled
        minimum_scale = $definition.minimum_scale
        dense = $definition.dense
        point_count = [int]$sourceCountByLayer[$definition.id]
    }
}

# Upstream currently mutates content under the same semantic version path.
# Bind the shipped dataset identity to the normalized projected content so a
# same-version refresh cannot masquerade as the previous file.
$contentIdentity = [ordered]@{
    source_version = $SourceVersion
    layers = @($layers)
    regions = @($regionDocuments)
}
$contentIdentityBytes = [System.Text.UTF8Encoding]::new($false).GetBytes(
    ($contentIdentity | ConvertTo-Json -Depth 8 -Compress)
)
$sha256 = [System.Security.Cryptography.SHA256]::Create()
try {
    $sourceContentSha256 = (
        [System.BitConverter]::ToString(
            $sha256.ComputeHash($contentIdentityBytes)
        ).Replace('-', '').ToLowerInvariant()
    )
} finally {
    $sha256.Dispose()
}
$datasetFingerprint = $sourceContentSha256.Substring(0, 12)

$document = [ordered]@{
    schema_version = 1
    game_build_id = $GameBuildId
    dataset_version = "map-layers-$SourceVersion-build-$GameBuildId-$datasetFingerprint"
    coordinate_contract = "pal-companion-map-region-projection-v1"
    provenance = [ordered]@{
        source_name = "PalMods + PalDB map data"
        source_page = "https://www.palmods.gg/tools/map"
        source_assets = $sourceRoot
        paldb_source_page = "https://paldb.cc/ko/Map"
        paldb_map_data = $PalDbMapDataUrl
        paldb_layers = @($palDbLayerDefinitions)
        source_version = $SourceVersion
        source_content_sha256 = $sourceContentSha256
        exact_local_build_verified = $false
        outside_region_points_excluded = $outsideRegionCount
        duplicate_points_removed = $duplicatePointCount
        deduplication_key = "map_id + region_id + source_layer_id + rounded_world_x_3 + rounded_world_y_3"
        usage = "supplemental factual coordinates; verified local core POIs remain authoritative"
    }
    layers = @($layers)
    regions = @($regionDocuments)
}

$parent = Split-Path -Parent $OutputPath
New-Item -ItemType Directory -Force -Path $parent | Out-Null
[System.IO.File]::WriteAllText(
    [System.IO.Path]::GetFullPath($OutputPath),
    ($document | ConvertTo-Json -Depth 8 -Compress),
    [System.Text.UTF8Encoding]::new($false)
)

$total = ($regionDocuments | Measure-Object point_count -Sum).Sum
Write-Host "Prepared $total supplemental map points across $($layers.Count) layers."
$regionDocuments | ForEach-Object {
    Write-Host "  $($_.map_id)/$($_.region_id): $($_.point_count)"
}
Write-Host "Output: $([System.IO.Path]::GetFullPath($OutputPath))"
