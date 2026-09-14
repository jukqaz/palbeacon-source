[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'

function Get-Sha256Hex {
    param([Parameter(Mandatory)][string] $LiteralPath)

    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    $stream = [System.IO.File]::OpenRead($LiteralPath)
    try {
        return ([System.BitConverter]::ToString(
            $algorithm.ComputeHash($stream)
        )).Replace('-', '')
    } finally {
        $stream.Dispose()
        $algorithm.Dispose()
    }
}

function Copy-VerifiedFile {
    param(
        [Parameter(Mandatory)][string] $Source,
        [Parameter(Mandatory)][string] $Destination
    )

    $parent = Split-Path -Parent $Destination
    [void] (New-Item -ItemType Directory -Path $parent -Force)
    Copy-Item -LiteralPath $Source -Destination $Destination -Force
    if ((Get-Sha256Hex -LiteralPath $Source) -ne
        (Get-Sha256Hex -LiteralPath $Destination)) {
        throw "Prepared file does not match its release source: $Destination"
    }
}

function Copy-TreeContent {
    param(
        [Parameter(Mandatory)][string] $Source,
        [Parameter(Mandatory)][string] $Destination
    )

    [void] (New-Item -ItemType Directory -Path $Destination -Force)
    foreach ($entry in Get-ChildItem -LiteralPath $Source -Force) {
        $target = Join-Path $Destination $entry.Name
        if ($entry.PSIsContainer) {
            Copy-TreeContent -Source $entry.FullName -Destination $target
        } else {
            Copy-VerifiedFile -Source $entry.FullName -Destination $target
        }
    }
}

function Assert-NoExtraFiles {
    param(
        [Parameter(Mandatory)][string] $Source,
        [Parameter(Mandatory)][string] $Destination
    )

    $destinationRoot = [System.IO.Path]::GetFullPath($Destination).TrimEnd('\', '/')
    foreach ($file in Get-ChildItem -LiteralPath $destinationRoot -Recurse -Force -File) {
        $relative = $file.FullName.Substring($destinationRoot.Length + 1)
        if (-not (Test-Path -LiteralPath (Join-Path $Source $relative) -PathType Leaf)) {
            throw 'The generated map-pack directory contains files outside the approved source.'
        }
    }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$uiRoot = Join-Path $repoRoot 'apps\palbeacon-ui'
$workerRoot = Join-Path $repoRoot 'experimental\pal-save-parser-worker'
$workerManifest = Join-Path $workerRoot 'Cargo.toml'
$workerLock = Join-Path $workerRoot 'Cargo.lock'
$workerSource = Join-Path $workerRoot `
    'target\x86_64-pc-windows-msvc\release\pal-save-parser-worker.exe'
$targetTriple = 'x86_64-pc-windows-msvc'
$runtimeTarget = Join-Path $repoRoot "target\$targetTriple\release"
$runtimeContractPath = Join-Path $repoRoot 'contracts\runtime-builds.json'
$runtimeContract = Get-Content -LiteralPath $runtimeContractPath -Raw | ConvertFrom-Json
$gameBuildId = [string] $runtimeContract.current_game_build_id
$mapPackSource = Join-Path $repoRoot `
    "assets\palbeacon\game\native-map-pack\$gameBuildId"
$activePointerSource = Join-Path $mapPackSource "$gameBuildId.active.json"
$mapPackStage = Join-Path $repoRoot 'target\palbeacon-runtime-resources\map-pack'
$sidecarDirectory = Join-Path $repoRoot 'apps\palbeacon-desktop\src-tauri\binaries'
$sidecarTarget = Join-Path $sidecarDirectory `
    'pal-save-parser-worker-x86_64-pc-windows-msvc.exe'
$desktopRelease = Join-Path $repoRoot `
    "apps\palbeacon-desktop\src-tauri\target\$targetTriple\release"
$parserData = Join-Path $repoRoot 'assets\save-parser'

if (-not (Test-Path -LiteralPath $workerLock -PathType Leaf)) {
    throw 'The pinned save parser Cargo.lock is missing.'
}
if (-not (Test-Path -LiteralPath (Join-Path $parserData 'pals.json') -PathType Leaf)) {
    throw 'The pinned save parser data directory is incomplete.'
}
if ($gameBuildId -notmatch '^\d+$') {
    throw 'The current game Build ID contract is invalid.'
}
if (-not (Test-Path -LiteralPath $activePointerSource -PathType Leaf)) {
    throw "The approved exact-build map pack is missing: $activePointerSource"
}
$activePointer = Get-Content -LiteralPath $activePointerSource -Raw | ConvertFrom-Json
if ([string] $activePointer.game_build_id -ne $gameBuildId -or
    [int] $activePointer.schema_version -ne 1) {
    throw 'The approved map pack active pointer does not match the runtime build contract.'
}
$relativeVersion = [string] $activePointer.relative_version_path
$mapPackSourceRoot = [System.IO.Path]::GetFullPath($mapPackSource)
$versionSource = [System.IO.Path]::GetFullPath((Join-Path $mapPackSource $relativeVersion))
if (-not $versionSource.StartsWith(
        $mapPackSourceRoot + [System.IO.Path]::DirectorySeparatorChar,
        [System.StringComparison]::OrdinalIgnoreCase) -or
    -not (Test-Path -LiteralPath (Join-Path $versionSource 'manifest.json') -PathType Leaf)) {
    throw 'The approved map pack active version path is unsafe or incomplete.'
}

$nodePath = $env:PALBEACON_NODE
if ([string]::IsNullOrWhiteSpace($nodePath)) {
    $node = Get-Command node.exe -ErrorAction SilentlyContinue
    if ($null -eq $node) {
        $node = Get-Command node -ErrorAction SilentlyContinue
    }
    $nodePath = if ($null -eq $node) { $null } else { $node.Source }
}
if ([string]::IsNullOrWhiteSpace($nodePath) -or
    -not (Test-Path -LiteralPath $nodePath -PathType Leaf)) {
    throw 'Node.js is required to build the PalBeacon frontend.'
}
$cargoPath = $env:PALBEACON_CARGO
if ([string]::IsNullOrWhiteSpace($cargoPath)) {
    $repositoryCargo = Join-Path $repoRoot '.tools\cargo\bin\cargo.exe'
    if (Test-Path -LiteralPath $repositoryCargo -PathType Leaf) {
        $cargoPath = $repositoryCargo
        $env:CARGO_HOME = Join-Path $repoRoot '.tools\cargo'
        $env:RUSTUP_HOME = Join-Path $repoRoot '.tools\rustup'
    } else {
        $cargo = Get-Command cargo.exe -ErrorAction SilentlyContinue
        if ($null -eq $cargo) {
            $cargo = Get-Command cargo -ErrorAction SilentlyContinue
        }
        $cargoPath = if ($null -eq $cargo) { $null } else { $cargo.Source }
    }
}
if ([string]::IsNullOrWhiteSpace($cargoPath) -or
    -not (Test-Path -LiteralPath $cargoPath -PathType Leaf)) {
    throw 'Cargo is required to build the PalBeacon save parser sidecar.'
}
$env:Path = "$(Split-Path -Parent $nodePath);$(Split-Path -Parent $cargoPath);$env:Path"
$syncScript = Join-Path $uiRoot 'scripts\sync-exact-data.mjs'
$verifyWebScript = Join-Path $uiRoot 'scripts\verify-web-release.mjs'
$viteScript = Join-Path $uiRoot 'node_modules\vite\bin\vite.js'
if (-not (Test-Path -LiteralPath $viteScript -PathType Leaf)) {
    throw 'Run the locked workspace dependency install before building PalBeacon.'
}

Push-Location $repoRoot
try {
    Set-Location $uiRoot
    & $nodePath $syncScript
    if ($LASTEXITCODE -ne 0) {
        throw "PalBeacon exact-data sync failed with exit code $LASTEXITCODE."
    }
    & $nodePath $viteScript build
    if ($LASTEXITCODE -ne 0) {
        throw "PalBeacon frontend build failed with exit code $LASTEXITCODE."
    }
    & $nodePath $verifyWebScript
    if ($LASTEXITCODE -ne 0) {
        throw "PalBeacon frontend release verification failed with exit code $LASTEXITCODE."
    }

    Set-Location $repoRoot
    & $cargoPath build --release --locked --offline --target $targetTriple `
        -p pal-core-win `
        -p pal-fullscreen-injector `
        -p pal-fullscreen-rhi-probe `
        -p pal-fullscreen-overlay-dx11 `
        -p pal-fullscreen-overlay-dx12
    if ($LASTEXITCODE -ne 0) {
        throw "Windows runtime build failed with exit code $LASTEXITCODE."
    }
    & $cargoPath build --release --locked --offline --target $targetTriple `
        -p pal-overlay-win --features approved-map-pack-runtime
    if ($LASTEXITCODE -ne 0) {
        throw "Windows overlay build failed with exit code $LASTEXITCODE."
    }

    & $cargoPath build --manifest-path $workerManifest --release --locked --offline
    if ($LASTEXITCODE -ne 0) {
        throw "Save parser sidecar build failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $workerSource -PathType Leaf)) {
    throw 'The save parser sidecar build did not produce its executable.'
}
$runtimeExecutables = @(
    [ordered]@{
        source = Join-Path $runtimeTarget 'pal-core.exe'
        sidecar = Join-Path $sidecarDirectory "pal-core-$targetTriple.exe"
        package = Join-Path $desktopRelease 'pal-core.exe'
    },
    [ordered]@{
        source = Join-Path $runtimeTarget 'pal-overlay.exe'
        sidecar = Join-Path $sidecarDirectory "pal-overlay-$targetTriple.exe"
        package = Join-Path $desktopRelease 'pal-overlay.exe'
    },
    [ordered]@{
        source = Join-Path $runtimeTarget 'pal-fullscreen-injector.exe'
        sidecar = Join-Path $sidecarDirectory "pal-fullscreen-injector-$targetTriple.exe"
        package = Join-Path $desktopRelease 'pal-fullscreen-injector.exe'
    }
)
$runtimeLibraries = @(
    [ordered]@{
        source = Join-Path $runtimeTarget 'pal_fullscreen_rhi_probe.dll'
        sidecar = Join-Path $sidecarDirectory 'pal-fullscreen-rhi-probe.dll'
        package = Join-Path $desktopRelease 'pal-fullscreen-rhi-probe.dll'
    },
    [ordered]@{
        source = Join-Path $runtimeTarget 'pal_fullscreen_overlay_dx11.dll'
        sidecar = Join-Path $sidecarDirectory 'pal-fullscreen-overlay-dx11.dll'
        package = Join-Path $desktopRelease 'pal-fullscreen-overlay-dx11.dll'
    },
    [ordered]@{
        source = Join-Path $runtimeTarget 'pal_fullscreen_overlay_dx12.dll'
        sidecar = Join-Path $sidecarDirectory 'pal-fullscreen-overlay-dx12.dll'
        package = Join-Path $desktopRelease 'pal-fullscreen-overlay-dx12.dll'
    }
)

foreach ($artifact in @($runtimeExecutables) + @($runtimeLibraries)) {
    if (-not (Test-Path -LiteralPath $artifact.source -PathType Leaf)) {
        throw "Windows runtime build output is missing: $($artifact.source)"
    }
}

$stagedVersion = Join-Path $mapPackStage $relativeVersion
Copy-VerifiedFile -Source $activePointerSource `
    -Destination (Join-Path $mapPackStage "$gameBuildId.active.json")
Copy-TreeContent -Source $versionSource -Destination $stagedVersion
Assert-NoExtraFiles -Source $mapPackSource -Destination $mapPackStage
$versionsRoot = Join-Path $mapPackStage '.versions'
$unexpectedVersions = @(
    Get-ChildItem -LiteralPath $versionsRoot -Directory -ErrorAction SilentlyContinue |
        Where-Object {
            -not $_.FullName.Equals(
                $stagedVersion,
                [System.StringComparison]::OrdinalIgnoreCase)
        }
)
if ($unexpectedVersions.Count -ne 0) {
    throw 'The generated map-pack staging root contains an older inactive version.'
}

[void] (New-Item -ItemType Directory -Path $sidecarDirectory -Force)
[void] (New-Item -ItemType Directory -Path $desktopRelease -Force)
Copy-VerifiedFile -Source $workerSource -Destination $sidecarTarget
foreach ($artifact in $runtimeExecutables) {
    Copy-VerifiedFile -Source $artifact.source -Destination $artifact.sidecar
    Copy-VerifiedFile -Source $artifact.source -Destination $artifact.package
}
foreach ($artifact in $runtimeLibraries) {
    Copy-VerifiedFile -Source $artifact.source -Destination $artifact.sidecar
    Copy-VerifiedFile -Source $artifact.source -Destination $artifact.package
}
Copy-TreeContent -Source $mapPackStage `
    -Destination (Join-Path $desktopRelease 'map-pack')
Assert-NoExtraFiles -Source $mapPackSource `
    -Destination (Join-Path $desktopRelease 'map-pack')

$packagedCore = Join-Path $desktopRelease 'pal-core.exe'
$packagedOverlay = Join-Path $desktopRelease 'pal-overlay.exe'
$packagedInjector = Join-Path $desktopRelease 'pal-fullscreen-injector.exe'
$packagedMap = Join-Path $desktopRelease 'map-pack'
& $packagedCore --approved-map-pack-check $packagedMap --game-build-id $gameBuildId
if ($LASTEXITCODE -ne 0) {
    throw 'The packaged Core rejected the exact-build map pack.'
}
& $packagedOverlay --approved-map-pack-check $packagedMap --game-build-id $gameBuildId
if ($LASTEXITCODE -ne 0) {
    throw 'The packaged Overlay rejected the exact-build map pack.'
}
& $packagedInjector --health-check
if ($LASTEXITCODE -ne 0) {
    throw 'The packaged fullscreen injector rejected its renderer DLL set.'
}

$targetHash = Get-Sha256Hex -LiteralPath $sidecarTarget
$coreHash = Get-Sha256Hex -LiteralPath (Join-Path $desktopRelease 'pal-core.exe')
$overlayHash = Get-Sha256Hex -LiteralPath (Join-Path $desktopRelease 'pal-overlay.exe')

Write-Host (
    "Prepared PalBeacon Web, parser=$targetHash, core=$coreHash, " +
    "overlay=$overlayHash, map_build=$gameBuildId"
)
