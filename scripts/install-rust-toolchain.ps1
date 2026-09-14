#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $Workspace
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = Join-Path $PSScriptRoot '..'
}
$Workspace = (Resolve-Path -LiteralPath $Workspace).Path
$lock = Get-Content `
    -LiteralPath (Join-Path $Workspace 'quality-tools.lock.json') `
    -Raw |
    ConvertFrom-Json
$toolchains = Get-Content `
    -LiteralPath (Join-Path $Workspace 'toolchains.lock.json') `
    -Raw |
    ConvertFrom-Json
$metadata = $lock.tools.rustup_init
$rust = $toolchains.rust
$installerRoot = Join-Path $Workspace `
    ".tools\quality\rustup_init\$($metadata.version)"
$installer = Get-ChildItem `
    -LiteralPath $installerRoot `
    -Filter ([string] $metadata.executable) `
    -File `
    -Recurse `
    -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($null -eq $installer) {
    throw @'
rustup-init is not installed. Run:
  install-quality-tools.ps1 -Tool rustup_init
'@
}

$previousRustupHome = $env:RUSTUP_HOME
$previousCargoHome = $env:CARGO_HOME
$env:RUSTUP_HOME = Join-Path $Workspace '.tools\rustup'
$env:CARGO_HOME = Join-Path $Workspace '.tools\cargo'
$rustup = Join-Path $env:CARGO_HOME 'bin\rustup.exe'

try {
    if (-not (Test-Path -LiteralPath $rustup -PathType Leaf)) {
        & $installer.FullName `
            -y `
            --no-modify-path `
            --profile minimal `
            --default-toolchain none
        if ($LASTEXITCODE -ne 0) {
            throw "rustup-init failed with exit code $LASTEXITCODE."
        }
    }

    $installArguments = @(
        'toolchain',
        'install',
        "$($rust.version)-$($rust.target)",
        '--profile',
        'minimal',
        '--component',
        (@($rust.components) -join ','),
        '--target',
        [string] $rust.wasm_target
    )
    & $rustup @installArguments
    if ($LASTEXITCODE -ne 0) {
        throw "rustup toolchain install failed with exit code $LASTEXITCODE."
    }
}
finally {
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
}
