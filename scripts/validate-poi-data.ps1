[CmdletBinding()]
param(
    [string]$ProjectRoot,
    [string]$GameBuildId = '24575825'
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($ProjectRoot)) {
    $ProjectRoot = Split-Path -Parent $PSScriptRoot
}

function Add-Failure {
    param(
        [System.Collections.Generic.List[string]]$Failures,
        [string]$Message
    )

    $Failures.Add($Message)
}

function Read-JsonObject {
    param([string]$Path)

    if (-not (Test-Path -LiteralPath $Path)) {
        throw "Required data file is missing: $Path"
    }

    return Get-Content -Raw -Encoding UTF8 -LiteralPath $Path |
        ConvertFrom-Json
}

$mapRoot = Join-Path $ProjectRoot 'assets\palbeacon\game\map'
$terminologyPath = Join-Path $mapRoot 'poi-terminology.ko.v1.json'
$corePoiPath = Join-Path $mapRoot 'pois.v1.json'
$supplementalLayerPath = Join-Path $mapRoot 'layers.v1.json'

$terminology = Read-JsonObject $terminologyPath
$core = Read-JsonObject $corePoiPath
$supplemental = Read-JsonObject $supplementalLayerPath
$failures = [System.Collections.Generic.List[string]]::new()

if ($terminology.schema_version -ne 1) {
    Add-Failure $failures 'Terminology schema_version must be 1.'
}
if ($terminology.language -ne 'ko') {
    Add-Failure $failures 'Terminology language must be ko.'
}
if ($terminology.game_build_id -ne "steam:$GameBuildId") {
    Add-Failure $failures "Terminology build must be steam:$GameBuildId."
}
if ($core.game_build_id -ne $GameBuildId) {
    Add-Failure $failures "Core POI build must be $GameBuildId."
}

$groupIds = @($terminology.groups | ForEach-Object { [string]$_.id })
$expectedGroupIds = @('location', 'enemy', 'npc', 'collectible', 'resource')
foreach ($groupId in $expectedGroupIds) {
    if ($groupIds -notcontains $groupId) {
        Add-Failure $failures "Required group is missing: $groupId"
    }
}

$terms = @($terminology.terms)
$termIds = @($terms | ForEach-Object { [string]$_.id })
$uniqueTermIds = @($termIds | Sort-Object -Unique)
if ($terms.Count -ne 48) {
    Add-Failure $failures "Terminology must contain 48 layers. Actual: $($terms.Count)"
}
if ($uniqueTermIds.Count -ne $termIds.Count) {
    Add-Failure $failures 'Terminology contains duplicate ids.'
}

foreach ($term in $terms) {
    $id = [string]$term.id
    $label = ([string]$term.label_ko).Trim()
    $genericTitle = ([string]$term.generic_title_ko).Trim()
    $description = ([string]$term.description_ko).Trim()
    $aliases = @($term.aliases_ko | ForEach-Object { [string]$_ })
    $deprecated = @($term.deprecated_labels_ko | ForEach-Object { [string]$_ })

    if ($groupIds -notcontains [string]$term.group_id) {
        Add-Failure $failures "Unknown group: $id -> $($term.group_id)"
    }
    if ([string]::IsNullOrWhiteSpace($label)) {
        Add-Failure $failures "Korean label is missing: $id"
    }
    if ([string]::IsNullOrWhiteSpace($genericTitle)) {
        Add-Failure $failures "Generic title is missing: $id"
    }
    if ($description.Length -lt 12 -or -not $description.EndsWith('.')) {
        Add-Failure $failures "Description format is invalid: $id"
    }
    if ($description -eq $label) {
        Add-Failure $failures "Description repeats the label: $id"
    }
    if (@($aliases | Sort-Object -Unique).Count -ne $aliases.Count) {
        Add-Failure $failures "Search aliases contain duplicates: $id"
    }
    if (@($deprecated | Sort-Object -Unique).Count -ne $deprecated.Count) {
        Add-Failure $failures "Deprecated labels contain duplicates: $id"
    }
    if ($aliases -contains $label) {
        Add-Failure $failures "Canonical label is duplicated in aliases: $id"
    }
    if (@('reviewed', 'needs_game_l10n_review') -notcontains [string]$term.verification) {
        Add-Failure $failures "Unknown verification state: $id"
    }
    if (
        [string]$term.verification -eq 'needs_game_l10n_review' -and
        @('merged', 'hidden_until_verified') -notcontains [string]$term.filter_visibility
    ) {
        Add-Failure $failures "Unreviewed layer is publicly visible: $id"
    }
}

$coreKinds = @($core.pois | ForEach-Object { [string]$_.kind } | Sort-Object -Unique)
$supplementalKinds = @(
    $supplemental.layers |
        ForEach-Object { [string]$_.id } |
        Sort-Object -Unique
)
foreach ($kind in @($coreKinds + $supplementalKinds | Sort-Object -Unique)) {
    if ($termIds -notcontains $kind) {
        Add-Failure $failures "Shipped layer is missing from terminology: $kind"
    }
}

$poiIds = @($core.pois | ForEach-Object { [string]$_.id })
if ([int]$core.poi_count -ne $core.pois.Count) {
    Add-Failure $failures "poi_count does not match row count: $($core.poi_count) / $($core.pois.Count)"
}
if (@($poiIds | Sort-Object -Unique).Count -ne $poiIds.Count) {
    Add-Failure $failures 'Core POI ids are not unique.'
}

foreach ($poi in $core.pois) {
    $id = [string]$poi.id
    if ($poi.source_build_id -ne $GameBuildId) {
        Add-Failure $failures "POI source build mismatch: $id"
    }
    if ($poi.verified -ne $true) {
        Add-Failure $failures "Unverified POI is present in core data: $id"
    }
    if ([string]::IsNullOrWhiteSpace([string]$poi.display_name)) {
        Add-Failure $failures "POI display name is missing: $id"
    }
    foreach ($coordinate in @('world_x', 'world_y', 'map_x', 'map_y')) {
        if ($null -eq $poi.$coordinate -or $poi.$coordinate -isnot [ValueType]) {
            Add-Failure $failures "POI coordinate is not numeric: $id / $coordinate"
        }
    }
}

$rules = $terminology.presentation_rules
if ($rules.entity_title_excludes_layer_label -ne $true) {
    Add-Failure $failures 'Entity titles must exclude layer labels.'
}
if ($rules.subtitle_must_not_equal_title -ne $true) {
    Add-Failure $failures 'Subtitle must not equal title.'
}
if ([string]$rules.cluster_subtitle_template_ko -match '\{label\}') {
    Add-Failure $failures 'Cluster subtitle repeats the layer label.'
}

$duplicateNameGroups = @(
    $core.pois |
        Group-Object kind, display_name |
        Where-Object Count -gt 1 |
        Sort-Object Count -Descending
)
$reviewedCount = @($terms | Where-Object verification -eq 'reviewed').Count
$pendingCount = @(
    $terms |
        Where-Object verification -eq 'needs_game_l10n_review'
).Count

$summary = [ordered]@{
    ok = $failures.Count -eq 0
    game_build_id = $core.game_build_id
    core_poi_count = $core.pois.Count
    supplemental_layer_count = $supplemental.layers.Count
    terminology_count = $terms.Count
    reviewed_term_count = $reviewedCount
    needs_game_l10n_review_count = $pendingCount
    duplicate_name_group_count = $duplicateNameGroups.Count
    largest_duplicate_name_group = if ($duplicateNameGroups.Count -gt 0) {
        [ordered]@{
            key = $duplicateNameGroups[0].Name
            count = $duplicateNameGroups[0].Count
        }
    } else {
        $null
    }
    failures = @($failures)
}

$summary | ConvertTo-Json -Depth 5

if ($failures.Count -gt 0) {
    exit 1
}
