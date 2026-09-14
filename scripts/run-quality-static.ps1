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

function Resolve-QualityExecutable {
    param([Parameter(Mandatory)] [string] $Name)

    $metadata = $lock.tools.PSObject.Properties[$Name].Value
    $directory = Join-Path $Workspace ".tools\quality\$Name\$($metadata.version)"
    $executable = Get-ChildItem -LiteralPath $directory `
        -Filter ([string] $metadata.executable) `
        -File `
        -Recurse `
        -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $executable) {
        throw "Quality tool is not installed: $Name"
    }
    return $executable.FullName
}

$bufMetadata = $lock.tools.buf
$buf = Resolve-QualityExecutable -Name 'buf'
$bufVersion = (& $buf --version 2>&1 | Out-String).Trim()
if ($bufVersion -ne [string] $bufMetadata.version) {
    throw "buf version mismatch: $bufVersion"
}
$env:BUF_CACHE_DIR = Join-Path $Workspace '.tools\quality\buf-cache'
[void] (New-Item -ItemType Directory -Path $env:BUF_CACHE_DIR -Force)
$git = Get-Command git -ErrorAction Stop
$gitRoot = Split-Path -Parent (Split-Path -Parent $git.Source)
$diffDirectory = Join-Path $gitRoot 'usr\bin'
if (Test-Path -LiteralPath (Join-Path $diffDirectory 'diff.exe')) {
    $env:Path = "$diffDirectory;$env:Path"
}

Push-Location -LiteralPath $Workspace
try {
    & $buf format --diff --exit-code
    if ($LASTEXITCODE -ne 0) {
        throw "buf format failed with exit code $LASTEXITCODE."
    }
    & $buf lint
    if ($LASTEXITCODE -ne 0) {
        throw "buf lint failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

$actionlintMetadata = $lock.tools.actionlint
$actionlint = Resolve-QualityExecutable -Name 'actionlint'
$actionlintVersion = (& $actionlint -version 2>&1 | Out-String).Trim()
if ($actionlintVersion -notmatch "^$([regex]::Escape([string] $actionlintMetadata.version))(?:\s|$)") {
    throw "actionlint version mismatch: $actionlintVersion"
}

Push-Location -LiteralPath $Workspace
try {
    & $actionlint
    if ($LASTEXITCODE -ne 0) {
        throw "actionlint failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
}

$analyzerMetadata = $lock.tools.psscriptanalyzer
$moduleManifest = Join-Path $Workspace `
    ".tools\powershell\PSScriptAnalyzer\$($analyzerMetadata.version)\$($analyzerMetadata.module_manifest)"
if (-not (Test-Path -LiteralPath $moduleManifest -PathType Leaf)) {
    throw "PSScriptAnalyzer is not installed: $moduleManifest"
}
Import-Module $moduleManifest -Force -ErrorAction Stop
$loadedVersion = (Get-Module PSScriptAnalyzer).Version.ToString()
if ($loadedVersion -ne [string] $analyzerMetadata.version) {
    throw "PSScriptAnalyzer version mismatch: $loadedVersion"
}

$settings = Join-Path $Workspace '.config\PSScriptAnalyzerSettings.psd1'
$findings = @(
    Invoke-ScriptAnalyzer `
        -Path (Join-Path $Workspace 'scripts') `
        -Recurse `
        -Settings $settings
)
if ($findings.Count -gt 0) {
    $findings |
        Format-Table ScriptName, Line, Severity, RuleName, Message -Wrap |
        Out-Host
    throw "PSScriptAnalyzer reported $($findings.Count) correctness findings."
}

& (Join-Path $PSScriptRoot 'verify-runtime-releases.ps1') `
    -RepositoryRoot $Workspace

Write-Output 'Static quality verification passed.'
