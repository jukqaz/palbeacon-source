#Requires -Version 5.1

[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$rustupHome = Join-Path $repoRoot '.tools\rustup'
$cargoHome = Join-Path $repoRoot '.tools\cargo'
$workerBuild = Join-Path $cargoHome 'bin\worker-build.exe'

if (-not (Test-Path -LiteralPath $workerBuild -PathType Leaf)) {
    throw "Project-managed worker-build was not found: $workerBuild"
}

$env:RUSTUP_HOME = $rustupHome
$env:CARGO_HOME = $cargoHome
$env:Path = "$(Join-Path $cargoHome 'bin');$env:Path"

& $workerBuild --release
if ($LASTEXITCODE -ne 0) {
    throw "worker-build failed with exit code $LASTEXITCODE."
}
