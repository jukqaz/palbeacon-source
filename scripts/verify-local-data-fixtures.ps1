[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Root,

    [switch] $Json
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RequiredCapabilities = @(
    'pals',
    'skills',
    'items',
    'recipes',
    'acquisition',
    'breeding',
    'technologies',
    'buildings',
    'locations',
    'pois',
    'aliases',
    'localization',
    'sources'
)
$AcquisitionKinds = @(
    'pal_drop',
    'gather',
    'craft',
    'merchant',
    'ranch',
    'expedition',
    'fishing',
    'salvage',
    'chest',
    'boss',
    'captured_cage'
)

function Assert-Condition {
    param(
        [bool] $Condition,
        [string] $Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Get-PropertyValue {
    param(
        [object] $Object,
        [string] $Name
    )

    $property = $Object.PSObject.Properties[$Name]
    if ($null -eq $property) {
        return $null
    }
    return $property.Value
}

function Assert-NoFloatingPoint {
    param(
        [object] $Value,
        [string] $Path
    )

    if ($null -eq $Value) {
        return
    }
    if (
        $Value -is [double] -or
        $Value -is [single] -or
        $Value -is [decimal]
    ) {
        throw "floating-point JSON number is forbidden at $Path"
    }
    if ($Value -is [string] -or $Value -is [bool] -or $Value -is [ValueType]) {
        return
    }
    if ($Value -is [System.Collections.IEnumerable]) {
        $index = 0
        foreach ($entry in $Value) {
            Assert-NoFloatingPoint -Value $entry -Path "$Path[$index]"
            $index++
        }
        return
    }
    foreach ($property in $Value.PSObject.Properties) {
        Assert-NoFloatingPoint -Value $property.Value -Path "$Path.$($property.Name)"
    }
}

function Read-Ndjson {
    param(
        [string] $Path,
        [int] $MaximumRows
    )

    $rows = [System.Collections.Generic.List[object]]::new()
    $lineNumber = 0
    foreach ($line in [System.IO.File]::ReadLines($Path)) {
        $lineNumber++
        Assert-Condition (
            [System.Text.Encoding]::UTF8.GetByteCount($line) -le 1MB
        ) "$Path line $lineNumber exceeds 1 MiB"
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        $row = $line | ConvertFrom-Json
        Assert-NoFloatingPoint -Value $row -Path "$Path[$lineNumber]"
        Assert-Condition ($row.schema_major -eq 1) "$Path line $lineNumber has wrong schema_major"
        Assert-Condition (
            $row.game_build_id -eq 'steam:24181527'
        ) "$Path line $lineNumber has wrong game_build_id"
        Assert-Condition (
            -not [string]::IsNullOrWhiteSpace($row.source_id)
        ) "$Path line $lineNumber has no source_id"
        Assert-Condition (
            $row.entity_version -eq 'steam:24181527'
        ) "$Path line $lineNumber has wrong entity_version"
        $rows.Add($row)
        Assert-Condition ($rows.Count -le $MaximumRows) "$Path exceeds its row bound"
    }
    return $rows.ToArray()
}

function Add-UniqueId {
    param(
        [System.Collections.Generic.HashSet[string]] $Set,
        [string] $Id,
        [string] $Kind
    )

    Assert-Condition (-not [string]::IsNullOrWhiteSpace($Id)) "$Kind ID is empty"
    Assert-Condition (
        [System.Text.Encoding]::ASCII.GetByteCount($Id) -le 128
    ) "$Kind ID exceeds 128 ASCII bytes: $Id"
    Assert-Condition ($Set.Add($Id)) "duplicate $Kind ID: $Id"
}

function Resolve-FixtureRoots {
    param([string] $RequestedRoot)

    $resolved = (Resolve-Path -LiteralPath $RequestedRoot).Path
    if (Test-Path -LiteralPath (Join-Path $resolved 'manifest.json') -PathType Leaf) {
        return [pscustomobject]@{
            LocalRoot = Split-Path -Parent $resolved
            CatalogRoot = $resolved
            IsCatalogOnly = $true
        }
    }
    $catalog = Join-Path $resolved 'catalog-synthetic-v1'
    Assert-Condition (
        (Test-Path -LiteralPath (Join-Path $catalog 'manifest.json') -PathType Leaf)
    ) "catalog fixture manifest is missing under $resolved"
    return [pscustomobject]@{
        LocalRoot = $resolved
        CatalogRoot = $catalog
        IsCatalogOnly = $false
    }
}

function Test-CatalogFixture {
    param([string] $CatalogRoot)

    $manifestPath = Join-Path $CatalogRoot 'manifest.json'
    $manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
    Assert-Condition ($manifest.schema_major -eq 1) 'catalog manifest schema_major must be 1'
    Assert-Condition (
        $manifest.game_build_id -eq 'steam:24181527'
    ) 'catalog manifest Build is not the exact synthetic Build'

    $capabilities = @($manifest.capabilities | ForEach-Object { $_.capability_id })
    Assert-Condition (
        $capabilities.Count -eq $RequiredCapabilities.Count
    ) 'catalog manifest must contain exactly 13 capabilities'
    Assert-Condition (
        (@($capabilities | Select-Object -Unique).Count) -eq $capabilities.Count
    ) 'catalog manifest contains duplicate capabilities'
    Assert-Condition (
        (($capabilities -join ',') -ceq ($RequiredCapabilities -join ','))
    ) 'catalog capability order or membership differs from the frozen contract'
    foreach ($capability in $manifest.capabilities) {
        Assert-Condition (
            @('complete', 'unsupported_by_build', 'failed') -contains $capability.status
        ) "invalid capability status: $($capability.status)"
    }

    Assert-Condition ($manifest.files.Count -le 32) 'catalog package exceeds 32 files'
    $rowsByFile = @{}
    foreach ($descriptor in $manifest.files) {
        $path = Join-Path $CatalogRoot $descriptor.path
        Assert-Condition (Test-Path -LiteralPath $path -PathType Leaf) "missing catalog file: $path"
        $file = Get-Item -LiteralPath $path
        Assert-Condition ($file.Length -le 256MB) "$path exceeds 256 MiB"
        $actualHash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        Assert-Condition ($actualHash -ceq $descriptor.sha256) "SHA-256 mismatch: $path"
        $maximumRows = switch ($descriptor.path) {
            'pals.ndjson' { 4096 }
            'skills.ndjson' { 65536 }
            'items.ndjson' { 131072 }
            'recipes.ndjson' { 131072 }
            'acquisition.ndjson' { 1048576 }
            'breeding.ndjson' { 1048576 }
            'world.ndjson' { 1048576 }
            'localization.ndjson' { 1048576 }
            'sources.ndjson' { 32 }
            default { throw "unexpected normalized file: $($descriptor.path)" }
        }
        $rows = @(Read-Ndjson -Path $path -MaximumRows $maximumRows)
        Assert-Condition (
            $rows.Count -eq $descriptor.row_count
        ) "row count mismatch: $path"
        $rowsByFile[$descriptor.path] = $rows
    }

    $sourceIds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['sources.ndjson']) {
        Add-UniqueId -Set $sourceIds -Id $row.source_id -Kind 'source'
    }
    foreach ($rows in $rowsByFile.Values) {
        foreach ($row in $rows) {
            Assert-Condition (
                $sourceIds.Contains($row.source_id)
            ) "row references missing source: $($row.source_id)"
        }
    }

    $speciesIds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['pals.ndjson']) {
        Add-UniqueId -Set $speciesIds -Id $row.species_id -Kind 'species'
    }
    $itemIds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['items.ndjson']) {
        Add-UniqueId -Set $itemIds -Id $row.item_id -Kind 'item'
    }
    $recipeIds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['recipes.ndjson']) {
        Add-UniqueId -Set $recipeIds -Id $row.recipe_id -Kind 'recipe'
        Assert-Condition (
            $itemIds.Contains($row.output_item_id)
        ) "recipe output references missing item: $($row.output_item_id)"
        foreach ($ingredient in $row.ingredients) {
            Assert-Condition (
                $itemIds.Contains($ingredient.item_id)
            ) "recipe ingredient references missing item: $($ingredient.item_id)"
            Assert-Condition ($ingredient.quantity -gt 0) 'recipe ingredient quantity must be positive'
        }
    }

    $locations = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['world.ndjson']) {
        if ($row.kind -eq 'location') {
            Add-UniqueId -Set $locations -Id $row.location.location_id -Kind 'location'
        }
    }

    $methodIds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    $seenKinds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['acquisition.ndjson']) {
        Add-UniqueId -Set $methodIds -Id $row.method_id -Kind 'acquisition method'
        Assert-Condition (
            $AcquisitionKinds -contains $row.kind
        ) "unknown acquisition kind: $($row.kind)"
        Assert-Condition ($seenKinds.Add($row.kind)) "duplicate acquisition kind in fixture: $($row.kind)"
        $subtypes = @(
            $AcquisitionKinds | Where-Object {
                $null -ne (Get-PropertyValue -Object $row -Name $_)
            }
        )
        Assert-Condition (
            $subtypes.Count -eq 1 -and $subtypes[0] -eq $row.kind
        ) "acquisition subtype does not match kind: $($row.method_id)"

        if ($row.kind -eq 'captured_cage') {
            Assert-Condition (
                $null -eq (Get-PropertyValue -Object $row -Name 'item_id')
            ) 'captured_cage cannot target an item'
            Assert-Condition (
                $speciesIds.Contains($row.species_id)
            ) "captured_cage references missing species: $($row.species_id)"
            Assert-Condition (
                $row.captured_cage.species_id -eq $row.species_id
            ) 'captured_cage nested species does not match its target'
            Assert-Condition (
                $row.captured_cage.minimum_level -le $row.captured_cage.maximum_level
            ) 'captured_cage level range is invalid'
            Assert-Condition (
                $row.captured_cage.source_weight -gt 0
            ) 'captured_cage source weight must be positive'
        } else {
            Assert-Condition (
                $null -eq (Get-PropertyValue -Object $row -Name 'species_id')
            ) "$($row.kind) cannot target a species"
            Assert-Condition (
                $itemIds.Contains($row.item_id)
            ) "$($row.kind) references missing item: $($row.item_id)"
        }
        if ($null -ne (Get-PropertyValue -Object $row -Name 'location_id')) {
            Assert-Condition (
                $locations.Contains($row.location_id)
            ) "$($row.kind) references missing location: $($row.location_id)"
        }
        $subtype = Get-PropertyValue -Object $row -Name $row.kind
        $minimum = Get-PropertyValue -Object $subtype -Name 'minimum_quantity'
        $maximum = Get-PropertyValue -Object $subtype -Name 'maximum_quantity'
        if ($null -ne $minimum -or $null -ne $maximum) {
            Assert-Condition (
                $null -ne $minimum -and $null -ne $maximum -and $minimum -le $maximum
            ) "$($row.kind) quantity range is invalid"
        }
        $probability = Get-PropertyValue -Object $subtype -Name 'probability_ppm'
        if ($null -ne $probability) {
            Assert-Condition (
                $probability -ge 0 -and $probability -le 1000000
            ) "$($row.kind) probability_ppm is out of range"
        }
        if ($row.kind -eq 'pal_drop') {
            Assert-Condition (
                $speciesIds.Contains($row.pal_drop.pal_id)
            ) "pal_drop references missing species: $($row.pal_drop.pal_id)"
        }
        if ($row.kind -eq 'craft') {
            Assert-Condition (
                $recipeIds.Contains($row.craft.recipe_id)
            ) "craft references missing recipe: $($row.craft.recipe_id)"
        }
        if ($row.kind -eq 'ranch') {
            Assert-Condition (
                $speciesIds.Contains($row.ranch.species_id)
            ) "ranch references missing species: $($row.ranch.species_id)"
        }
        if ($row.kind -eq 'merchant') {
            Assert-Condition (
                $itemIds.Contains($row.merchant.currency_item_id)
            ) "merchant references missing currency item: $($row.merchant.currency_item_id)"
        }
        if ($row.kind -eq 'salvage') {
            Assert-Condition (
                $row.salvage.item_id -eq $row.item_id
            ) 'salvage nested item does not match its target'
        }
    }
    Assert-Condition (
        (($seenKinds | Sort-Object) -join ',') -ceq (($AcquisitionKinds | Sort-Object) -join ',')
    ) 'fixture does not exercise every acquisition subtype exactly once'

    $ruleIds = [System.Collections.Generic.HashSet[string]]::new(
        [System.StringComparer]::Ordinal
    )
    foreach ($row in $rowsByFile['breeding.ndjson']) {
        Add-UniqueId -Set $ruleIds -Id $row.rule_id -Kind 'breeding rule'
        foreach ($speciesId in @(
            $row.parent_a_species_id,
            $row.parent_b_species_id,
            $row.child_species_id
        )) {
            Assert-Condition (
                $speciesIds.Contains($speciesId)
            ) "breeding rule references missing species: $speciesId"
        }
    }

    return $capabilities
}

function Test-PrivateHashes {
    param([string] $PrivateRoot)

    $mismatchCount = 0
    $hashList = Join-Path $PrivateRoot 'hashes.sha256'
    Assert-Condition (Test-Path -LiteralPath $hashList -PathType Leaf) 'private catalog hashes are missing'
    foreach ($line in [System.IO.File]::ReadLines($hashList)) {
        if ([string]::IsNullOrWhiteSpace($line)) {
            continue
        }
        $parts = $line -split '\s{2}', 2
        Assert-Condition ($parts.Count -eq 2) "invalid hashes.sha256 line: $line"
        $path = Join-Path $PrivateRoot ($parts[1] -replace '/', '\')
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            $mismatchCount++
            continue
        }
        $actual = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
        if ($actual -cne $parts[0]) {
            $mismatchCount++
        }
    }
    return $mismatchCount
}

try {
    $roots = Resolve-FixtureRoots -RequestedRoot $Root
    $capabilities = @(Test-CatalogFixture -CatalogRoot $roots.CatalogRoot)

    $cloudProfile = [ordered]@{
        section_count = 0
        source_snapshot_matches = $false
        identity_matches = $false
        private_field_count = 0
    }
    $privateCatalog = [ordered]@{
        distribution_scope = 'not_checked'
        asset_field_count = 0
        hash_mismatch_count = 0
    }

    if (-not $roots.IsCatalogOnly) {
        $privateRoot = Join-Path $roots.LocalRoot 'private-cloud-catalog-v1'
        $privateCatalog.hash_mismatch_count = Test-PrivateHashes -PrivateRoot $privateRoot
        Assert-Condition (
            $privateCatalog.hash_mismatch_count -eq 0
        ) 'private catalog contains hash mismatches'

        $workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
        $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        if ($null -eq $cargo) {
            $cargoPath = Join-Path $workspace '.tools\cargo\bin\cargo.exe'
            Assert-Condition (
                (Test-Path -LiteralPath $cargoPath -PathType Leaf)
            ) 'cargo is required to decode local Protobuf fixtures'
        } else {
            $cargoPath = $cargo.Source
        }
        $helperJson = & $cargoPath run --quiet -p pal-wire --bin verify-local-fixture --locked -- `
            --root $roots.LocalRoot
        Assert-Condition ($LASTEXITCODE -eq 0) 'Rust Protobuf fixture verifier failed'
        $helper = $helperJson | ConvertFrom-Json
        $cloudProfile = [ordered]@{
            section_count = $helper.cloud_profile.section_count
            source_snapshot_matches = $helper.cloud_profile.source_snapshot_matches
            identity_matches = $helper.cloud_profile.identity_matches
            private_field_count = $helper.cloud_profile.private_field_count
        }
        $privateCatalog.distribution_scope = $helper.private_catalog.distribution_scope
        $privateCatalog.asset_field_count = $helper.private_catalog.asset_field_count
    }

    $result = [ordered]@{
        schema_major = 1
        game_build_id = 'steam:24181527'
        capabilities = $capabilities
        capability_count = $capabilities.Count
        cloud_profile = $cloudProfile
        private_catalog = $privateCatalog
        orphan_reference_count = 0
    }
    if ($Json) {
        $result | ConvertTo-Json -Depth 10 -Compress
    } else {
        $result | ConvertTo-Json -Depth 10
    }
    $global:LASTEXITCODE = 0
} catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    $global:LASTEXITCODE = 1
    exit 1
}
