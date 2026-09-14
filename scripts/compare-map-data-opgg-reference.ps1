param(
    [string]$ReferencePath = "",
    [string]$CorePoiPath = "",
    [string]$SupplementalLayerPath = ""
)

$ErrorActionPreference = "Stop"

if ([string]::IsNullOrWhiteSpace($ReferencePath)) {
    $ReferencePath = Join-Path $PSScriptRoot `
        "..\docs\research\opgg-map-reference.ko.2026-07-26.json"
}
if ([string]::IsNullOrWhiteSpace($CorePoiPath)) {
    $CorePoiPath = Join-Path $PSScriptRoot `
        "..\assets\palbeacon\game\map\pois.v1.json"
}
if ([string]::IsNullOrWhiteSpace($SupplementalLayerPath)) {
    $SupplementalLayerPath = Join-Path $PSScriptRoot `
        "..\assets\palbeacon\game\map\layers.v1.json"
}

$reference = Get-Content -LiteralPath $ReferencePath -Raw -Encoding utf8 |
    ConvertFrom-Json
$core = Get-Content -LiteralPath $CorePoiPath -Raw -Encoding utf8 |
    ConvertFrom-Json
$supplemental = Get-Content -LiteralPath $SupplementalLayerPath -Raw -Encoding utf8 |
    ConvertFrom-Json

$supplementalLayerByIndex = @{}
foreach ($layer in $supplemental.layers) {
    $supplementalLayerByIndex[[int]$layer.index] = [string]$layer.id
}

$rows = [System.Collections.Generic.List[object]]::new()
$requiredMismatchCount = 0
foreach ($map in $reference.maps) {
    $coreCounts = @{}
    foreach ($poi in $core.pois) {
        if ([string]$poi.map_id -ne [string]$map.map_id -or
            [string]$poi.region_id -ne [string]$map.region_id) {
            continue
        }
        $kind = [string]$poi.kind
        if (-not $coreCounts.ContainsKey($kind)) {
            $coreCounts[$kind] = 0
        }
        $coreCounts[$kind]++
    }

    $supplementalCounts = @{}
    $region = @($supplemental.regions | Where-Object {
        [string]$_.map_id -eq [string]$map.map_id -and
        [string]$_.region_id -eq [string]$map.region_id
    }) | Select-Object -First 1
    if ($null -eq $region) {
        throw "Supplemental region is missing: $($map.map_id)/$($map.region_id)"
    }
    foreach ($point in $region.points) {
        $kind = $supplementalLayerByIndex[[int]$point[0]]
        if (-not $supplementalCounts.ContainsKey($kind)) {
            $supplementalCounts[$kind] = 0
        }
        $supplementalCounts[$kind]++
    }

    foreach ($check in $map.checks) {
        $countSource = if ([string]$check.local_source -eq "core") {
            $coreCounts
        } elseif ([string]$check.local_source -eq "supplemental") {
            $supplementalCounts
        } else {
            throw "Unsupported local_source: $($check.local_source)"
        }

        $localCount = 0
        foreach ($localId in $check.local_ids) {
            if ($countSource.ContainsKey([string]$localId)) {
                $localCount += [int]$countSource[[string]$localId]
            }
        }
        $referenceCount = [int]$check.reference_count
        $isMatch = $localCount -eq $referenceCount
        if (-not $isMatch -and [string]$check.gate -eq "must_match") {
            $requiredMismatchCount++
        }
        $rows.Add([pscustomobject][ordered]@{
            Map = [string]$map.label_ko
            Group = [string]$check.group_ko
            Layer = [string]$check.label_ko
            Reference = $referenceCount
            Local = $localCount
            Delta = $localCount - $referenceCount
            Gate = [string]$check.gate
            Result = if ($isMatch) { "MATCH" } else { "DIFF" }
        })
    }
}

$rows | Format-Table -AutoSize

$matchedCount = @($rows | Where-Object Result -eq "MATCH").Count
$differences = $rows.Count - $matchedCount
Write-Host ""
Write-Host "OP.GG reference parity: $matchedCount matched, $differences differences."

if ($requiredMismatchCount -gt 0) {
    throw "$requiredMismatchCount required OP.GG parity checks failed."
}
