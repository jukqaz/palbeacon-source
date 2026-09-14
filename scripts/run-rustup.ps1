#Requires -Version 5.1

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$invocationDirectory = (Get-Location).Path
$RustupArguments = @($args)
$Repository = Join-Path $PSScriptRoot '..'
$Repository = (Resolve-Path -LiteralPath $Repository).Path
$previousRustupHome = $env:RUSTUP_HOME
$previousCargoHome = $env:CARGO_HOME
$previousCargoTargetDirectory = $env:CARGO_TARGET_DIR
$previousCargoIncremental = $env:CARGO_INCREMENTAL
$previousPath = $env:PATH
$configuredCargoTargetDirectory = $env:PAL_CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($configuredCargoTargetDirectory)) {
    $configuredCargoTargetDirectory = [Environment]::GetEnvironmentVariable(
        'PAL_CARGO_TARGET_DIR',
        'User')
}
$resolvedCargoTargetDirectory = $null
if (-not [string]::IsNullOrWhiteSpace($configuredCargoTargetDirectory)) {
    $resolvedCargoTargetDirectory = [IO.Path]::GetFullPath(
        $configuredCargoTargetDirectory)
    if (
        $resolvedCargoTargetDirectory -eq
        [IO.Path]::GetPathRoot($resolvedCargoTargetDirectory)
    ) {
        throw "CARGO target directory cannot be a drive root."
    }
}
$env:RUSTUP_HOME = Join-Path $Repository '.tools\rustup'
$env:CARGO_HOME = Join-Path $Repository '.tools\cargo'
$rustup = Join-Path $env:CARGO_HOME 'bin\rustup.exe'
if (-not (Test-Path -LiteralPath $rustup -PathType Leaf)) {
    throw 'rustup is not installed. Run install-rust-toolchain.ps1.'
}

Push-Location -LiteralPath $invocationDirectory
try {
    $env:PATH = "$(Join-Path $env:CARGO_HOME 'bin');$previousPath"
    if ($null -ne $resolvedCargoTargetDirectory) {
        [void] (New-Item `
            -ItemType Directory `
            -Path $resolvedCargoTargetDirectory `
            -Force)
        $env:CARGO_TARGET_DIR = $resolvedCargoTargetDirectory
    }

    $cargoIndex = [Array]::IndexOf($RustupArguments, 'cargo')
    if ($cargoIndex -ge 0 -and $cargoIndex + 1 -lt $RustupArguments.Count) {
        $cargoCommand = $RustupArguments[$cargoIndex + 1]
        $diskHeavyCommands = @(
            'bench',
            'build',
            'check',
            'clippy',
            'install',
            'run',
            'test'
        )
        if ($diskHeavyCommands -contains $cargoCommand) {
            if ([string]::IsNullOrWhiteSpace($env:CARGO_INCREMENTAL)) {
                $env:CARGO_INCREMENTAL = '0'
            }
            $spacePath = if ($null -ne $resolvedCargoTargetDirectory) {
                $resolvedCargoTargetDirectory
            }
            else {
                $invocationDirectory
            }
            $driveRoot = [IO.Path]::GetPathRoot($spacePath)
            $driveName = $driveRoot.TrimEnd('\').TrimEnd(':')
            $drive = Get-PSDrive -Name $driveName -ErrorAction Stop
            $minimumFreeGiB = 40.0
            if (-not [string]::IsNullOrWhiteSpace(
                $env:PAL_MIN_BUILD_FREE_GIB
            )) {
                $minimumFreeGiB = [double] $env:PAL_MIN_BUILD_FREE_GIB
            }
            $freeGiB = $drive.Free / 1GB
            if ($freeGiB -lt $minimumFreeGiB) {
                throw (
                    "Refusing cargo $cargoCommand with only " +
                    "$([math]::Round($freeGiB, 1)) GiB free on $driveRoot. " +
                    "At least $minimumFreeGiB GiB is required."
                )
            }
        }
    }

    & $rustup @RustupArguments
    if ($LASTEXITCODE -ne 0) {
        throw "rustup failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
    $env:PATH = $previousPath
    if ($null -eq $previousRustupHome) {
        Remove-Item Env:RUSTUP_HOME -ErrorAction SilentlyContinue
    }
    else {
        $env:RUSTUP_HOME = $previousRustupHome
    }
    if ($null -eq $previousCargoHome) {
        Remove-Item Env:CARGO_HOME -ErrorAction SilentlyContinue
    }
    else {
        $env:CARGO_HOME = $previousCargoHome
    }
    if ($null -eq $previousCargoTargetDirectory) {
        Remove-Item Env:CARGO_TARGET_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:CARGO_TARGET_DIR = $previousCargoTargetDirectory
    }
    if ($null -eq $previousCargoIncremental) {
        Remove-Item Env:CARGO_INCREMENTAL -ErrorAction SilentlyContinue
    }
    else {
        $env:CARGO_INCREMENTAL = $previousCargoIncremental
    }
}
