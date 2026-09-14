#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $Workspace,

    [string] $Against = '.git#branch=main,subdir=proto'
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
$metadata = $lock.tools.buf
$directory = Join-Path $Workspace `
    ".tools\quality\buf\$($metadata.version)"
$buf = Get-ChildItem -LiteralPath $directory `
    -Filter ([string] $metadata.executable) `
    -File `
    -Recurse `
    -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($null -eq $buf) {
    throw 'buf is not installed. Run scripts/install-quality-tools.ps1 -Tool buf.'
}
$env:BUF_CACHE_DIR = Join-Path $Workspace '.tools\quality\buf-cache'
[void] (New-Item -ItemType Directory -Path $env:BUF_CACHE_DIR -Force)

Push-Location -LiteralPath $Workspace
try {
    & $buf.FullName breaking --against $Against
    if ($LASTEXITCODE -ne 0) {
        throw "buf breaking failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

Write-Output "Protobuf compatibility verified against $Against."
