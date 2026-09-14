#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $SteamAppId = '1623730',
    [string] $MappingsPath = $env:PAL_USMAP_PATH,
    [string] $BuildId = '24575825',
    [string] $SteamRoot = '',
    [string] $ContractPath,
    [string] $DatasetRoot,
    [Parameter(Mandatory = $true)]
    [string] $LandmarksPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$workspace = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$miseRunner = Join-Path $workspace 'scripts\run-mise.ps1'
$solution = Join-Path $workspace 'tools\pal-map-pack\PalMapPack.slnx'
$project = Join-Path $workspace 'tools\pal-map-pack\src\PalMapPack\PalMapPack.csproj'
if ([string]::IsNullOrWhiteSpace($ContractPath)) {
    $ContractPath = Join-Path $workspace "tools\pal-map-pack\contracts\$BuildId.asset-contract.json"
}
if ([string]::IsNullOrWhiteSpace($DatasetRoot)) {
    if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
        [Console]::Error.WriteLine('LOCALAPPDATA is unavailable; Gate B is NO-GO.')
        exit 16
    }
    $DatasetRoot = Join-Path $env:LOCALAPPDATA 'PalCompanion\datasets'
}

$env:DOTNET_CLI_TELEMETRY_OPTOUT = '1'
& $miseRunner -Workspace $workspace -Run dotnet -RunArgument @(
    'restore', $solution, '--locked-mode'
)
& $miseRunner -Workspace $workspace -Run dotnet -RunArgument @(
    'test', $solution, '-c', 'Release', '--no-restore'
)

$pipeline = @(
    'doctor',
    'probe-assets',
    'accept-contract',
    'stage',
    'calibrate',
    'validate',
    'publish'
)

foreach ($command in $pipeline) {
    $arguments = @(
        'run', '--project', $project, '-c', 'Release', '--no-restore', '--',
        $command,
        '--steam-app-id', $SteamAppId,
        '--build', $BuildId,
        '--mappings', $MappingsPath,
        '--contract', $ContractPath,
        '--dataset-root', $DatasetRoot,
        '--landmarks', $LandmarksPath
    )
    if (-not [string]::IsNullOrWhiteSpace($SteamRoot)) {
        $arguments += @('--steam-root', $SteamRoot)
    }
    try {
        & $miseRunner -Workspace $workspace -Run dotnet -RunArgument $arguments
    } catch {
        [Console]::Error.WriteLine(
            "Gate B NO-GO at $command. No map pack was activated.")
        throw
    }
}

Write-Output 'Gate B PASS: exact-Build pack validated and published.'
exit 0
