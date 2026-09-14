#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string] $BundleDirectory
)

$ErrorActionPreference = 'Stop'

function Get-WebView2RuntimeVersion {
    $client = 'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    foreach ($key in @(
            "HKLM:\SOFTWARE\WOW6432Node\$client",
            "HKLM:\SOFTWARE\$client",
            "HKCU:\SOFTWARE\$client"
        )) {
        $value = Get-ItemProperty -LiteralPath $key -Name pv -ErrorAction SilentlyContinue
        $version = $null
        if ($null -ne $value -and
            [version]::TryParse([string] $value.pv, [ref] $version) -and $version.Major -gt 0) {
            return $version.ToString()
        }
    }
    return $null
}

# This installs an application only on an ephemeral GitHub Windows runner.
# The normal NSIS installer owns WebView2 preparation; do not weaken its checks.
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or
    $env:GITHUB_RUN_ID -notmatch '^\d+$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^\d+$' -or
    [string]::IsNullOrWhiteSpace($env:RUNNER_TEMP) -or
    -not [System.IO.Path]::IsPathRooted($env:RUNNER_TEMP)) {
    throw 'Bundle installation is restricted to an ephemeral GitHub Windows runner.'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$config = Get-Content -LiteralPath (
    Join-Path $repoRoot 'apps\palbeacon-desktop\src-tauri\tauri.conf.json'
) -Raw | ConvertFrom-Json
$bundleRoot = (Resolve-Path -LiteralPath $BundleDirectory).Path
$installer = Join-Path $bundleRoot "PalBeacon_$($config.version)_x64-setup.exe"
if (-not (Test-Path -LiteralPath $installer -PathType Leaf)) {
    throw 'The current-run PalBeacon NSIS installer is missing.'
}
$runnerRoot = [System.IO.Path]::GetFullPath($env:RUNNER_TEMP).TrimEnd('\', '/')
$installDirectory = Join-Path $runnerRoot (
    "palbeacon-native-e2e-$($env:GITHUB_RUN_ID)-$($env:GITHUB_RUN_ATTEMPT)"
)
if (Test-Path -LiteralPath $installDirectory) {
    throw 'The current-run install directory already exists; refusing to overwrite it.'
}
$beforeRuntime = Get-WebView2RuntimeVersion
Write-Output "WebView2 before installation: $beforeRuntime"
$installerProcess = Start-Process -FilePath $installer -ArgumentList "/S /D=$installDirectory" `
    -WindowStyle Hidden -PassThru
if (-not $installerProcess.WaitForExit(300000)) {
    Stop-Process -Id $installerProcess.Id -ErrorAction SilentlyContinue
    throw 'The PalBeacon NSIS installation did not finish within five minutes.'
}
$installerProcess.Refresh()
if ($installerProcess.ExitCode -ne 0) {
    throw "The PalBeacon NSIS installer failed with exit code $($installerProcess.ExitCode)."
}
$runtimeVersion = Get-WebView2RuntimeVersion
if ([string]::IsNullOrWhiteSpace($runtimeVersion)) {
    throw 'WebView2 is not installed; an Edge browser alone cannot run the native UI test.'
}
$binary = Join-Path $installDirectory 'palbeacon-desktop.exe'
$runtime = Get-Content -LiteralPath (Join-Path $repoRoot 'contracts\runtime-builds.json') `
    -Raw | ConvertFrom-Json
foreach ($relative in @(
        'palbeacon-desktop.exe', 'pal-core.exe', 'pal-overlay.exe',
        'pal-fullscreen-injector.exe', 'pal-save-parser-worker.exe',
        'pal-fullscreen-rhi-probe.dll', 'pal-fullscreen-overlay-dx11.dll',
        'pal-fullscreen-overlay-dx12.dll', 'save-parser-data\pals.json',
        "map-pack\$($runtime.current_game_build_id).active.json"
    )) {
    if (-not (Test-Path -LiteralPath (Join-Path $installDirectory $relative) -PathType Leaf)) {
        throw "The installed bundle is incomplete: $relative"
    }
}
$evidence = Join-Path $repoRoot 'apps\palbeacon-desktop\test-results\installation'
New-Item -ItemType Directory -Path $evidence -Force | Out-Null
[ordered]@{
    installer_sha256 = (Get-FileHash -LiteralPath $installer -Algorithm SHA256).Hash
    binary_sha256 = (Get-FileHash -LiteralPath $binary -Algorithm SHA256).Hash
    webview2_before = $beforeRuntime
    webview2_after = $runtimeVersion
    installer_exit_code = $installerProcess.ExitCode
} | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $evidence 'installed-bundle.json')
Add-Content -LiteralPath $env:GITHUB_ENV -Value "PALBEACON_TAURI_BINARY=$binary"
Write-Output "Verified installed PalBeacon bundle with WebView2 $runtimeVersion."
