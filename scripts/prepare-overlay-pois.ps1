param(
    [string]$OutputPath = "",
    [string]$InputPath = "",
    [string]$IconDirectory = "",
    [string]$GameBuildId = "24575825",
    [switch]$AllowThirdPartyBenchmarkDownload
)

$ErrorActionPreference = "Stop"
$sourceUrl = "https://www.palworldguide.net/data/map-locations.json"
$palCalcRevision = "922822d99076465e026364f7b07f257f46b3e7a6"

if (-not $AllowThirdPartyBenchmarkDownload) {
    throw @"
This script downloads third-party benchmark coordinates and PalCalc portraits.
They are not exact installed-game authority. Pass
-AllowThirdPartyBenchmarkDownload only for an explicit, unverified comparison
artifact. Production map data comes from the reviewed installed-game pipeline.
"@
}

if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $PSScriptRoot "..\artifacts\palworld-map-pois-$GameBuildId.json"
}
if ([string]::IsNullOrWhiteSpace($IconDirectory)) {
    $IconDirectory = Join-Path $PSScriptRoot "..\artifacts\palworld-poi-icons-$GameBuildId"
}

function ConvertTo-IconMatchKey([string]$Value) {
    return [regex]::Replace($Value.ToLowerInvariant(), '[^a-z0-9]', '')
}

function ConvertTo-RawGitHubUrl([string]$RepositoryPath) {
    $encoded = ($RepositoryPath -split '/' | ForEach-Object {
        [uri]::EscapeDataString($_)
    }) -join '/'
    return "https://raw.githubusercontent.com/tylercamp/palcalc/$palCalcRevision/$encoded"
}

$temporary = $null
try {
    if ([string]::IsNullOrWhiteSpace($InputPath)) {
        $temporary = Join-Path ([System.IO.Path]::GetTempPath()) "pal-companion-map-locations-$([guid]::NewGuid().ToString('N')).json"
        Invoke-WebRequest -UseBasicParsing -Uri $sourceUrl -OutFile $temporary
        $InputPath = $temporary
    }

    $source = Get-Content -Raw -Encoding utf8 -LiteralPath $InputPath | ConvertFrom-Json
    if ($source.coordinateSystem -ne "pal-map-x-y-in-[-1000,1000]") {
        throw "Unsupported POI coordinate system: $($source.coordinateSystem)"
    }

    $kindByCategory = @{
        "Fast Travel" = "fast_travel"
        "Alpha Pals" = "boss"
        "Dungeons" = "dungeon"
    }
    New-Item -ItemType Directory -Force -Path $IconDirectory | Out-Null
    $githubHeaders = @{ "User-Agent" = "PalCompanion-private-icon-preparer" }
    $palCalcTree = Invoke-RestMethod `
        -Headers $githubHeaders `
        -Uri "https://api.github.com/repos/tylercamp/palcalc/git/trees/$palCalcRevision`?recursive=1"
    $palIconCandidates = @(
        $palCalcTree.tree |
            Where-Object { $_.path -match '^PalCalc\.UI/Resources/Pals/.+\.png$' } |
            ForEach-Object {
                $stem = [System.IO.Path]::GetFileNameWithoutExtension([string]$_.path)
                [pscustomobject]@{
                    Path = [string]$_.path
                    Stem = $stem
                    MatchKey = ConvertTo-IconMatchKey $stem
                }
            } |
            Sort-Object { $_.MatchKey.Length } -Descending
    )
    if ($palIconCandidates.Count -lt 200) {
        throw "Pal portrait index contains too few rows: $($palIconCandidates.Count)"
    }

    $index = 0
    $matchedBossPortraits = 0
    $pois = foreach ($location in $source.locations) {
        $kind = $kindByCategory[[string]$location.category]
        if (-not $kind) {
            continue
        }
        $mapX = [double]$location.x
        $mapY = [double]$location.y
        if ([double]::IsNaN($mapX) -or [double]::IsInfinity($mapX) -or
            [double]::IsNaN($mapY) -or [double]::IsInfinity($mapY)) {
            throw "POI contains a non-finite coordinate"
        }
        $iconFile = $null
        if ($kind -eq "boss") {
            $hrefKey = ConvertTo-IconMatchKey ([string]$location.href)
            $candidate = $palIconCandidates |
                Where-Object {
                    $_.MatchKey.Length -ge 4 -and $hrefKey.EndsWith($_.MatchKey)
                } |
                Select-Object -First 1
            if ($candidate) {
                $iconName = "$($candidate.MatchKey).png"
                $iconPath = Join-Path $IconDirectory $iconName
                if (-not (Test-Path -LiteralPath $iconPath -PathType Leaf)) {
                    Invoke-WebRequest `
                        -UseBasicParsing `
                        -Uri (ConvertTo-RawGitHubUrl $candidate.Path) `
                        -OutFile $iconPath
                }
                $iconFile = "poi-icons/$iconName"
                $matchedBossPortraits++
            }
        }
        $index++
        [pscustomobject][ordered]@{
            id = "$kind-$('{0:D4}' -f $index)"
            name = [string]$location.name
            kind = $kind
            # palworld-coord compatible map-to-save conversion. The overlay transform owns
            # the final axis rotation and clipping for the exact MainMap bounds.
            world_x = [math]::Round(($mapY * 459.0) - 123888.0, 3)
            world_y = [math]::Round(($mapX * 459.0) + 158000.0, 3)
            icon_file = $iconFile
        }
    }

    if (@($pois).Count -lt 100) {
        throw "POI extraction returned too few rows: $(@($pois).Count)"
    }

    $document = [ordered]@{
        schema = "pal_companion.overlay_pois.v1"
        game_build_id = [uint64]$GameBuildId
        dataset_version = "overlay-pois-unverified-$($source.version)-comparison-$GameBuildId"
        verified = $false
        authority = "third_party_benchmark_unverified"
        source_url = $sourceUrl
        source_version = [string]$source.version
        source_content_sha256 = (
            Get-FileHash -LiteralPath $InputPath -Algorithm SHA256
        ).Hash.ToLowerInvariant()
        palcalc_repository = "https://github.com/tylercamp/palcalc"
        palcalc_revision = $palCalcRevision
        pois = @($pois)
    }

    $parent = Split-Path -Parent $OutputPath
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    [System.IO.File]::WriteAllText(
        [System.IO.Path]::GetFullPath($OutputPath),
        ($document | ConvertTo-Json -Depth 5),
        [System.Text.UTF8Encoding]::new($false)
    )

    $counts = @($pois) | Group-Object kind | Sort-Object Name
    Write-Host "Prepared $(@($pois).Count) overlay POIs from $($source.version):"
    $counts | ForEach-Object { Write-Host "  $($_.Name): $($_.Count)" }
    Write-Host "  boss portraits: $matchedBossPortraits"
    Write-Host "Output: $([System.IO.Path]::GetFullPath($OutputPath))"
}
finally {
    if ($temporary -and (Test-Path -LiteralPath $temporary)) {
        Remove-Item -LiteralPath $temporary -Force
    }
}
