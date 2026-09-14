[CmdletBinding()]
param(
    [string]$RepositoryRoot,
    [string]$OutputDirectory,
    [switch]$AllowHistoricalBuild24181527
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

if (-not $AllowHistoricalBuild24181527) {
    throw @"
This audit is the retired Build 24181527 inventory generator and must not
overwrite the current Build 24575825 evidence. Use scripts/run-data-pack-gate.ps1
and docs/data/DATA_QUALITY.24575825.md for the current exact-build authority.
Pass -AllowHistoricalBuild24181527 only when deliberately reproducing the
historical snapshot with all of its pinned inputs restored.
"@
}

Add-Type -AssemblyName PresentationCore
Add-Type -AssemblyName System.Web.Extensions

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = Split-Path -Parent $PSScriptRoot
}
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $RepositoryRoot "docs\data"
}

$jsonSerializer = New-Object System.Web.Script.Serialization.JavaScriptSerializer
$jsonSerializer.MaxJsonLength = [int]::MaxValue
$jsonSerializer.RecursionLimit = 512

function Read-JsonObject {
    param([Parameter(Mandatory = $true)][string]$Path)

    return $jsonSerializer.DeserializeObject([IO.File]::ReadAllText($Path, [Text.Encoding]::UTF8))
}

function Get-Sha256 {
    param([Parameter(Mandatory = $true)][string]$Path)

    return (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
}

function Test-Text {
    param([object]$Value)

    return $null -ne $Value -and -not [string]::IsNullOrWhiteSpace([string]$Value)
}

function New-Coverage {
    param(
        [Parameter(Mandatory = $true)][string]$Field,
        [Parameter(Mandatory = $true)][int]$Present,
        [Parameter(Mandatory = $true)][int]$Total
    )

    $percent = if ($Total -eq 0) { 0.0 } else { [Math]::Round(($Present * 100.0) / $Total, 2) }
    return [ordered]@{
        field = $Field
        present = $Present
        total = $Total
        percent = $percent
        missing = $Total - $Present
    }
}

function Get-CaseInsensitiveCollisions {
    param([Parameter(Mandatory = $true)][object[]]$Keys)

    $groups = [Collections.Generic.Dictionary[string, Collections.Generic.List[string]]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    foreach ($keyObject in $Keys) {
        $key = [string]$keyObject
        if (-not $groups.ContainsKey($key)) {
            $groups[$key] = [Collections.Generic.List[string]]::new()
        }
        if (-not $groups[$key].Contains($key)) {
            $groups[$key].Add($key)
        }
    }

    $result = @()
    foreach ($entry in $groups.GetEnumerator()) {
        if ($entry.Value.Count -gt 1) {
            $result += ,([ordered]@{
                folded_key = $entry.Key.ToLowerInvariant()
                keys = @($entry.Value | Sort-Object)
            })
        }
    }
    return @($result | Sort-Object { $_.folded_key })
}

function Get-IdMatchReport {
    param(
        [Parameter(Mandatory = $true)][string]$Id,
        [Parameter(Mandatory = $true)][object[]]$SourceIds,
        [Parameter(Mandatory = $true)][object[]]$TargetIds
    )

    $exactTargets = New-OrdinalSet
    $caseFoldedTargets = [Collections.Generic.Dictionary[string, Collections.Generic.List[string]]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    foreach ($targetObject in $TargetIds) {
        $target = [string]$targetObject
        [void]$exactTargets.Add($target)
        if (-not $caseFoldedTargets.ContainsKey($target)) {
            $caseFoldedTargets[$target] = [Collections.Generic.List[string]]::new()
        }
        if (-not $caseFoldedTargets[$target].Contains($target)) {
            $caseFoldedTargets[$target].Add($target)
        }
    }

    $sourceDistinct = New-OrdinalSet
    foreach ($sourceObject in $SourceIds) {
        [void]$sourceDistinct.Add([string]$sourceObject)
    }

    $exact = @()
    $caseOnly = @()
    $unmatched = @()
    foreach ($source in @($sourceDistinct | Sort-Object)) {
        if ($exactTargets.Contains($source)) {
            $exact += $source
            continue
        }
        if ($caseFoldedTargets.ContainsKey($source)) {
            $caseOnly += ,([ordered]@{
                source_id = $source
                target_candidates = @($caseFoldedTargets[$source] | Sort-Object)
            })
            continue
        }
        $unmatched += $source
    }

    return [ordered]@{
        id = $Id
        source_count = $sourceDistinct.Count
        target_count = $exactTargets.Count
        exact_match_count = $exact.Count
        case_only_match_count = $caseOnly.Count
        unmatched_count = $unmatched.Count
        exact_match_ids = $exact
        case_only_matches = $caseOnly
        unmatched_ids = $unmatched
    }
}

function New-OrdinalSet {
    return ,([Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal))
}

function New-OrdinalIgnoreCaseSet {
    return ,([Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase))
}

function Convert-ToRepoPath {
    param([Parameter(Mandatory = $true)][string]$AbsolutePath)

    $rootWithSeparator = $RepositoryRoot.TrimEnd("\", "/") + [IO.Path]::DirectorySeparatorChar
    if (-not $AbsolutePath.StartsWith($rootWithSeparator, [StringComparison]::OrdinalIgnoreCase)) {
        return $AbsolutePath.Replace("\", "/")
    }
    return $AbsolutePath.Substring($rootWithSeparator.Length).Replace("\", "/")
}

function Get-AssetClass {
    param([Parameter(Mandatory = $true)][string]$RepoPath)

    switch -Regex ($RepoPath) {
        "^assets/palbeacon/game/pals/T_dummy_icon\.webp$" { return "game_fallback_pal_icon" }
        "^assets/palbeacon/game/pals/" { return "game_pal_icon" }
        "^assets/palbeacon/game/elements/" { return "game_element_icon" }
        "^assets/palbeacon/game/items/icons/" { return "game_item_icon" }
        "^assets/palbeacon/game/buildings/icons/" { return "game_building_icon" }
        "^assets/palbeacon/game/map/" { return "game_map_asset" }
        "^assets/palbeacon/game/wanted/" { return "game_wanted_portrait" }
        "^assets/palbeacon/brand/generated/" { return "palbeacon_generated_brand" }
        default { return "other_app_asset" }
    }
}

function Get-RasterMetadata {
    param(
        [Parameter(Mandatory = $true)][string]$AbsolutePath,
        [Parameter(Mandatory = $true)][string]$RepoPath
    )

    $file = Get-Item -LiteralPath $AbsolutePath
    $result = [ordered]@{
        path = $RepoPath
        class = Get-AssetClass -RepoPath $RepoPath
        extension = $file.Extension.ToLowerInvariant()
        size_bytes = $file.Length
        sha256 = Get-Sha256 -Path $AbsolutePath
        width = $null
        height = $null
        source_pixel_format = $null
        has_alpha_channel = $null
        uses_transparency = $null
        alpha_min = $null
        alpha_max = $null
        transparent_pixel_count = $null
        partial_alpha_pixel_count = $null
        opaque_pixel_count = $null
        nontransparent_bounds = $null
        transparent_padding = $null
        decode_error = $null
        source_asset_paths = @()
        logical_ids = @()
        manifest_sha256_matches_file = $null
        build_verified = $false
    }

    if ($file.Extension -notin @(".png", ".webp", ".jpg", ".jpeg", ".bmp", ".gif", ".tif", ".tiff", ".ico")) {
        $result.decode_error = "unsupported_non_raster_format"
        return $result
    }

    $stream = $null
    try {
        $stream = [IO.File]::Open($AbsolutePath, [IO.FileMode]::Open, [IO.FileAccess]::Read, [IO.FileShare]::Read)
        $decoder = [Windows.Media.Imaging.BitmapDecoder]::Create(
            $stream,
            [Windows.Media.Imaging.BitmapCreateOptions]::PreservePixelFormat,
            [Windows.Media.Imaging.BitmapCacheOption]::OnLoad
        )
        $frame = $decoder.Frames[0]
        $sourceFormat = $frame.Format.ToString()

        $converted = New-Object Windows.Media.Imaging.FormatConvertedBitmap
        $converted.BeginInit()
        $converted.Source = $frame
        $converted.DestinationFormat = [Windows.Media.PixelFormats]::Bgra32
        $converted.EndInit()

        $width = $converted.PixelWidth
        $height = $converted.PixelHeight
        $stride = $width * 4
        $pixels = New-Object byte[] ($stride * $height)
        $converted.CopyPixels($pixels, $stride, 0)

        $alphaMin = 255
        $alphaMax = 0
        $transparent = 0L
        $partial = 0L
        $opaque = 0L
        $minX = $width
        $minY = $height
        $maxX = -1
        $maxY = -1

        for ($y = 0; $y -lt $height; $y++) {
            $rowOffset = $y * $stride
            for ($x = 0; $x -lt $width; $x++) {
                $alpha = [int]$pixels[$rowOffset + ($x * 4) + 3]
                if ($alpha -lt $alphaMin) { $alphaMin = $alpha }
                if ($alpha -gt $alphaMax) { $alphaMax = $alpha }
                if ($alpha -eq 0) {
                    $transparent++
                    continue
                }

                if ($alpha -eq 255) { $opaque++ } else { $partial++ }
                if ($x -lt $minX) { $minX = $x }
                if ($x -gt $maxX) { $maxX = $x }
                if ($y -lt $minY) { $minY = $y }
                if ($y -gt $maxY) { $maxY = $y }
            }
        }

        $hasContent = $maxX -ge 0 -and $maxY -ge 0
        $result.width = $width
        $result.height = $height
        $result.source_pixel_format = $sourceFormat
        $result.has_alpha_channel = $sourceFormat -match "(?i)alpha|argb|bgra|rgba|pbgra|prgba"
        $result.uses_transparency = $alphaMin -lt 255
        $result.alpha_min = $alphaMin
        $result.alpha_max = $alphaMax
        $result.transparent_pixel_count = $transparent
        $result.partial_alpha_pixel_count = $partial
        $result.opaque_pixel_count = $opaque
        if ($hasContent) {
            $result.nontransparent_bounds = [ordered]@{
                x = $minX
                y = $minY
                width = $maxX - $minX + 1
                height = $maxY - $minY + 1
            }
            $result.transparent_padding = [ordered]@{
                left = $minX
                top = $minY
                right = $width - $maxX - 1
                bottom = $height - $maxY - 1
            }
        }
    }
    catch {
        $result.decode_error = $_.Exception.Message
    }
    finally {
        if ($null -ne $stream) { $stream.Dispose() }
    }

    return $result
}

$RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
if (-not (Test-Path -LiteralPath $OutputDirectory)) {
    New-Item -ItemType Directory -Path $OutputDirectory | Out-Null
}
$OutputDirectory = (Resolve-Path -LiteralPath $OutputDirectory).Path

$paths = [ordered]@{
    pals = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\pals.json"
    active_skills = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\active_skills.json"
    passive_skills = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\passive_skills.json"
    pal_l10n_ko = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\l10n\ko\pals.json"
    active_l10n_ko = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\l10n\ko\active_skills.json"
    passive_l10n_ko = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\l10n\ko\passive_skills.json"
    korean_manifest = Join-Path $RepositoryRoot "assets\save-parser\l10n\ko\manifest.v1.json"
    items = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\items.v1.json"
    world = Join-Path $RepositoryRoot "assets\palbeacon\game\catalog\world.v1.json"
    spawns = Join-Path $RepositoryRoot "assets\palbeacon\game\map\spawns.search.v1.json"
    breeding = Join-Path $RepositoryRoot "assets\breeding\breeding.json"
    item_icon_manifest = Join-Path $RepositoryRoot "assets\palbeacon\game\items\manifest.json"
    building_icon_manifest = Join-Path $RepositoryRoot "assets\palbeacon\game\buildings\manifest.json"
    item_localization_matches = Join-Path $RepositoryRoot "docs\data\ITEM_LOCALIZATION_MATCHES.24181527.json"
    game_icon_manifest = Join-Path $RepositoryRoot "docs\data\GAME_ICON_MANIFEST.24181527.json"
    pal_icon_source_links = Join-Path $RepositoryRoot "docs\data\PAL_ICON_SOURCE_LINKS.24181527.json"
    game_icon_match_report = Join-Path $RepositoryRoot "docs\data\GAME_ICON_MATCH_REPORT.24181527.json"
    raid_display_icon_links = Join-Path $RepositoryRoot "docs\data\RAID_DISPLAY_ICON_LINKS.24181527.json"
    source_probe = Join-Path $RepositoryRoot "docs\data\SOURCE_PROBE.24181527.json"
    parser_lock = Join-Path $RepositoryRoot "third_party\palworld-save-parser.lock.json"
    parser_pals = Join-Path $RepositoryRoot "assets\save-parser\pals.json"
    parser_active_skills = Join-Path $RepositoryRoot "assets\save-parser\active_skills.json"
    parser_passive_skills = Join-Path $RepositoryRoot "assets\save-parser\passive_skills.json"
    parser_items = Join-Path $RepositoryRoot "assets\save-parser\items.v1.json"
    parser_world = Join-Path $RepositoryRoot "assets\save-parser\world.v1.json"
    parser_pal_l10n_ko = Join-Path $RepositoryRoot "assets\save-parser\l10n\ko\pals.json"
    parser_active_l10n_ko = Join-Path $RepositoryRoot "assets\save-parser\l10n\ko\active_skills.json"
    parser_passive_l10n_ko = Join-Path $RepositoryRoot "assets\save-parser\l10n\ko\passive_skills.json"
}
foreach ($entry in $paths.GetEnumerator()) {
    if (-not (Test-Path -LiteralPath $entry.Value -PathType Leaf)) {
        throw "Required audit input is missing: $($entry.Key) ($($entry.Value))"
    }
}

$pals = Read-JsonObject -Path $paths.pals
$activeSkills = Read-JsonObject -Path $paths.active_skills
$passiveSkills = Read-JsonObject -Path $paths.passive_skills
$palL10n = Read-JsonObject -Path $paths.pal_l10n_ko
$activeL10n = Read-JsonObject -Path $paths.active_l10n_ko
$passiveL10n = Read-JsonObject -Path $paths.passive_l10n_ko
[void] (Read-JsonObject -Path $paths.korean_manifest)
$itemsRoot = Read-JsonObject -Path $paths.items
$worldRoot = Read-JsonObject -Path $paths.world
$spawnsRoot = Read-JsonObject -Path $paths.spawns
$breedingRoot = Read-JsonObject -Path $paths.breeding
$itemIconManifest = Read-JsonObject -Path $paths.item_icon_manifest
$buildingIconManifest = Read-JsonObject -Path $paths.building_icon_manifest
$itemLocalizationMatches = Read-JsonObject -Path $paths.item_localization_matches
$gameIconManifest = Read-JsonObject -Path $paths.game_icon_manifest
$palIconSourceLinks = Read-JsonObject -Path $paths.pal_icon_source_links
$gameIconMatchReport = Read-JsonObject -Path $paths.game_icon_match_report
$raidDisplayIconLinks = Read-JsonObject -Path $paths.raid_display_icon_links
$sourceProbe = Read-JsonObject -Path $paths.source_probe
$parserLock = Read-JsonObject -Path $paths.parser_lock

$gameIconManifestHash = Get-Sha256 -Path $paths.game_icon_manifest
$palIconSourceLinksHash = Get-Sha256 -Path $paths.pal_icon_source_links
$gameIconKoreanCatalogHash = Get-Sha256 -Path $paths.pal_l10n_ko
$itemCatalogHash = Get-Sha256 -Path $paths.items
$itemIconManifestHash = Get-Sha256 -Path $paths.item_icon_manifest
$worldCatalogHash = Get-Sha256 -Path $paths.world
$buildingIconManifestHash = Get-Sha256 -Path $paths.building_icon_manifest
$gameIconMatchReportHash = Get-Sha256 -Path $paths.game_icon_match_report
$raidDisplayIconLinksHash = Get-Sha256 -Path $paths.raid_display_icon_links
if (
    $itemLocalizationMatches["verified"] -ne $true -or
    [string]$itemLocalizationMatches["game_build_id"] -ne "steam:24181527" -or
    [string]$itemLocalizationMatches["mapping_sha256"] -ne [string]$itemsRoot["mapping_sha256"] -or
    [string]$itemLocalizationMatches["item_catalog_sha256"] -ne $itemCatalogHash -or
    [int]$itemLocalizationMatches["case_only_key_count"] -ne 19 -or
    [int]$itemLocalizationMatches["affected_name_record_count"] -ne 6 -or
    [int]$itemLocalizationMatches["affected_description_record_count"] -ne 90 -or
    [int]$itemLocalizationMatches["collision_count"] -ne 0 -or
    @($itemLocalizationMatches["matches"]).Count -ne 19 -or
    [string]$buildingIconManifest["game_build_id"] -ne "24181527" -or
    [string]$buildingIconManifest["mapping_sha256"] -ne [string]$worldRoot["mapping_sha256"] -or
    [string]$buildingIconManifest["world_catalog_sha256"] -ne $worldCatalogHash -or
    [string]$buildingIconManifest["localization_origin"] -ne "game_asset" -or
    $buildingIconManifest["original_game_assets_only"] -ne $true -or
    @($buildingIconManifest["icons"]).Count -ne 487 -or
    @($buildingIconManifest["missing_original_building_ids"]).Count -ne 0 -or
    [string]$gameIconManifest["game_build_id"] -ne "24181527" -or
    [string]$gameIconManifest["mapping_sha256"] -ne "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0" -or
    [string]$gameIconManifest["distribution_scope"] -ne "local_windows_only" -or
    $gameIconManifest["original_game_assets_only"] -ne $true -or
    @($gameIconManifest["icons"]).Count -ne 589 -or
    $palIconSourceLinks["verified"] -ne $true -or
    [string]$palIconSourceLinks["game_build_id"] -ne "24181527" -or
    [string]$palIconSourceLinks["mapping_sha256"] -ne "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0" -or
    [int]$palIconSourceLinks["parameter_table_row_count"] -ne 753 -or
    [int]$palIconSourceLinks["icon_table_row_count"] -ne 674 -or
    @($palIconSourceLinks["icon_table_entries"]).Count -ne 674 -or
    $gameIconMatchReport["verified"] -ne $true -or
    [string]$gameIconMatchReport["icon_manifest_sha256"] -ne $gameIconManifestHash -or
    [string]$gameIconMatchReport["korean_pal_catalog_sha256"] -ne $gameIconKoreanCatalogHash -or
    [string]$gameIconMatchReport["pal_icon_source_links_sha256"] -ne $palIconSourceLinksHash -or
    [int]$gameIconMatchReport["eligible_pal_icon_count"] -ne 308 -or
    [int]$gameIconMatchReport["multi_icon_group_match_count"] -ne 1 -or
    [int]$gameIconMatchReport["multi_icon_group_member_count"] -ne 2 -or
    [int]$gameIconMatchReport["korean_localization_without_icon_count"] -ne 2 -or
    [int]$gameIconMatchReport["icon_without_korean_localization_count"] -ne 123 -or
    $raidDisplayIconLinks["verified"] -ne $true -or
    [string]$raidDisplayIconLinks["game_build_id"] -ne "24181527" -or
    [string]$raidDisplayIconLinks["mapping_sha256"] -ne "241c45de9d5b55b246cd4b39d62b9209faf7758ce0637e1f7a545aa0f75f71f0" -or
    [string]$raidDisplayIconLinks["distribution_scope"] -ne "local_windows_only" -or
    $raidDisplayIconLinks["policy"]["external_images_copied"] -ne $false -or
    $raidDisplayIconLinks["policy"]["only_exact_installed_game_assets_are_linked"] -ne $true -or
    [string]$raidDisplayIconLinks["source_hashes"]["item_catalog_sha256"] -ne $itemCatalogHash -or
    [string]$raidDisplayIconLinks["source_hashes"]["item_icon_manifest_sha256"] -ne $itemIconManifestHash -or
    [string]$raidDisplayIconLinks["source_hashes"]["pal_icon_source_links_sha256"] -ne $palIconSourceLinksHash -or
    [string]$raidDisplayIconLinks["source_hashes"]["pal_portrait_match_report_sha256"] -ne $gameIconMatchReportHash -or
    @($raidDisplayIconLinks["links"]).Count -ne 2 -or
    @($raidDisplayIconLinks["unresolved"]).Count -ne 1 -or
    [int]$raidDisplayIconLinks["summary"]["raid_display_icon_link_count"] -ne 2 -or
    [int]$raidDisplayIconLinks["summary"]["remaining_without_exact_standalone_display_icon_count"] -ne 1
) {
    throw "Reviewed localization, item/building icon, or raid display report identity changed."
}

Push-Location $RepositoryRoot
try {
    $gitCommit = (& git rev-parse HEAD).Trim()
    $gitBranch = (& git branch --show-current).Trim()
    # Include newly extracted, not-yet-staged assets so a pre-commit audit
    # cannot silently omit them from coverage and hash checks.
    $trackedAssetPaths = @(
        & git ls-files --cached --others --exclude-standard -- "assets/palbeacon" |
            Where-Object { $_ -match "(?i)\.(png|webp|jpg|jpeg|bmp|gif|tif|tiff|ico|svg)$" } |
            ForEach-Object { $_.Replace("\", "/") } |
            Sort-Object
    )
}
finally {
    Pop-Location
}

$palEntries = @($pals.GetEnumerator())
$palRows = @($palEntries | Where-Object { $_.Value["is_pal"] -eq $true -and $_.Value["disabled"] -ne $true })
$palDeckRows = @($palRows | Where-Object { [int]$_.Value["pal_deck_index"] -gt 0 })
$catalogKeysExact = New-OrdinalSet
$catalogKeysIgnoreCase = New-OrdinalIgnoreCaseSet
foreach ($entry in $palEntries) {
    [void]$catalogKeysExact.Add([string]$entry.Key)
    [void]$catalogKeysIgnoreCase.Add([string]$entry.Key)
}
$palKeysExact = New-OrdinalSet
$palKeysIgnoreCase = New-OrdinalIgnoreCaseSet
foreach ($entry in $palRows) {
    [void]$palKeysExact.Add([string]$entry.Key)
    [void]$palKeysIgnoreCase.Add([string]$entry.Key)
}

$itemRows = @($itemsRoot["items"])
$legalItemRows = @($itemRows | Where-Object { $_["legal_in_game"] -eq $true })
$itemKeysExact = New-OrdinalSet
$itemKeysIgnoreCase = New-OrdinalIgnoreCaseSet
$itemById = @{}
foreach ($item in $itemRows) {
    $itemId = [string]$item["item_id"]
    [void]$itemKeysExact.Add($itemId)
    [void]$itemKeysIgnoreCase.Add($itemId)
    $itemById[$itemId] = $item
}

$itemIconByName = @{}
foreach ($icon in @($itemIconManifest["icons"])) {
    $itemIconByName[[string]$icon["icon_name"]] = $icon
}
foreach ($link in @($raidDisplayIconLinks["links"])) {
    $linkedItemId = [string]$link["item_id"]
    if (-not $itemById.ContainsKey($linkedItemId)) {
        throw "Raid display icon references an unknown item: $linkedItemId"
    }
    $linkedItem = $itemById[$linkedItemId]
    if (
        [string]$linkedItem["name_ko"] -ne [string]$link["item_name_ko"] -or
        [string]$linkedItem["icon_name"] -ne $linkedItemId -or
        -not $itemIconByName.ContainsKey($linkedItemId)
    ) {
        throw "Raid display icon item identity changed: $linkedItemId"
    }

    $linkedIcon = $itemIconByName[$linkedItemId]
    $expectedRepoPath = "assets/palbeacon/game/items/" +
        ([string]$linkedIcon["relative_path"]).Replace("\", "/")
    $expectedAbsolutePath = Join-Path $RepositoryRoot $expectedRepoPath.Replace("/", "\")
    if (
        [string]$linkedIcon["match_kind"] -ne "typed_exact" -or
        [string]$linkedIcon["package_path"] -ne [string]$link["icon_package_path"] -or
        $expectedRepoPath -ne [string]$link["extracted_relative_path"] -or
        [int]$linkedIcon["source_width"] -ne [int]$link["width"] -or
        [int]$linkedIcon["source_height"] -ne [int]$link["height"] -or
        [string]$linkedIcon["output_sha256"] -ne [string]$link["png_sha256"] -or
        [string]$link["icon_role"] -ne "raid_summon_item_icon" -or
        [string]$link["display_icon_status"] -ne "resolved_exact_installed_game_ui_icon" -or
        -not (Test-Text $link["required_ui_badge_ko"]) -or
        -not (Test-Path -LiteralPath $expectedAbsolutePath -PathType Leaf) -or
        (Get-Sha256 -Path $expectedAbsolutePath) -ne [string]$link["png_sha256"]
    ) {
        throw "Raid display icon asset contract changed: $linkedItemId"
    }
}
$unresolvedRaidDisplayIcon = @($raidDisplayIconLinks["unresolved"])[0]
if (
    [string]$unresolvedRaidDisplayIcon["display_entity_id"] -ne "RAID_YakushimaBoss001_Green" -or
    [string]$unresolvedRaidDisplayIcon["entity_role"] -ne "moon_lord_spawned_subentity" -or
    [string]$unresolvedRaidDisplayIcon["display_icon_status"] -ne "no_exact_standalone_game_ui_icon_found" -or
    -not (Test-Text $unresolvedRaidDisplayIcon["rejected_substitution"]["package_path"])
) {
    throw "Unresolved raid display icon classification changed."
}

$sourceProbe["reviewed_reextract_comparison"]["game_icons"]["raid_display_icon_variant_count"] = 2
$sourceProbe["reviewed_reextract_comparison"]["game_icons"]["remaining_without_exact_display_icon_count"] = 1
$sourceProbe["reviewed_reextract_comparison"]["game_icons"]["raid_display_icon_report_sha256"] =
    $raidDisplayIconLinksHash

$buildingKeysExact = New-OrdinalSet
foreach ($building in @($worldRoot["buildings"])) {
    [void]$buildingKeysExact.Add([string]$building["building_id"])
}

$assetRows = @()
foreach ($repoPath in $trackedAssetPaths) {
    $absolutePath = Join-Path $RepositoryRoot $repoPath.Replace("/", "\")
    $assetRows += ,(Get-RasterMetadata -AbsolutePath $absolutePath -RepoPath $repoPath)
}

$itemManifestByRelativePath = @{}
foreach ($icon in @($itemIconManifest["icons"])) {
    $relative = "assets/palbeacon/game/items/" + ([string]$icon["relative_path"]).Replace("\", "/")
    if (-not $itemManifestByRelativePath.ContainsKey($relative)) {
        $itemManifestByRelativePath[$relative] = @()
    }
    $itemManifestByRelativePath[$relative] += ,$icon
}
$buildingManifestByRelativePath = @{}
foreach ($icon in @($buildingIconManifest["icons"])) {
    $relative = "assets/palbeacon/game/buildings/" + ([string]$icon["relative_path"]).Replace("\", "/")
    if (-not $buildingManifestByRelativePath.ContainsKey($relative)) {
        $buildingManifestByRelativePath[$relative] = @()
    }
    $buildingManifestByRelativePath[$relative] += ,$icon
}
foreach ($asset in $assetRows) {
    if ($itemManifestByRelativePath.ContainsKey($asset.path)) {
        $manifestRows = @($itemManifestByRelativePath[$asset.path])
        $asset.logical_ids = @($manifestRows | ForEach-Object { [string]$_["icon_name"] } | Sort-Object -Unique)
        $asset.source_asset_paths = @($manifestRows | ForEach-Object { [string]$_["package_path"] } | Sort-Object -Unique)
        $expectedHashes = @($manifestRows | ForEach-Object { [string]$_["output_sha256"] } | Sort-Object -Unique)
        $asset.manifest_sha256_matches_file =
            $expectedHashes.Count -eq 1 -and $expectedHashes[0] -eq [string]$asset.sha256
        $asset.build_verified = $asset.manifest_sha256_matches_file
    }
    elseif ($buildingManifestByRelativePath.ContainsKey($asset.path)) {
        $manifestRows = @($buildingManifestByRelativePath[$asset.path])
        $asset.logical_ids = @($manifestRows | ForEach-Object { [string]$_["building_id"] } | Sort-Object -Unique)
        $asset.source_asset_paths = @($manifestRows | ForEach-Object { [string]$_["package_path"] } | Sort-Object -Unique)
        $expectedHashes = @($manifestRows | ForEach-Object { [string]$_["output_sha256"] } | Sort-Object -Unique)
        $asset.manifest_sha256_matches_file =
            $expectedHashes.Count -eq 1 -and $expectedHashes[0] -eq [string]$asset.sha256
        $asset.build_verified = $asset.manifest_sha256_matches_file
    }
}

$assetByPath = @{}
foreach ($asset in $assetRows) {
    $assetByPath[[string]$asset.path] = $asset
}
$itemManifestMissingFilePaths = @()
$itemManifestHashMismatchPaths = @()
foreach ($relative in @($itemManifestByRelativePath.Keys | Sort-Object)) {
    if (-not $assetByPath.ContainsKey($relative)) {
        $itemManifestMissingFilePaths += $relative
        continue
    }
    if ($assetByPath[$relative].manifest_sha256_matches_file -ne $true) {
        $itemManifestHashMismatchPaths += $relative
    }
}
$buildingManifestMissingFilePaths = @()
$buildingManifestHashMismatchPaths = @()
foreach ($relative in @($buildingManifestByRelativePath.Keys | Sort-Object)) {
    if (-not $assetByPath.ContainsKey($relative)) {
        $buildingManifestMissingFilePaths += $relative
        continue
    }
    if ($assetByPath[$relative].manifest_sha256_matches_file -ne $true) {
        $buildingManifestHashMismatchPaths += $relative
    }
}

$palIconAssets = @($assetRows | Where-Object { $_.class -eq "game_pal_icon" })
$palIconNames = New-OrdinalIgnoreCaseSet
foreach ($asset in $palIconAssets) {
    [void]$palIconNames.Add([IO.Path]::GetFileNameWithoutExtension([string]$asset.path))
}

$palIconResolved = @(
    $palRows | Where-Object {
        (Test-Text $_.Value["icon"]) -and $palIconNames.Contains([string]$_.Value["icon"])
    }
).Count
$palDeckIconResolved = @(
    $palDeckRows | Where-Object {
        (Test-Text $_.Value["icon"]) -and $palIconNames.Contains([string]$_.Value["icon"])
    }
).Count
$localizedPalIconResolved = [int]$gameIconMatchReport["eligible_pal_icon_count"]
$localizedPalIconGroupResolved = [int]$gameIconMatchReport["multi_icon_group_match_count"]
$localizedPalMissingIconIds = @(
    @($gameIconMatchReport["korean_localizations_without_icon"]) |
        ForEach-Object { [string]$_["internal_id"] } |
        Sort-Object
)

$palKoreanExact = @(
    $palRows | Where-Object {
        $palL10n.ContainsKey([string]$_.Key) -and (Test-Text $palL10n[[string]$_.Key]["localized_name"])
    }
).Count
$palKoreanDescriptionExact = @(
    $palRows | Where-Object {
        $palL10n.ContainsKey([string]$_.Key) -and (Test-Text $palL10n[[string]$_.Key]["description"])
    }
).Count

$palL10nIgnoreCase = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::OrdinalIgnoreCase)
foreach ($entry in $palL10n.GetEnumerator()) {
    if (-not $palL10nIgnoreCase.ContainsKey([string]$entry.Key)) {
        $palL10nIgnoreCase.Add([string]$entry.Key, $entry.Value)
    }
}
$palKoreanCaseFolded = @(
    $palRows | Where-Object {
        $palL10nIgnoreCase.ContainsKey([string]$_.Key) -and
        (Test-Text $palL10nIgnoreCase[[string]$_.Key]["localized_name"])
    }
).Count
$palKoreanExactJoinMissingIds = @(
    $palRows |
        Where-Object { -not $palL10n.ContainsKey([string]$_.Key) } |
        ForEach-Object { [string]$_.Key } |
        Sort-Object
)
$palKoreanCaseOnlyJoinIds = @(
    $palRows |
        Where-Object {
            -not $palL10n.ContainsKey([string]$_.Key) -and
            $palL10nIgnoreCase.ContainsKey([string]$_.Key)
        } |
        ForEach-Object { [string]$_.Key } |
        Sort-Object
)

$palFieldCoverage = @(
    New-Coverage "internal_id" $palRows.Count $palRows.Count
    New-Coverage "paldex_number_positive" @($palRows | Where-Object { [int]$_.Value["pal_deck_index"] -gt 0 }).Count $palRows.Count
    New-Coverage "korean_name_exact_id_join" $palKoreanExact $palRows.Count
    New-Coverage "korean_description_exact_id_join" $palKoreanDescriptionExact $palRows.Count
    New-Coverage "english_name" 0 $palRows.Count
    New-Coverage "english_description" 0 $palRows.Count
    New-Coverage "elements_nonempty" @($palRows | Where-Object { @($_.Value["element_types"]).Count -gt 0 }).Count $palRows.Count
    New-Coverage "base_stats_hp_attack_defense" @(
        $palRows | Where-Object {
            $null -ne $_.Value["scaling"] -and
            $null -ne $_.Value["scaling"]["hp"] -and
            $null -ne $_.Value["scaling"]["attack"] -and
            $null -ne $_.Value["scaling"]["defense"]
        }
    ).Count $palRows.Count
    New-Coverage "work_suitability_object" @($palRows | Where-Object { $null -ne $_.Value["work_suitability"] }).Count $palRows.Count
    New-Coverage "nocturnal_flag" @($palRows | Where-Object { $_.Value.ContainsKey("nocturnal") }).Count $palRows.Count
    New-Coverage "movement_speed_fields" @(
        $palRows | Where-Object {
            $_.Value.ContainsKey("walk_speed") -and
            $_.Value.ContainsKey("run_speed") -and
            $_.Value.ContainsKey("ride_sprint_speed")
        }
    ).Count $palRows.Count
    New-Coverage "stamina" @($palRows | Where-Object { $_.Value.ContainsKey("stamina") }).Count $palRows.Count
    New-Coverage "active_skill_set_object" @($palRows | Where-Object { $null -ne $_.Value["skill_set"] }).Count $palRows.Count
    New-Coverage "pal_icon_resolved" $palIconResolved $palRows.Count
)

$activeSkillLinks = 0
$activeSkillLinkUnresolved = 0
$activeSkillLinkUnresolvedIds = New-OrdinalSet
foreach ($pal in $palRows) {
    foreach ($skillIdObject in $pal.Value["skill_set"].Keys) {
        $activeSkillLinks++
        $skillId = "EPalWazaID::$skillIdObject"
        if (-not $activeSkills.ContainsKey($skillId)) {
            $activeSkillLinkUnresolved++
            [void]$activeSkillLinkUnresolvedIds.Add([string]$skillIdObject)
        }
    }
}

$passiveSkillLinks = 0
$passiveSkillLinkUnresolved = 0
$passiveSkillLinkUnresolvedIds = New-OrdinalSet
foreach ($pal in $palRows) {
    foreach ($skillIdObject in @($pal.Value["passive_skills"])) {
        $passiveSkillLinks++
        $skillId = [string]$skillIdObject
        if (-not $passiveSkills.ContainsKey($skillId)) {
            $passiveSkillLinkUnresolved++
            [void]$passiveSkillLinkUnresolvedIds.Add($skillId)
        }
    }
}

$itemManifestNames = New-OrdinalIgnoreCaseSet
foreach ($icon in @($itemIconManifest["icons"])) {
    [void]$itemManifestNames.Add([string]$icon["icon_name"])
}
$legalItemIconResolved = @(
    $legalItemRows | Where-Object {
        (Test-Text $_["icon_name"]) -and $itemManifestNames.Contains([string]$_["icon_name"])
    }
).Count
$legalItemMissingKoreanDescriptionIds = @(
    $legalItemRows |
        Where-Object { -not (Test-Text $_["description_ko"]) } |
        ForEach-Object { [string]$_["item_id"] } |
        Sort-Object
)
$legalItemDescriptionDynamicMarkupGapIds = @(
    $legalItemRows |
        Where-Object {
            (Test-Text $_["description_ko"]) -and
            [string]$_["description_ko"] -match "\(\s*\)"
        } |
        ForEach-Object { [string]$_["item_id"] } |
        Sort-Object
)
$legalItemLocalizationFallbackIds = @(
    $legalItemRows |
        Where-Object { $_["localization_fallback"] -eq $true } |
        ForEach-Object { [string]$_["item_id"] } |
        Sort-Object
)

$itemFieldCoverage = @(
    New-Coverage "internal_id" @($legalItemRows | Where-Object { Test-Text $_["item_id"] }).Count $legalItemRows.Count
    New-Coverage "korean_name" @($legalItemRows | Where-Object { Test-Text $_["name_ko"] }).Count $legalItemRows.Count
    New-Coverage "korean_name_without_localization_fallback" @(
        $legalItemRows | Where-Object {
            (Test-Text $_["name_ko"]) -and $_["localization_fallback"] -ne $true
        }
    ).Count $legalItemRows.Count
    New-Coverage "korean_description" @($legalItemRows | Where-Object { Test-Text $_["description_ko"] }).Count $legalItemRows.Count
    New-Coverage "category_type_a" @($legalItemRows | Where-Object { Test-Text $_["type_a"] }).Count $legalItemRows.Count
    New-Coverage "subcategory_type_b" @($legalItemRows | Where-Object { Test-Text $_["type_b"] }).Count $legalItemRows.Count
    New-Coverage "rarity" @($legalItemRows | Where-Object { $_.ContainsKey("rarity") }).Count $legalItemRows.Count
    New-Coverage "weight" @($legalItemRows | Where-Object { $_.ContainsKey("weight_milli") }).Count $legalItemRows.Count
    New-Coverage "price" @($legalItemRows | Where-Object { $_.ContainsKey("price") }).Count $legalItemRows.Count
    New-Coverage "maximum_stack_count" @($legalItemRows | Where-Object { $_.ContainsKey("maximum_stack_count") }).Count $legalItemRows.Count
    New-Coverage "icon_manifest_match" $legalItemIconResolved $legalItemRows.Count
)

$recipeOutputRefs = 0
$recipeOutputUnresolved = 0
$ingredientRefs = 0
$ingredientUnresolved = 0
$unlockRefs = 0
$unlockUnresolved = 0
$usedAsIngredientItems = New-OrdinalSet
foreach ($recipe in @($itemsRoot["recipes"])) {
    $recipeOutputRefs++
    if (-not $itemKeysExact.Contains([string]$recipe["output_item_id"])) { $recipeOutputUnresolved++ }
    foreach ($ingredient in @($recipe["ingredients"])) {
        $ingredientRefs++
        $ingredientId = [string]$ingredient["item_id"]
        [void]$usedAsIngredientItems.Add($ingredientId)
        if (-not $itemKeysExact.Contains($ingredientId)) { $ingredientUnresolved++ }
    }
    if (Test-Text $recipe["unlock_item_id"]) {
        $unlockRefs++
        if (-not $itemKeysExact.Contains([string]$recipe["unlock_item_id"])) { $unlockUnresolved++ }
    }
}

$dropPalRefs = 0
$dropPalUnresolvedCatalogExact = 0
$dropPalUnresolvedCatalogCaseFolded = 0
$dropPalUnresolvedPalFlagExact = 0
$dropPalUnresolvedPalFlagCaseFolded = 0
$dropPalUnresolvedCatalogIds = New-OrdinalSet
$dropPalUnresolvedPalFlagIds = New-OrdinalSet
$dropItemRefs = 0
$dropItemUnresolved = 0
$dropPalIds = New-OrdinalSet
$dropItemIds = New-OrdinalSet
foreach ($drop in @($itemsRoot["pal_drops"])) {
    $dropPalRefs++
    $palId = [string]$drop["pal_id"]
    [void]$dropPalIds.Add($palId)
    if (-not $catalogKeysExact.Contains($palId)) {
        $dropPalUnresolvedCatalogExact++
        [void]$dropPalUnresolvedCatalogIds.Add($palId)
    }
    if (-not $catalogKeysIgnoreCase.Contains($palId)) { $dropPalUnresolvedCatalogCaseFolded++ }
    if (-not $palKeysExact.Contains($palId)) {
        $dropPalUnresolvedPalFlagExact++
        [void]$dropPalUnresolvedPalFlagIds.Add($palId)
    }
    if (-not $palKeysIgnoreCase.Contains($palId)) { $dropPalUnresolvedPalFlagCaseFolded++ }

    $dropItemRefs++
    $itemId = [string]$drop["item_id"]
    [void]$dropItemIds.Add($itemId)
    if (-not $itemKeysExact.Contains($itemId)) { $dropItemUnresolved++ }
}

$productionItemRefs = 0
$productionItemUnresolved = 0
$productionBuildingRefs = 0
$productionBuildingUnresolved = 0
$productionBuildingUnresolvedIds = New-OrdinalSet
$productionItemIds = New-OrdinalSet
foreach ($method in @($worldRoot["production_methods"])) {
    $productionItemRefs++
    $itemId = [string]$method["item_id"]
    [void]$productionItemIds.Add($itemId)
    if (-not $itemKeysExact.Contains($itemId)) { $productionItemUnresolved++ }

    $productionBuildingRefs++
    if (-not $buildingKeysExact.Contains([string]$method["building_id"])) {
        $productionBuildingUnresolved++
        [void]$productionBuildingUnresolvedIds.Add([string]$method["building_id"])
    }
}

$shopItemRefs = 0
$shopItemUnresolved = 0
$shopCurrencyRefs = 0
$shopCurrencyUnresolved = 0
$shopItemIds = New-OrdinalSet
foreach ($shop in @($worldRoot["shop_groups"])) {
    if (Test-Text $shop["currency_item_id"]) {
        $shopCurrencyRefs++
        if (-not $itemKeysExact.Contains([string]$shop["currency_item_id"])) { $shopCurrencyUnresolved++ }
    }
    foreach ($product in @($shop["products"])) {
        $shopItemRefs++
        $itemId = [string]$product["item_id"]
        [void]$shopItemIds.Add($itemId)
        if (-not $itemKeysExact.Contains($itemId)) { $shopItemUnresolved++ }
    }
}

$spawnSpeciesKeys = @($spawnsRoot["species"].Keys)
$spawnSpeciesResolvedCatalogExact = 0
$spawnSpeciesResolvedCatalogCaseFolded = 0
$spawnSpeciesResolvedPalFlagExact = 0
$spawnSpeciesResolvedPalFlagCaseFolded = 0
$spawnSpeciesUnresolvedCatalogIds = New-OrdinalSet
$spawnSpeciesUnresolvedPalFlagIds = New-OrdinalSet
$spawnRuleCount = 0
$spawnDayRules = 0
$spawnNightRules = 0
$spawnUndefinedTimeRules = 0
foreach ($speciesIdObject in $spawnSpeciesKeys) {
    $speciesId = [string]$speciesIdObject
    if ($catalogKeysExact.Contains($speciesId)) { $spawnSpeciesResolvedCatalogExact++ }
    if ($catalogKeysIgnoreCase.Contains($speciesId)) { $spawnSpeciesResolvedCatalogCaseFolded++ }
    if ($palKeysExact.Contains($speciesId)) { $spawnSpeciesResolvedPalFlagExact++ }
    if ($palKeysIgnoreCase.Contains($speciesId)) { $spawnSpeciesResolvedPalFlagCaseFolded++ }
    if (-not $catalogKeysExact.Contains($speciesId)) { [void]$spawnSpeciesUnresolvedCatalogIds.Add($speciesId) }
    if (-not $palKeysExact.Contains($speciesId)) { [void]$spawnSpeciesUnresolvedPalFlagIds.Add($speciesId) }
    foreach ($rule in @($spawnsRoot["species"][$speciesId])) {
        $spawnRuleCount++
        $time = [string]$rule[3]
        switch ($time) {
            "Day" { $spawnDayRules++ }
            "Night" { $spawnNightRules++ }
            default { $spawnUndefinedTimeRules++ }
        }
    }
}

$breedingRules = @($breedingRoot["Breeding"])
$breedingSpecies = New-OrdinalSet
$breedingUnresolvedExact = New-OrdinalSet
$breedingUnresolvedCaseFolded = New-OrdinalSet
$breedingCanonicalCounts = @{}
$genderSpecificBreedingRules = 0
foreach ($rule in $breedingRules) {
    $parent1 = [string]$rule["Parent1InternalName"]
    $parent2 = [string]$rule["Parent2InternalName"]
    $child = [string]$rule["ChildInternalName"]
    $gender1 = [string]$rule["Parent1Gender"]
    $gender2 = [string]$rule["Parent2Gender"]
    foreach ($speciesId in @($parent1, $parent2, $child)) {
        [void]$breedingSpecies.Add($speciesId)
        if (-not $palKeysExact.Contains($speciesId)) { [void]$breedingUnresolvedExact.Add($speciesId) }
        if (-not $palKeysIgnoreCase.Contains($speciesId)) { [void]$breedingUnresolvedCaseFolded.Add($speciesId) }
    }
    if ($gender1 -ne "WILDCARD" -or $gender2 -ne "WILDCARD") { $genderSpecificBreedingRules++ }

    $left = "$parent1|$gender1"
    $right = "$parent2|$gender2"
    if ([string]::CompareOrdinal($left, $right) -gt 0) {
        $swap = $left
        $left = $right
        $right = $swap
    }
    $canonicalKey = "$left+$right=>$child"
    if (-not $breedingCanonicalCounts.ContainsKey($canonicalKey)) {
        $breedingCanonicalCounts[$canonicalKey] = 0
    }
    $breedingCanonicalCounts[$canonicalKey]++
}
$symmetricDuplicateRecords = 0
$duplicateCanonicalGroups = 0
foreach ($count in $breedingCanonicalCounts.Values) {
    if ($count -gt 1) {
        $duplicateCanonicalGroups++
        $symmetricDuplicateRecords += $count - 1
    }
}

$assetDecodeErrors = @($assetRows | Where-Object { Test-Text $_.decode_error })
$duplicateAssetGroups = @(
    $assetRows |
        Group-Object -Property { $_.sha256 } |
        Where-Object { $_.Count -gt 1 } |
        ForEach-Object {
            [ordered]@{
                sha256 = $_.Name
                count = $_.Count
                paths = @($_.Group | ForEach-Object { $_.path } | Sort-Object)
            }
        } |
        Sort-Object sha256
)
$assetClassSummary = @(
    $assetRows |
        Group-Object -Property { $_.class } |
        ForEach-Object {
            $group = @($_.Group)
            [ordered]@{
                class = $_.Name
                file_count = $_.Count
                total_bytes = [long](($group | ForEach-Object { [long]$_.size_bytes } | Measure-Object -Sum).Sum)
                decode_error_count = @($group | Where-Object { Test-Text $_.decode_error }).Count
                transparency_used_count = @($group | Where-Object { $_.uses_transparency -eq $true }).Count
                build_verified_count = @($group | Where-Object { $_.build_verified -eq $true }).Count
            }
        } |
        Sort-Object class
)

$sourceRows = @()
$sourceDefinitions = @(
    @("pal_static_catalog", $paths.pals, $false, $null, "Pinned psp-core static catalog; exact game build is not proven."),
    @("active_skill_static_catalog", $paths.active_skills, $false, $null, "Pinned psp-core static catalog; exact game build is not proven."),
    @("passive_skill_static_catalog", $paths.passive_skills, $false, $null, "Pinned psp-core static catalog; exact game build is not proven."),
    @("korean_pal_catalog", $paths.pal_l10n_ko, $true, "steam:24181527", "Reviewed Korean catalog extraction."),
    @("korean_active_skill_catalog", $paths.active_l10n_ko, $true, "steam:24181527", "Reviewed Korean catalog extraction."),
    @("korean_passive_skill_catalog", $paths.passive_l10n_ko, $true, "steam:24181527", "Reviewed Korean catalog extraction."),
    @("item_catalog", $paths.items, $true, "steam:24181527", "Reviewed item catalog extraction; byte-identical re-extraction."),
    @("world_catalog", $paths.world, $true, "steam:24181527", "Reviewed world catalog extraction; byte-identical re-extraction."),
    @("spawn_search_index", $paths.spawns, $true, "steam:24181527", "Reviewed spawn extraction and deterministic search index."),
    @("breeding_rules", $paths.breeding, $false, $null, "Pinned PalCalc snapshot; exact game build is not proven."),
    @("item_icon_manifest", $paths.item_icon_manifest, $true, "steam:24181527", "Reviewed icon contract; byte-identical re-extraction."),
    @("game_icon_manifest", $paths.game_icon_manifest, $true, "steam:24181527", "Reviewed local-only extraction of 589 installed-game icons; extracted PNG files are not committed."),
    @("pal_icon_source_links", $paths.pal_icon_source_links, $true, "steam:24181527", "Reviewed links from 753 Pal parameter rows and 674 character icon table rows."),
    @("game_icon_korean_match_report", $paths.game_icon_match_report, $true, "steam:24181527", "Installed-game table links resolve 308 single portraits plus one exact two-member localization group; 2 records have no Pal portrait."),
    @("raid_display_icon_links", $paths.raid_display_icon_links, $true, "steam:24181527", "Exact raid-table and altar-UI links resolve the Moon Lord normal/master display icons to the installed-game Celestial Sigil item icons; one spawned sub-entity remains without a standalone UI icon.")
)
foreach ($definition in $sourceDefinitions) {
    $file = Get-Item -LiteralPath $definition[1]
    $sourceRows += ,([ordered]@{
        id = $definition[0]
        path = Convert-ToRepoPath -AbsolutePath $file.FullName
        size_bytes = $file.Length
        sha256 = Get-Sha256 -Path $file.FullName
        verified = [bool]$definition[2]
        game_build_id = $definition[3]
        note = $definition[4]
    })
}

$repositoryMirrorPairs = @(
    @("pals", $paths.pals, $paths.parser_pals),
    @("active_skills", $paths.active_skills, $paths.parser_active_skills),
    @("passive_skills", $paths.passive_skills, $paths.parser_passive_skills),
    @("items", $paths.items, $paths.parser_items),
    @("world", $paths.world, $paths.parser_world),
    @("pal_l10n_ko", $paths.pal_l10n_ko, $paths.parser_pal_l10n_ko),
    @("active_l10n_ko", $paths.active_l10n_ko, $paths.parser_active_l10n_ko),
    @("passive_l10n_ko", $paths.passive_l10n_ko, $paths.parser_passive_l10n_ko)
)
$repositoryMirrorIntegrity = @(
    foreach ($pair in $repositoryMirrorPairs) {
        $appHash = Get-Sha256 -Path $pair[1]
        $parserHash = Get-Sha256 -Path $pair[2]
        [ordered]@{
            id = $pair[0]
            app_path = Convert-ToRepoPath -AbsolutePath $pair[1]
            parser_path = Convert-ToRepoPath -AbsolutePath $pair[2]
            app_sha256 = $appHash
            parser_sha256 = $parserHash
            byte_identical = $appHash -eq $parserHash
        }
    }
)

$currentInstallEvidence = [ordered]@{
    steam_appmanifest_exists = $false
    steam_appmanifest_current_sha256 = $null
    steam_appmanifest_sha256_matches = $null
    steam_appmanifest_build_id = $null
    steam_appmanifest_target_build_id = $null
    steam_appmanifest_build_identity_matches = $null
    launcher_exists = $false
    launcher_sha256_matches = $null
    main_pak_exists = $false
    main_pak_size_matches = $null
    mappings_exists = $false
    mappings_sha256_matches = $null
}
$installed = $sourceProbe["installed_game"]
$appManifestPath = [string]$installed["steam_appmanifest_path"]
if (Test-Path -LiteralPath $appManifestPath -PathType Leaf) {
    $currentInstallEvidence.steam_appmanifest_exists = $true
    $currentAppManifestSha = Get-Sha256 -Path $appManifestPath
    $currentAppManifestText = [IO.File]::ReadAllText($appManifestPath)
    $buildMatch = [regex]::Match($currentAppManifestText, '(?m)^\s*"buildid"\s+"(?<id>\d+)"')
    $targetBuildMatch = [regex]::Match($currentAppManifestText, '(?m)^\s*"TargetBuildID"\s+"(?<id>\d+)"')
    $currentInstallEvidence.steam_appmanifest_current_sha256 = $currentAppManifestSha
    $currentInstallEvidence.steam_appmanifest_sha256_matches =
        $currentAppManifestSha -eq [string]$installed["steam_appmanifest_sha256"]
    if ($buildMatch.Success) { $currentInstallEvidence.steam_appmanifest_build_id = $buildMatch.Groups["id"].Value }
    if ($targetBuildMatch.Success) { $currentInstallEvidence.steam_appmanifest_target_build_id = $targetBuildMatch.Groups["id"].Value }
    $expectedBuildId = ([string]$installed["game_build_id"]).Replace("steam:", "")
    $currentInstallEvidence.steam_appmanifest_build_identity_matches =
        $buildMatch.Success -and
        $targetBuildMatch.Success -and
        $buildMatch.Groups["id"].Value -eq $expectedBuildId -and
        $targetBuildMatch.Groups["id"].Value -eq $expectedBuildId
}
$launcherPath = Join-Path ([string]$installed["install_path"]) "Palworld.exe"
if (Test-Path -LiteralPath $launcherPath -PathType Leaf) {
    $currentInstallEvidence.launcher_exists = $true
    $currentInstallEvidence.launcher_sha256_matches =
        (Get-Sha256 -Path $launcherPath) -eq [string]$installed["launcher_sha256"]
}
$pakPath = Join-Path ([string]$installed["install_path"]) ([string]$installed["main_pak"]["path"])
if (Test-Path -LiteralPath $pakPath -PathType Leaf) {
    $currentInstallEvidence.main_pak_exists = $true
    $currentInstallEvidence.main_pak_size_matches =
        (Get-Item -LiteralPath $pakPath).Length -eq [long]$installed["main_pak"]["size_bytes"]
}
$mappingPath = [string]$sourceProbe["mappings"]["path_observed_read_only"]
if (Test-Path -LiteralPath $mappingPath -PathType Leaf) {
    $currentInstallEvidence.mappings_exists = $true
    $currentInstallEvidence.mappings_sha256_matches =
        (Get-Sha256 -Path $mappingPath) -eq [string]$sourceProbe["mappings"]["sha256"]
}

$referenceIntegrity = @(
    [ordered]@{ relation = "pal.active_skill"; references = $activeSkillLinks; unresolved_exact = $activeSkillLinkUnresolved; unresolved_ids = @($activeSkillLinkUnresolvedIds | Sort-Object) }
    [ordered]@{ relation = "pal.passive_skill"; references = $passiveSkillLinks; unresolved_exact = $passiveSkillLinkUnresolved; unresolved_ids = @($passiveSkillLinkUnresolvedIds | Sort-Object) }
    [ordered]@{ relation = "recipe.output_item"; references = $recipeOutputRefs; unresolved_exact = $recipeOutputUnresolved }
    [ordered]@{ relation = "recipe.ingredient_item"; references = $ingredientRefs; unresolved_exact = $ingredientUnresolved }
    [ordered]@{ relation = "recipe.unlock_item"; references = $unlockRefs; unresolved_exact = $unlockUnresolved }
    [ordered]@{ relation = "pal_drop.pal"; references = $dropPalRefs; unresolved_to_all_static_catalog_exact = $dropPalUnresolvedCatalogExact; unresolved_to_all_static_catalog_case_insensitive = $dropPalUnresolvedCatalogCaseFolded; unresolved_to_all_static_catalog_distinct_ids = @($dropPalUnresolvedCatalogIds | Sort-Object); unresolved_to_pal_flag_catalog_exact = $dropPalUnresolvedPalFlagExact; unresolved_to_pal_flag_catalog_case_insensitive = $dropPalUnresolvedPalFlagCaseFolded; unresolved_to_pal_flag_catalog_distinct_ids = @($dropPalUnresolvedPalFlagIds | Sort-Object) }
    [ordered]@{ relation = "pal_drop.item"; references = $dropItemRefs; unresolved_exact = $dropItemUnresolved }
    [ordered]@{ relation = "production.item"; references = $productionItemRefs; unresolved_exact = $productionItemUnresolved }
    [ordered]@{ relation = "production.building"; references = $productionBuildingRefs; unresolved_exact = $productionBuildingUnresolved; unresolved_ids = @($productionBuildingUnresolvedIds | Sort-Object) }
    [ordered]@{ relation = "shop.product_item"; references = $shopItemRefs; unresolved_exact = $shopItemUnresolved }
    [ordered]@{ relation = "shop.currency_item"; references = $shopCurrencyRefs; unresolved_exact = $shopCurrencyUnresolved }
    [ordered]@{ relation = "spawn.species"; references = $spawnSpeciesKeys.Count; unresolved_to_all_static_catalog_exact = $spawnSpeciesKeys.Count - $spawnSpeciesResolvedCatalogExact; unresolved_to_all_static_catalog_case_insensitive = $spawnSpeciesKeys.Count - $spawnSpeciesResolvedCatalogCaseFolded; unresolved_to_all_static_catalog_distinct_ids = @($spawnSpeciesUnresolvedCatalogIds | Sort-Object); unresolved_to_pal_flag_catalog_exact = $spawnSpeciesKeys.Count - $spawnSpeciesResolvedPalFlagExact; unresolved_to_pal_flag_catalog_case_insensitive = $spawnSpeciesKeys.Count - $spawnSpeciesResolvedPalFlagCaseFolded; unresolved_to_pal_flag_catalog_distinct_ids = @($spawnSpeciesUnresolvedPalFlagIds | Sort-Object) }
    [ordered]@{ relation = "breeding.species_distinct"; references = $breedingSpecies.Count; unresolved_exact = $breedingUnresolvedExact.Count; unresolved_case_insensitive = $breedingUnresolvedCaseFolded.Count; unresolved_ids = @($breedingUnresolvedCaseFolded | Sort-Object) }
)

$issues = @(
    [ordered]@{ id = "PAL_SOURCE_BUILD_UNPROVEN"; severity = "blocker"; area = "pal"; summary = "The 809-row static catalog has no proven Build 24181527 identity; the installed game table has 753 rows." }
    [ordered]@{ id = "PAL_EXACT_NORMALIZATION_MISSING"; severity = "blocker"; area = "pal"; summary = "DT_PalMonsterParameter and DT_WazaMasterLevel are probeable but have no reviewed normalized extractor/output." }
    [ordered]@{ id = "PAL_ENGLISH_MISSING"; severity = "high"; area = "pal"; summary = "Exact English localization paths exist in the game, but no English pal names or descriptions are normalized in the repository." }
    [ordered]@{ id = "PARTNER_SKILLS_MISSING"; severity = "high"; area = "pal"; summary = "Partner-skill tables and localization paths are known, but links, values, text, and icons are not normalized." }
    [ordered]@{ id = "GAME_ICON_SEMANTIC_MAPPINGS_PENDING"; severity = "high"; area = "assets"; summary = "Element, work-suitability, skill, rarity, and item-category icons are exact-Build assets, but their numeric logical IDs are not yet joined to reviewed game data enums/tables." }
    [ordered]@{ id = "ITEM_LEGAL_ICONS_MISSING"; severity = "medium"; area = "item"; summary = "$(@($itemIconManifest["legal_missing_icon_names"]).Count) legal logical item icons are not resolved by the reviewed icon contract." }
    [ordered]@{ id = "RECIPE_FACILITY_FIELDS_OMITTED"; severity = "high"; area = "item"; summary = "Recipe WorkableAttribute, EnergyAmount, and EnergyType exist in the source table but are omitted from items.v1.json." }
    [ordered]@{ id = "ITEM_ACQUISITION_RELATIONS_INCOMPLETE"; severity = "high"; area = "item"; summary = "Lottery, ranch, expedition, fishing, salvage, merchant identity/location, and other acquisition relations are not fully normalized." }
    [ordered]@{ id = "BREEDING_SOURCE_BUILD_UNPROVEN"; severity = "blocker"; area = "breeding"; summary = "The 44,851 PalCalc rules have no proven Build 24181527 identity." }
    [ordered]@{ id = "SPECIAL_BREEDING_NOT_NORMALIZED"; severity = "blocker"; area = "breeding"; summary = "The exact DT_PalCombiUnique table has 258 rows, but no reviewed normalized special-breeding output exists." }
    [ordered]@{ id = "SPAWN_CASE_COLLISION"; severity = "high"; area = "spawn"; summary = "The spawn species index contains case-insensitive ID collisions and cannot be parsed by case-insensitive JSON object parsers without loss." }
    [ordered]@{ id = "SPAWN_REGION_LOCALIZATION_MISSING"; severity = "medium"; area = "spawn"; summary = "Spawn groups and coordinates are verified, but user-facing region names are not normalized." }
    [ordered]@{ id = "WEB_ASSET_RIGHTS_UNRESOLVED"; severity = "blocker"; area = "distribution"; summary = "Game-derived visual assets must not be selected for Web/PWA redistribution until rights are separately confirmed." }
)

$generatedAtUtc = [DateTime]::UtcNow.ToString("o")
$localizationMatches = @(
    Get-IdMatchReport `
        -Id "pal_localization_to_active_pal_catalog" `
        -SourceIds @($palL10n.Keys) `
        -TargetIds @($palRows | ForEach-Object { [string]$_.Key })
    Get-IdMatchReport `
        -Id "active_pal_catalog_to_pal_localization" `
        -SourceIds @($palRows | ForEach-Object { [string]$_.Key }) `
        -TargetIds @($palL10n.Keys)
    Get-IdMatchReport `
        -Id "active_skill_localization_to_static_catalog" `
        -SourceIds @($activeL10n.Keys) `
        -TargetIds @($activeSkills.Keys)
    Get-IdMatchReport `
        -Id "static_active_skill_to_localization" `
        -SourceIds @($activeSkills.Keys) `
        -TargetIds @($activeL10n.Keys)
    Get-IdMatchReport `
        -Id "passive_skill_localization_to_static_catalog" `
        -SourceIds @($passiveL10n.Keys) `
        -TargetIds @($passiveSkills.Keys)
    Get-IdMatchReport `
        -Id "static_passive_skill_to_localization" `
        -SourceIds @($passiveSkills.Keys) `
        -TargetIds @($passiveL10n.Keys)
    Get-IdMatchReport `
        -Id "breeding_species_to_pal_localization" `
        -SourceIds @($breedingSpecies) `
        -TargetIds @($palL10n.Keys)
    Get-IdMatchReport `
        -Id "spawn_species_to_pal_localization" `
        -SourceIds $spawnSpeciesKeys `
        -TargetIds @($palL10n.Keys)
    Get-IdMatchReport `
        -Id "drop_pal_ids_to_pal_localization" `
        -SourceIds @($dropPalIds) `
        -TargetIds @($palL10n.Keys)
)

$verifiedLegalItemKoreanNameIds = @(
    $legalItemRows |
        Where-Object {
            (Test-Text $_["name_ko"]) -and $_["localization_fallback"] -ne $true
        } |
        ForEach-Object { [string]$_["item_id"] } |
        Sort-Object
)

$localizationMatchReport = [ordered]@{
    schema_version = 1
    generated_at_utc = $generatedAtUtc
    game_build_id = "steam:24181527"
    policy = [ordered]@{
        primary_locale = "ko"
        display_name_precedence = @(
            "verified Korean game localization",
            "explicit internal-ID fallback with an unlocalized badge"
        )
        case_only_match_behavior = "accept only a unique exact-Build FName-style case-only match, record it separately, and fail on collisions"
        unmatched_behavior = "keep the entity addressable by internal ID and show it in the unmatched report"
        unverified_numeric_catalog_behavior = "Korean text may be shown, but unverified static numeric fields do not become verified through localization matching"
    }
    items = [ordered]@{
        legal_item_count = $legalItemRows.Count
        verified_korean_name_count = $verifiedLegalItemKoreanNameIds.Count
        verified_korean_name_ids = $verifiedLegalItemKoreanNameIds
        internal_id_name_fallback_count = $legalItemLocalizationFallbackIds.Count
        internal_id_name_fallback_ids = $legalItemLocalizationFallbackIds
        missing_korean_description_count = $legalItemMissingKoreanDescriptionIds.Count
        missing_korean_description_ids = $legalItemMissingKoreanDescriptionIds
        unresolved_dynamic_markup_description_count = $legalItemDescriptionDynamicMarkupGapIds.Count
        unresolved_dynamic_markup_description_ids = $legalItemDescriptionDynamicMarkupGapIds
    }
    id_matches = $localizationMatches
}

$dataReport = [ordered]@{
    schema_version = 1
    generated_at_utc = $generatedAtUtc
    repository = [ordered]@{
        commit = $gitCommit
        branch = $gitBranch
        audit_script = "scripts/audit-palbeacon-data.ps1"
    }
    scope = [ordered]@{
        ui_implementation_started = $false
        game_files_modified = $false
        save_files_read = $false
        overlay_files_modified = $false
        external_sites_used_as_operational_data = $false
    }
    installed_source_probe = $sourceProbe
    current_install_evidence = $currentInstallEvidence
    pinned_provider_ledger = $parserLock
    sources = $sourceRows
    repository_mirror_integrity = $repositoryMirrorIntegrity
    localization_policy = $localizationMatchReport.policy
    localization_match_report = "docs/data/LOCALIZATION_MATCH_REPORT.json"
    datasets = [ordered]@{
        pals = [ordered]@{
            static_catalog_record_count = $palEntries.Count
            active_pal_flag_record_count = $palRows.Count
            positive_paldex_number_record_count = $palDeckRows.Count
            exact_game_table_probe_row_count = 753
            static_catalog_verified = $false
            korean_localization_record_count = $palL10n.Count
            korean_exact_id_join_count = $palKoreanExact
            korean_case_insensitive_join_count = $palKoreanCaseFolded
            korean_exact_id_join_missing_ids = $palKoreanExactJoinMissingIds
            korean_case_only_join_ids = $palKoreanCaseOnlyJoinIds
            pal_icon_file_count = $palIconAssets.Count
            pal_icon_resolved_for_static_pal_rows = $palIconResolved
            pal_icon_resolved_for_positive_paldex_rows = $palDeckIconResolved
            pal_icon_resolved_for_localized_ids = $localizedPalIconResolved
            pal_icon_multi_entity_localization_group_count = $localizedPalIconGroupResolved
            localized_pal_missing_icon_ids = $localizedPalMissingIconIds
            reviewed_game_icon_candidate_count = [int]$gameIconMatchReport["pal_portrait_candidate_count"]
            reviewed_game_icon_eligible_count = [int]$gameIconMatchReport["eligible_pal_icon_count"]
            reviewed_game_icon_exact_match_count = [int]$gameIconMatchReport["exact_match_count"]
            reviewed_game_icon_case_only_match_count = [int]$gameIconMatchReport["case_only_match_count"]
            reviewed_game_icon_localized_name_bridge_match_count = [int]$gameIconMatchReport["localized_name_bridge_match_count"]
            reviewed_game_icon_multi_group_member_count = [int]$gameIconMatchReport["multi_icon_group_member_count"]
            reviewed_game_icon_unlocalized_candidate_count = [int]$gameIconMatchReport["icon_without_korean_localization_count"]
            raid_display_icon_variant_count = [int]$raidDisplayIconLinks["summary"]["raid_display_icon_link_count"]
            localized_pal_missing_exact_display_icon_ids = @(
                $raidDisplayIconLinks["unresolved"] |
                    ForEach-Object { [string]$_["display_entity_id"] } |
                    Sort-Object
            )
            field_coverage = $palFieldCoverage
            catalog_case_collisions = @(Get-CaseInsensitiveCollisions -Keys @($pals.Keys))
            localization_case_collisions = @(Get-CaseInsensitiveCollisions -Keys @($palL10n.Keys))
        }
        skills = [ordered]@{
            static_active_skill_count = $activeSkills.Count
            exact_active_skill_probe_row_count = 384
            korean_active_skill_count = $activeL10n.Count
            static_passive_skill_count = $passiveSkills.Count
            exact_passive_skill_probe_row_count = 1905
            korean_passive_skill_count = $passiveL10n.Count
            exact_pal_active_skill_link_probe_row_count = 5772
            partner_skill_normalized_count = 0
            static_catalogs_verified = $false
        }
        items = [ordered]@{
            total_item_count = $itemRows.Count
            legal_item_count = $legalItemRows.Count
            verified = [bool]$itemsRoot["verified"]
            recipe_count = @($itemsRoot["recipes"]).Count
            pal_drop_relation_count = @($itemsRoot["pal_drops"]).Count
            nonzero_drop_source_pal_count = $dropPalIds.Count
            field_coverage_legal_items = $itemFieldCoverage
            localization_fallback_count_all_items = @($itemRows | Where-Object { $_["localization_fallback"] -eq $true }).Count
            legal_localization_fallback_ids = $legalItemLocalizationFallbackIds
            legal_items_missing_korean_description_ids = $legalItemMissingKoreanDescriptionIds
            legal_items_with_unresolved_dynamic_description_markup_ids = $legalItemDescriptionDynamicMarkupGapIds
            case_only_localization_match_report = "docs/data/ITEM_LOCALIZATION_MATCHES.24181527.json"
            case_only_localization_key_count = [int]$itemLocalizationMatches["case_only_key_count"]
            case_only_localization_collision_count = [int]$itemLocalizationMatches["collision_count"]
            used_as_recipe_ingredient_distinct_item_count = $usedAsIngredientItems.Count
            pal_drop_distinct_item_count = $dropItemIds.Count
            production_distinct_item_count = $productionItemIds.Count
            shop_distinct_item_count = $shopItemIds.Count
            item_icon_logical_manifest_count = @($itemIconManifest["icons"]).Count
            item_icon_unique_file_count = @($assetRows | Where-Object { $_.class -eq "game_item_icon" }).Count
            item_icon_manifest_missing_file_paths = $itemManifestMissingFilePaths
            item_icon_manifest_hash_mismatch_paths = $itemManifestHashMismatchPaths
            legal_missing_icon_names = @($itemIconManifest["legal_missing_icon_names"])
        }
        world_item_relations = [ordered]@{
            verified = [bool]$worldRoot["verified"]
            building_count = @($worldRoot["buildings"]).Count
            technology_count = @($worldRoot["technologies"]).Count
            production_method_count = @($worldRoot["production_methods"]).Count
            shop_group_count = @($worldRoot["shop_groups"]).Count
            shop_pool_count = @($worldRoot["shop_pools"]).Count
            building_icon_manifest_sha256 = $buildingIconManifestHash
            building_icon_manifest_count = @($buildingIconManifest["icons"]).Count
            building_icon_manifest_missing_file_paths = $buildingManifestMissingFilePaths
            building_icon_manifest_hash_mismatch_paths = $buildingManifestHashMismatchPaths
        }
        breeding = [ordered]@{
            rule_count = $breedingRules.Count
            min_breeding_steps_target_count = $breedingRoot["MinBreedingSteps"].Count
            distinct_species_count = $breedingSpecies.Count
            gender_specific_rule_count = $genderSpecificBreedingRules
            canonical_parent_pair_result_count = $breedingCanonicalCounts.Count
            canonical_duplicate_group_count = $duplicateCanonicalGroups
            symmetric_duplicate_record_count = $symmetricDuplicateRecords
            exact_special_breeding_probe_row_count = 258
            exact_special_breeding_normalized_count = 0
            verified = $false
        }
        spawns = [ordered]@{
            verified = [bool]$spawnsRoot["verified"]
            placement_count = [int]$spawnsRoot["placement_count"]
            resolved_placement_count = [int]$spawnsRoot["resolved_placement_count"]
            spawn_group_count = [int]$spawnsRoot["spawn_group_count"]
            species_count = $spawnSpeciesKeys.Count
            rule_count = $spawnRuleCount
            day_rule_count = $spawnDayRules
            night_rule_count = $spawnNightRules
            unspecified_time_rule_count = $spawnUndefinedTimeRules
            species_case_collisions = @(Get-CaseInsensitiveCollisions -Keys $spawnSpeciesKeys)
        }
    }
    reference_integrity = $referenceIntegrity
    asset_summary = [ordered]@{
        tracked_image_count = $assetRows.Count
        tracked_image_bytes = [long](($assetRows | ForEach-Object { [long]$_.size_bytes } | Measure-Object -Sum).Sum)
        decode_error_count = $assetDecodeErrors.Count
        duplicate_hash_group_count = $duplicateAssetGroups.Count
        classes = $assetClassSummary
        local_game_icon_pack = [ordered]@{
            manifest_path = "docs/data/GAME_ICON_MANIFEST.24181527.json"
            manifest_sha256 = $gameIconManifestHash
            distribution_scope = [string]$gameIconManifest["distribution_scope"]
            original_icon_count = @($gameIconManifest["icons"]).Count
            derived_thumbnail_count = @($gameIconManifest["icons"]).Count
            extracted_png_committed = $false
            eligible_korean_pal_icon_count = [int]$gameIconMatchReport["eligible_pal_icon_count"]
            korean_pal_multi_icon_group_count = [int]$gameIconMatchReport["multi_icon_group_match_count"]
            korean_pal_multi_icon_group_member_count = [int]$gameIconMatchReport["multi_icon_group_member_count"]
            korean_pal_icon_missing_count = [int]$gameIconMatchReport["korean_localization_without_icon_count"]
            pal_icon_source_link_sha256 = $palIconSourceLinksHash
            raid_display_icon_variant_count = [int]$raidDisplayIconLinks["summary"]["raid_display_icon_link_count"]
            remaining_without_exact_display_icon_count = [int]$raidDisplayIconLinks["summary"]["remaining_without_exact_standalone_display_icon_count"]
            raid_display_icon_report_path = "docs/data/RAID_DISPLAY_ICON_LINKS.24181527.json"
            raid_display_icon_report_sha256 = $raidDisplayIconLinksHash
        }
    }
    verified_displayable_now = [ordered]@{
        pals = @(
            "Korean game names and descriptions from the reviewed Build 24181527 localization extraction.",
            "Build 24181527 spawn coordinates, group IDs, encounter categories, levels, counts, and day/night flags.",
            "Build 24181527 pal-to-item drop relations, subject to unresolved pal catalog identity joins being shown as unverified.",
            "308 installed-game Pal portraits plus one exact two-member localization group joined through the character-icon and parameter tables; 2 raid records have no Pal portrait.",
            "Moon Lord normal/master use 2 exact installed-game Celestial Sigil icons through the raid-table and altar-UI item-icon flow; only the spawned True Eye sub-entity has no exact standalone display icon."
        )
        items = @(
            "Internal ID, Korean name and description, TypeA/TypeB category, rarity, weight, price, stack limit, rank, legal flag.",
            "Recipe outputs, quantities, ingredients, work amount, and unlock item.",
            "Pal drops, production building links, shop group products, currency, prices, quantities, product type, and stock.",
            "Reviewed item icons with original package path, source dimensions, output hash, and match method."
        )
        breeding = @()
    }
    observed_but_not_verified_for_display = [ordered]@{
        pals = @(
            "Paldex number, elements, base stats, work suitability, nocturnal flag, movement speeds, stamina, rarity, active/passive skill assignments, boss flags, and breeding power from the pinned static catalog.",
            "Legacy bundled Pal portraits and element icons from the private benchmark asset package.",
            "Numeric semantic mappings for the reviewed element, work-suitability, skill, rarity, and item-category icon sets."
        )
        skills = @(
            "Active skill element/type/power/range/cooldown/effects and passive skill rank/effects/invocation flags from pinned static catalogs."
        )
        breeding = @(
            "Parent A plus parent B to child rules and MinBreedingSteps from the pinned PalCalc snapshot."
        )
    }
    issues = $issues
}

$assetReport = [ordered]@{
    schema_version = 1
    generated_at_utc = $dataReport.generated_at_utc
    repository_commit = $gitCommit
    game_build_id_for_verified_assets = "steam:24181527"
    summary = $dataReport.asset_summary
    duplicate_hash_groups = $duplicateAssetGroups
    assets = $assetRows
}

$localizationMarkdown = New-Object Text.StringBuilder
[void]$localizationMarkdown.AppendLine("# Korean localization match and unmatched report")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("Source Build: Steam 24181527")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("Display names prefer verified Korean game localization. When Korean text is unavailable, show the internal ID as an explicit fallback with an unlocalized badge. Pal and skill case-only matches remain review items. Exact-Build item keys may use a unique case-only match recorded in ITEM_LOCALIZATION_MATCHES.24181527.json; any collision fails extraction.")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("## Summary")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("| Relation | Source | Exact | Case-only | Unmatched |")
[void]$localizationMarkdown.AppendLine("|---|---:|---:|---:|---:|")
foreach ($match in $localizationMatches) {
    [void]$localizationMarkdown.AppendLine(
        "| $($match.id) | $($match.source_count) | $($match.exact_match_count) | $($match.case_only_match_count) | $($match.unmatched_count) |"
    )
}
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("Items: $($legalItemRows.Count) legal records, $($verifiedLegalItemKoreanNameIds.Count) verified Korean names, $($legalItemLocalizationFallbackIds.Count) internal-ID name fallbacks, $($legalItemMissingKoreanDescriptionIds.Count) missing Korean descriptions, and $($legalItemDescriptionDynamicMarkupGapIds.Count) descriptions with unresolved dynamic quality markup.")

foreach ($match in $localizationMatches) {
    if ($match.case_only_match_count -eq 0 -and $match.unmatched_count -eq 0) {
        continue
    }
    [void]$localizationMarkdown.AppendLine()
    [void]$localizationMarkdown.AppendLine("## $($match.id)")
    if ($match.case_only_match_count -gt 0) {
        [void]$localizationMarkdown.AppendLine()
        [void]$localizationMarkdown.AppendLine("### Case-only matches")
        [void]$localizationMarkdown.AppendLine()
        [void]$localizationMarkdown.AppendLine('```text')
        foreach ($caseMatch in $match.case_only_matches) {
            [void]$localizationMarkdown.AppendLine(
                "$($caseMatch.source_id) -> $(@($caseMatch.target_candidates) -join ', ')"
            )
        }
        [void]$localizationMarkdown.AppendLine('```')
    }
    if ($match.unmatched_count -gt 0) {
        [void]$localizationMarkdown.AppendLine()
        [void]$localizationMarkdown.AppendLine("### Unmatched IDs")
        [void]$localizationMarkdown.AppendLine()
        [void]$localizationMarkdown.AppendLine('```text')
        foreach ($unmatchedId in $match.unmatched_ids) {
            [void]$localizationMarkdown.AppendLine([string]$unmatchedId)
        }
        [void]$localizationMarkdown.AppendLine('```')
    }
}

[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("## Item Korean-name fallbacks")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine('```text')
foreach ($itemId in $legalItemLocalizationFallbackIds) {
    [void]$localizationMarkdown.AppendLine([string]$itemId)
}
[void]$localizationMarkdown.AppendLine('```')
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("## Missing item Korean descriptions")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine('```text')
foreach ($itemId in $legalItemMissingKoreanDescriptionIds) {
    [void]$localizationMarkdown.AppendLine([string]$itemId)
}
[void]$localizationMarkdown.AppendLine('```')
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("## Item descriptions with unresolved dynamic quality markup")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("These rows have a verified Korean source string, but a runtime quality placeholder currently renders as empty parentheses. The extractor must resolve the exact game quality display value before UI presentation; no grade text is inferred here.")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("Count: $($legalItemDescriptionDynamicMarkupGapIds.Count). The machine-readable ID list is in `LOCALIZATION_MATCH_REPORT.json`.")
[void]$localizationMarkdown.AppendLine()
[void]$localizationMarkdown.AppendLine("This report describes ID and localization linkage only. Static Pal or skill values and PalCalc breeding rules do not become verified merely because their IDs match Korean localization.")

$dataOutputPath = Join-Path $OutputDirectory "DATA_INVENTORY.json"
$assetOutputPath = Join-Path $OutputDirectory "ASSET_INVENTORY.json"
$localizationJsonOutputPath = Join-Path $OutputDirectory "LOCALIZATION_MATCH_REPORT.json"
$localizationMarkdownOutputPath = Join-Path $OutputDirectory "LOCALIZATION_UNMATCHED.md"
$utf8NoBom = New-Object Text.UTF8Encoding($false)
[IO.File]::WriteAllText($dataOutputPath, ($dataReport | ConvertTo-Json -Depth 30), $utf8NoBom)
[IO.File]::WriteAllText($assetOutputPath, ($assetReport | ConvertTo-Json -Depth 30), $utf8NoBom)
[IO.File]::WriteAllText($localizationJsonOutputPath, ($localizationMatchReport | ConvertTo-Json -Depth 30), $utf8NoBom)
[IO.File]::WriteAllText($localizationMarkdownOutputPath, $localizationMarkdown.ToString(), $utf8NoBom)

Write-Host "Wrote $(Convert-ToRepoPath -AbsolutePath $dataOutputPath)"
Write-Host "Wrote $(Convert-ToRepoPath -AbsolutePath $assetOutputPath)"
Write-Host "Wrote $(Convert-ToRepoPath -AbsolutePath $localizationJsonOutputPath)"
Write-Host "Wrote $(Convert-ToRepoPath -AbsolutePath $localizationMarkdownOutputPath)"
Write-Host "Pals: $($palRows.Count) static pal rows; exact installed table probe: 753; verified Korean: $($palL10n.Count)"
Write-Host "Items: $($itemRows.Count) total; $($legalItemRows.Count) legal; recipes: $(@($itemsRoot["recipes"]).Count)"
Write-Host "Breeding: $($breedingRules.Count) unverified rules; exact special rows pending normalization: 258"
Write-Host "Assets: $($assetRows.Count) tracked images; decode errors: $($assetDecodeErrors.Count); duplicate hash groups: $($duplicateAssetGroups.Count)"
