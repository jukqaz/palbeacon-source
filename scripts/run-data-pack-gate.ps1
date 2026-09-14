[CmdletBinding()]
param(
    [Parameter()]
    [string] $Fixture = "tests/fixtures/local-data/catalog-synthetic-v1",

    [Parameter()]
    # The normalized synthetic fixture has its own frozen identity. The current
    # installed-game identity is independently pinned by $Contract for LiveProbe.
    [string] $Build = "steam:24467282",

    [Parameter()]
    [switch] $LiveProbe,

    [Parameter()]
    [string] $Mapping = ".tools/vendor/usefulfiles/Mappings.usmap",

    [Parameter()]
    [string] $Contract = "tools/pal-data-pack/contracts/24575825.asset-contract.json"
)

$ErrorActionPreference = "Stop"
$repositoryRoot = Split-Path -Parent $PSScriptRoot
$miseRunner = Join-Path $repositoryRoot "scripts/run-mise.ps1"
$solution = Join-Path $repositoryRoot "tools/pal-data-pack/PalDataPack.slnx"
$project = Join-Path $repositoryRoot "tools/pal-data-pack/src/PalDataPack"
$fixturePath = [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot $Fixture))
$temporaryRoot = Join-Path ([System.IO.Path]::GetTempPath()) (
    "pal-data-pack-gate-" + [Guid]::NewGuid().ToString("N"))
$resolvedTemporaryRoot = [System.IO.Path]::GetFullPath($temporaryRoot)
$resolvedSystemTemp = [System.IO.Path]::GetFullPath(
    [System.IO.Path]::GetTempPath())
$temporaryLeaf = Split-Path -Leaf $resolvedTemporaryRoot

if (
    -not $resolvedTemporaryRoot.StartsWith(
        $resolvedSystemTemp,
        [System.StringComparison]::OrdinalIgnoreCase) -or
    -not $temporaryLeaf.StartsWith(
        "pal-data-pack-gate-",
        [System.StringComparison]::Ordinal)
) {
    throw "Refusing to use an unsafe temporary gate directory: $resolvedTemporaryRoot"
}

if (-not (Test-Path -LiteralPath $miseRunner -PathType Leaf)) {
    throw "mise runner is missing: $miseRunner"
}

function Invoke-Dotnet {
    param([Parameter(Mandatory)][string[]] $ArgumentList)

    & $miseRunner `
        -Workspace $repositoryRoot `
        -Run dotnet `
        -RunArgument $ArgumentList
    if ($LASTEXITCODE -ne 0) {
        throw "global mise dotnet failed with exit code $LASTEXITCODE"
    }
}

try {
    Invoke-Dotnet @("restore", $solution, "--locked-mode", "--nologo")
    Invoke-Dotnet @("test", $solution, "--no-restore", "--nologo")
    Invoke-Dotnet @(
        "run",
        "--project", $project,
        "--no-restore",
        "--",
        "publish-normalized",
        "--source", $fixturePath,
        "--dataset-root", $temporaryRoot,
        "--build", $Build
    )

    $safeBuild = $Build.Replace(":", "_")
    $pointerPath = Join-Path $temporaryRoot "$safeBuild.active.json"
    $pointer = Get-Content -LiteralPath $pointerPath -Raw | ConvertFrom-Json
    $versionPath = Join-Path $temporaryRoot (
        [string] $pointer.relative_version_path).Replace("/", "\")

    Invoke-Dotnet @(
        "run",
        "--project", $project,
        "--no-restore",
        "--",
        "validate-package",
        "--path", $versionPath
    )

    if ($LiveProbe) {
        Invoke-Dotnet @(
            "run",
            "--project", $project,
            "--no-restore",
            "--",
            "probe",
            "--install", "auto",
            "--mapping", (
                [System.IO.Path]::GetFullPath(
                    (Join-Path $repositoryRoot $Mapping)
                )
            ),
            "--contract", (
                [System.IO.Path]::GetFullPath(
                    (Join-Path $repositoryRoot $Contract)
                )
            )
        )
    }

    [pscustomobject]@{
        ok = $true
        build = $Build
        dataset_manifest_id = [string] $pointer.dataset_manifest_id
        capability_count = 13
        live_probe = [bool] $LiveProbe
    } | ConvertTo-Json -Depth 3
}
finally {
    if (Test-Path -LiteralPath $resolvedTemporaryRoot) {
        Remove-Item -LiteralPath $resolvedTemporaryRoot -Recurse -Force
    }
}
