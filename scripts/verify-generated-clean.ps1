[CmdletBinding()]
param()

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$workspaceRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$cargo = Get-Command -Name 'cargo' -CommandType Application -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($null -eq $cargo) {
    throw "Required executable 'cargo' is unavailable."
}

Push-Location -LiteralPath $workspaceRoot
try {
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $cargo.Path run -p pal-wire --bin write-wire-goldens --locked -- -Check
        $goldenExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    if ($goldenExitCode -ne 0) {
        throw "Committed wire goldens are stale; fixture check exited with $goldenExitCode."
    }

    & $cargo.Path run `
        -p pal-map-contract `
        --bin generate-map-contract `
        --locked `
        -- `
        --check
    if ($LASTEXITCODE -ne 0) {
        throw 'Committed map contract outputs are stale.'
    }
} finally {
    Pop-Location
}
