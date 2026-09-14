[CmdletBinding()]
param(
    [switch] $RequireBuiltWeb,
    [switch] $RequireWindowsBundle,
    [switch] $RequireSignedWindowsBundle
)

$ErrorActionPreference = 'Stop'

function Assert-Condition {
    param(
        [Parameter(Mandatory)][bool] $Condition,
        [Parameter(Mandatory)][string] $Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$uiRoot = Join-Path $repoRoot 'apps\palbeacon-ui'
$desktopRoot = Join-Path $repoRoot 'apps\palbeacon-desktop'
$contractPath = Join-Path $repoRoot 'contracts\palbeacon\cutover.v1.json'
$routesPath = Join-Path $repoRoot 'contracts\palbeacon\routes.v1.json'
$runtimeBuildsPath = Join-Path $repoRoot 'contracts\runtime-builds.json'

Assert-Condition (Test-Path -LiteralPath $contractPath -PathType Leaf) `
    'The PalBeacon cutover contract is missing.'
Assert-Condition (Test-Path -LiteralPath $routesPath -PathType Leaf) `
    'The PalBeacon route contract is missing.'
Assert-Condition (Test-Path -LiteralPath $runtimeBuildsPath -PathType Leaf) `
    'The PalBeacon runtime build contract is missing.'

$cutover = [System.IO.File]::ReadAllText(
    $contractPath,
    [System.Text.Encoding]::UTF8
) | ConvertFrom-Json
$routes = [System.IO.File]::ReadAllText(
    $routesPath,
    [System.Text.Encoding]::UTF8
) | ConvertFrom-Json
$runtimeBuilds = [System.IO.File]::ReadAllText(
    $runtimeBuildsPath,
    [System.Text.Encoding]::UTF8
) | ConvertFrom-Json
$currentGameBuildId = [string] $runtimeBuilds.current_game_build_id
Assert-Condition ($cutover.schema_version -eq 1) 'Unsupported cutover contract version.'
Assert-Condition ($routes.routes.Count -gt 0) 'The route contract is empty.'
Assert-Condition ($currentGameBuildId -match '^\d+$') `
    'The runtime build contract has an invalid current game Build ID.'
$incompleteRoutes = @(
    $routes.routes | Where-Object { $_.migration_status -ne 'implemented' }
)
Assert-Condition ($incompleteRoutes.Count -eq 0) `
    "Routes still carry incomplete migration states: $($incompleteRoutes.id -join ', ')"
Assert-Condition `
    ([string] $cutover.history_reference.checkpoint_commit -match '^[a-f0-9]{40}$') `
    'The legacy checkpoint commit is missing from the cutover contract.'
foreach ($legacyPath in @($cutover.history_reference.removed_executable_surfaces)) {
    Assert-Condition `
        (-not (Test-Path -LiteralPath (Join-Path $repoRoot $legacyPath))) `
        "Removed legacy executable surface returned: $legacyPath"
}

$sourceFiles = Get-ChildItem -LiteralPath (Join-Path $uiRoot 'src') -Recurse -File |
    Where-Object { $_.Extension -in @('.svelte', '.ts', '.js') }
$forbidden = @(
    'package:flutter'
    'MaterialApp'
    'CupertinoApp'
    'Widgetbook'
    'package:forui'
    'package:pixel_ui'
    'MIGRATION QUEUE'
)
foreach ($pattern in $forbidden) {
    $sourceMatches = @($sourceFiles | Select-String -SimpleMatch -Pattern $pattern)
    Assert-Condition ($sourceMatches.Count -eq 0) `
        "Legacy UI marker '$pattern' remains in the canonical Svelte source."
}

if ($RequireBuiltWeb) {
    foreach ($relativePath in @('index.html', '_app')) {
        Assert-Condition `
            (Test-Path -LiteralPath (Join-Path $uiRoot "build\$relativePath")) `
            "The SvelteKit Web build is incomplete: $relativePath"
    }
}

$bundleRoots = @(
    (Join-Path $desktopRoot 'src-tauri\target\release\bundle\nsis')
    (Join-Path $desktopRoot `
        'src-tauri\target\x86_64-pc-windows-msvc\release\bundle\nsis')
)
$windowsBundles = @(
    $bundleRoots |
        Where-Object { Test-Path -LiteralPath $_ -PathType Container } |
        ForEach-Object { Get-ChildItem -LiteralPath $_ -Filter '*.exe' -File }
)
if ($RequireWindowsBundle -or $RequireSignedWindowsBundle) {
    Assert-Condition ($windowsBundles.Count -gt 0) 'The Tauri NSIS bundle is missing.'

    $releaseRoots = @(
        (Join-Path $desktopRoot 'src-tauri\target\release')
        (Join-Path $desktopRoot 'src-tauri\target\x86_64-pc-windows-msvc\release')
    )
    $runtimeRoot = @(
        $releaseRoots |
            Where-Object {
                Test-Path -LiteralPath (Join-Path $_ 'palbeacon-desktop.exe') -PathType Leaf
            }
    ) | Select-Object -First 1
    Assert-Condition (-not [string]::IsNullOrWhiteSpace($runtimeRoot)) `
        'The Tauri Windows release executable is missing.'
    foreach ($relativePath in @(
            'pal-core.exe'
            'pal-overlay.exe'
            'pal-fullscreen-injector.exe'
            'pal-fullscreen-rhi-probe.dll'
            'pal-fullscreen-overlay-dx11.dll'
            'pal-fullscreen-overlay-dx12.dll'
            "map-pack\$currentGameBuildId.active.json"
        )) {
        Assert-Condition `
            (Test-Path -LiteralPath (Join-Path $runtimeRoot $relativePath) -PathType Leaf) `
            "The self-contained Windows runtime is incomplete: $relativePath"
    }
}
if ($RequireSignedWindowsBundle) {
    foreach ($bundle in $windowsBundles) {
        $signature = Get-AuthenticodeSignature -LiteralPath $bundle.FullName
        Assert-Condition ($signature.Status -eq 'Valid') `
            "The public Windows bundle is not Authenticode signed: $($bundle.FullName)"
    }
}

Write-Output "PalBeacon cutover contract verified: $($routes.routes.Count) implemented routes."
