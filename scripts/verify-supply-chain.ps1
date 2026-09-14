#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $Workspace,
    [string] $CargoLock,
    [switch] $SkipExternalTools
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-Executable {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $LocalPath
    )

    if (Test-Path -LiteralPath $LocalPath -PathType Leaf) {
        return (Resolve-Path -LiteralPath $LocalPath).Path
    }
    $command = Get-Command -Name $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -ne $command) {
        return $command.Path
    }
    throw "Required executable '$Name' is unavailable."
}

function Get-CargoManifests {
    param([Parameter(Mandatory)] [string] $Root)

    $manifests = [Collections.Generic.List[string]]::new()
    $rootManifest = Join-Path $Root 'Cargo.toml'
    if (Test-Path -LiteralPath $rootManifest -PathType Leaf) {
        $manifests.Add((Resolve-Path -LiteralPath $rootManifest).Path)
    }
    foreach ($relativeRoot in @('crates', 'packages', 'tools')) {
        $searchRoot = Join-Path $Root $relativeRoot
        if (-not (Test-Path -LiteralPath $searchRoot -PathType Container)) {
            continue
        }
        Get-ChildItem -LiteralPath $searchRoot -Filter Cargo.toml -File -Recurse -ErrorAction SilentlyContinue |
            Where-Object {
                $_.FullName -notmatch '[\\/](?:target|node_modules|bin|obj)[\\/]'
            } |
            ForEach-Object { $manifests.Add($_.FullName) }
    }
    @($manifests | Sort-Object -Unique)
}

function Assert-NoUnpinnedCargoSources {
    param(
        [Parameter(Mandatory)] [string] $LockPath,
        [Parameter(Mandatory)] [string[]] $Manifests
    )

    $lockText = Get-Content -LiteralPath $LockPath -Raw
    if ($lockText -match '(?im)^\s*source\s*=\s*"git\+') {
        throw 'Git dependencies are forbidden in Cargo.lock.'
    }
    foreach ($manifest in $Manifests) {
        $text = Get-Content -LiteralPath $manifest -Raw
        if ($text -match '(?im)\bgit\s*=\s*"') {
            throw "Git dependencies are forbidden in $manifest."
        }
        if ($text -match '(?im)\bbranch\s*=\s*"') {
            throw "Branch dependencies are forbidden in $manifest."
        }
        if ($text -match '(?im)\bversion\s*=\s*"\s*\*\s*"' -or
            $text -match '(?im)^\s*[A-Za-z0-9_-]+\s*=\s*"\s*\*\s*"\s*$') {
            throw "Wildcard Cargo versions are forbidden in $manifest."
        }
    }
}

function Assert-RuntimeInventory {
    param(
        [Parameter(Mandatory)] [string] $InventoryPath,
        [Parameter(Mandatory)] [object] $Sbom
    )

    if (-not (Test-Path -LiteralPath $InventoryPath -PathType Leaf)) {
        throw "Reviewed runtime inventory is missing: $InventoryPath"
    }
    $inventory = Get-Content -LiteralPath $InventoryPath -Raw | ConvertFrom-Json
    if ($inventory.schema_version -ne 1) {
        throw 'Unsupported runtime dependency inventory schema.'
    }
    $components = @($inventory.components)
    if ($components.Count -eq 0) {
        throw 'Reviewed runtime dependency inventory is empty.'
    }

    $requiredFields = @(
        'name',
        'version',
        'purl',
        'source',
        'license',
        'purpose',
        'native_or_unsafe',
        'spawned_threads_or_processes',
        'rollback_path',
        'review_date'
    )
    $seenPurls = @{}
    $sbomPurls = @{}
    foreach ($component in @($Sbom.components)) {
        $sbomPurls[[string] $component.purl] = $true
    }
    foreach ($component in $components) {
        foreach ($field in $requiredFields) {
            $property = $component.PSObject.Properties[$field]
            if ($null -eq $property -or
                [string]::IsNullOrWhiteSpace([string] $property.Value)) {
                throw "Runtime dependency '$($component.name)' is missing '$field'."
            }
        }
        if (@($component.owners).Count -eq 0) {
            throw "Runtime dependency '$($component.name)' has no owner."
        }
        if ([string] $component.version -match '[*<>=~^,\s]') {
            throw "Runtime dependency '$($component.name)' does not use an exact version."
        }
        if ([string] $component.license -notmatch '^[A-Za-z0-9.+-]+(?:\s+(?:AND|OR)\s+[A-Za-z0-9.+-]+)*$') {
            throw "Runtime dependency '$($component.name)' has a non-SPDX license expression."
        }
        if ([string] $component.purl -notmatch '^pkg:[a-z0-9.+-]+/.+@[^@]+$') {
            throw "Runtime dependency '$($component.name)' has an invalid package URL."
        }
        if ($seenPurls.ContainsKey([string] $component.purl)) {
            throw "Duplicate reviewed package URL: $($component.purl)"
        }
        $seenPurls[[string] $component.purl] = $true

        $isPlanned = @($component.owners) -match '^planned '
        if (-not $isPlanned -and -not $sbomPurls.ContainsKey([string] $component.purl)) {
            throw "Reviewed runtime dependency is absent from committed locks: $($component.purl)"
        }
    }

    foreach ($requiredName in @(
            'prost',
            'tonic',
            'tokio',
            'windows-sys',
            'ring',
            'rusqlite',
            'wasm-bindgen'
        )) {
        if (@($components | Where-Object { $_.name -eq $requiredName }).Count -ne 1) {
            throw "Required reviewed runtime dependency is missing or duplicated: $requiredName"
        }
    }
}

function Assert-Sbom {
    param([Parameter(Mandatory)] [object] $Sbom)

    if ($Sbom.bomFormat -ne 'CycloneDX' -or $Sbom.specVersion -ne '1.6') {
        throw 'The generated SBOM is not CycloneDX 1.6.'
    }
    $components = @($Sbom.components)
    if ($components.Count -eq 0) {
        throw 'The generated SBOM contains no components.'
    }
    foreach ($component in $components) {
        if ([string]::IsNullOrWhiteSpace([string] $component.version) -or
            [string]::IsNullOrWhiteSpace([string] $component.purl)) {
            throw "SBOM component '$($component.name)' has no exact version or package URL."
        }
        if ([string] $component.version -match '[*<>=~^,\s]') {
            throw "SBOM component '$($component.name)' does not use an exact version."
        }
    }
}

function Assert-NoPrivateMarkers {
    param(
        [Parameter(Mandatory)] [string] $Root,
        [Parameter(Mandatory)] [string[]] $Paths
    )

    $patterns = @(
        '(?i)\bplayer_ip\b',
        '(?i)\bplatform_account_id\b',
        '(?i)\braw_save_bytes\b',
        '(?i)Authorization:\s*Bearer\s+[A-Za-z0-9._~-]{12,}',
        '(?i)\\Pal\\Saved\\SaveGames\\',
        '(?i)\\Steam\\userdata\\'
    )
    $privateMatches = [Collections.Generic.List[string]]::new()
    foreach ($path in $Paths) {
        if (-not (Test-Path -LiteralPath $path)) {
            continue
        }
        $files = if (Test-Path -LiteralPath $path -PathType Container) {
            @(Get-ChildItem -LiteralPath $path -File -Recurse -ErrorAction SilentlyContinue)
        }
        else {
            @(Get-Item -LiteralPath $path)
        }
        foreach ($file in $files) {
            if ($file.Name -eq 'supply-chain.log') {
                continue
            }
            if ($file.Length -gt 16MB) {
                continue
            }
            $bytes = [IO.File]::ReadAllBytes($file.FullName)
            $text = [Text.Encoding]::UTF8.GetString($bytes)
            foreach ($pattern in $patterns) {
                if ($text -match $pattern) {
                    $relative = $file.FullName.Substring($Root.TrimEnd('\').Length).TrimStart('\')
                    $privateMatches.Add("$relative matched $pattern")
                }
            }
        }
    }
    if ($privateMatches.Count -gt 0) {
        throw "Private-data markers were found:`n$($privateMatches -join "`n")"
    }
}

function Invoke-CheckedCommand {
    param(
        [Parameter(Mandatory)] [string] $Executable,
        [Parameter(Mandatory)] [string[]] $Arguments,
        [Parameter(Mandatory)] [string] $Description
    )

    & $Executable @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Description failed with exit code $LASTEXITCODE."
    }
}

try {
    if ([string]::IsNullOrWhiteSpace($Workspace)) {
        $Workspace = Join-Path $PSScriptRoot '..'
    }
    $Workspace = (Resolve-Path -LiteralPath $Workspace).Path
    if ([string]::IsNullOrWhiteSpace($CargoLock)) {
        $CargoLock = Join-Path $Workspace 'Cargo.lock'
    }
    elseif (-not [IO.Path]::IsPathRooted($CargoLock)) {
        $CargoLock = Join-Path $Workspace $CargoLock
    }
    $CargoLock = (Resolve-Path -LiteralPath $CargoLock).Path

    $cargo = Resolve-Executable -Name 'cargo' `
        -LocalPath (Join-Path $Workspace '.tools\cargo\bin\cargo.exe')
    $manifests = @(Get-CargoManifests -Root $Workspace)
    Assert-NoUnpinnedCargoSources -LockPath $CargoLock -Manifests $manifests

    $generator = Join-Path $Workspace 'scripts\generate-sbom.ps1'
    & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $generator -Workspace $Workspace
    if ($LASTEXITCODE -ne 0) {
        throw "SBOM generation failed with exit code $LASTEXITCODE."
    }
    $sbomPath = Join-Path $Workspace 'artifacts\sbom\palbeacon.cdx.json'
    $sbom = Get-Content -LiteralPath $sbomPath -Raw | ConvertFrom-Json
    Assert-Sbom -Sbom $sbom
    Assert-RuntimeInventory `
        -InventoryPath (Join-Path $Workspace 'third_party\runtime-dependencies.json') `
        -Sbom $sbom
    Assert-NoPrivateMarkers -Root $Workspace -Paths @(
        $sbomPath,
        (Join-Path $Workspace 'third_party\THIRD_PARTY_NOTICES.md'),
        (Join-Path $Workspace 'tests\fixtures\contracts\wire-v1'),
        (Join-Path $Workspace 'artifacts\logs')
    )

    if (-not $SkipExternalTools) {
        Push-Location -LiteralPath $Workspace
        try {
            Invoke-CheckedCommand -Executable $cargo `
                -Arguments @('deny', 'check', 'licenses', 'bans', 'sources') `
                -Description 'cargo-deny'
            Invoke-CheckedCommand -Executable $cargo `
                -Arguments @('audit') `
                -Description 'cargo-audit'
        }
        finally {
            Pop-Location
        }
        $generatedCheck = Join-Path $Workspace 'scripts\verify-generated-clean.ps1'
        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $generatedCheck
        if ($LASTEXITCODE -ne 0) {
            throw "Generated-code cleanliness check failed with exit code $LASTEXITCODE."
        }
    }

    Write-Output "Supply-chain verification passed with $(@($sbom.components).Count) SBOM components."
    exit 0
}
catch {
    [Console]::Error.WriteLine($_.Exception.Message)
    exit 1
}
