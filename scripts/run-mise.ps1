#Requires -Version 5.1

[CmdletBinding(PositionalBinding = $false)]
param(
    [string] $Workspace,
    [string] $Run,
    [string[]] $RunArgument = @(),
    [Parameter(ValueFromRemainingArguments)]
    [string[]] $MiseArguments
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$invocationDirectory = (Get-Location).Path

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = Join-Path $PSScriptRoot '..'
}
$Workspace = (Resolve-Path -LiteralPath $Workspace).Path
$lock = Get-Content `
    -LiteralPath (Join-Path $Workspace 'quality-tools.lock.json') `
    -Raw |
    ConvertFrom-Json
$metadata = $lock.tools.mise
$toolRoot = Join-Path $Workspace ".tools\quality\mise\$($metadata.version)"
$mise = Get-ChildItem -LiteralPath $toolRoot `
    -Filter ([string] $metadata.executable) `
    -File `
    -Recurse `
    -ErrorAction SilentlyContinue |
    Select-Object -First 1
if ($null -eq $mise) {
    throw 'mise is not installed. Run install-quality-tools.ps1 -Tool mise.'
}

$previousDataDirectory = $env:MISE_DATA_DIR
$previousTrustedPaths = $env:MISE_TRUSTED_CONFIG_PATHS
$previousPath = $env:Path
$previousManagedEnvironment = @{}
$configuredDataDirectory = $env:MISE_DATA_DIR
if ([string]::IsNullOrWhiteSpace($configuredDataDirectory)) {
    $configuredDataDirectory = [Environment]::GetEnvironmentVariable(
        'MISE_DATA_DIR',
        'User')
}
if ([string]::IsNullOrWhiteSpace($configuredDataDirectory)) {
    $configuredDataDirectory = Join-Path $env:LOCALAPPDATA 'mise'
}
$dataDirectory = [IO.Path]::GetFullPath($configuredDataDirectory)
if ($dataDirectory -eq [IO.Path]::GetPathRoot($dataDirectory)) {
    throw 'mise data directory cannot be a drive root.'
}
[void] (New-Item -ItemType Directory -Path $dataDirectory -Force)
Push-Location -LiteralPath $invocationDirectory
try {
    $env:MISE_DATA_DIR = $dataDirectory
    $env:MISE_TRUSTED_CONFIG_PATHS = $Workspace
    if ([string]::IsNullOrWhiteSpace($Run)) {
        & $mise.FullName @MiseArguments
    }
    else {
        # On native Windows, `mise exec` can resolve the first npm.cmd while
        # omitting the managed Node directory from the child PATH. Apply the
        # complete mise environment explicitly so nested npm scripts and .NET
        # child processes inherit the same managed runtime selection.
        $managedEnvironmentJson = @(
            & $mise.FullName env --json 2>$null
        ) -join [Environment]::NewLine
        if ($LASTEXITCODE -ne 0) {
            throw 'mise could not resolve the managed tool environment.'
        }
        $managedEnvironment = $managedEnvironmentJson | ConvertFrom-Json
        foreach ($property in $managedEnvironment.PSObject.Properties) {
            $name = [string] $property.Name
            $previousManagedEnvironment[$name] = (
                [Environment]::GetEnvironmentVariable($name, 'Process')
            )
            Set-Item -LiteralPath "Env:$name" -Value ([string] $property.Value)
        }

        $resolvedToolOutput = @(& $mise.FullName which $Run 2>$null)
        $whichExitCode = $LASTEXITCODE
        $resolvedTool = $resolvedToolOutput | Select-Object -Last 1
        if (
            $whichExitCode -eq 0 -and
            -not [string]::IsNullOrWhiteSpace($resolvedTool) -and
            (Test-Path -LiteralPath $resolvedTool)
        ) {
            $toolDirectory = Split-Path -Parent $resolvedTool
            $env:Path = "$toolDirectory;$env:Path"
        }
        else {
            throw "mise could not resolve the requested tool: $Run"
        }
        $resolvedExecutable = $resolvedTool
        foreach ($extension in @('.exe', '.cmd', '.bat')) {
            $candidate = "$resolvedTool$extension"
            if (Test-Path -LiteralPath $candidate -PathType Leaf) {
                $resolvedExecutable = $candidate
                break
            }
        }
        & $resolvedExecutable @RunArgument
    }
    if ($LASTEXITCODE -ne 0) {
        throw "mise failed with exit code $LASTEXITCODE."
    }
}
finally {
    Pop-Location
    if ($null -eq $previousDataDirectory) {
        Remove-Item Env:MISE_DATA_DIR -ErrorAction SilentlyContinue
    }
    else {
        $env:MISE_DATA_DIR = $previousDataDirectory
    }
    if ($null -eq $previousTrustedPaths) {
        Remove-Item Env:MISE_TRUSTED_CONFIG_PATHS -ErrorAction SilentlyContinue
    }
    else {
        $env:MISE_TRUSTED_CONFIG_PATHS = $previousTrustedPaths
    }
    foreach ($name in $previousManagedEnvironment.Keys) {
        $previousValue = $previousManagedEnvironment[$name]
        if ($null -eq $previousValue) {
            Remove-Item -LiteralPath "Env:$name" -ErrorAction SilentlyContinue
        }
        else {
            Set-Item -LiteralPath "Env:$name" -Value $previousValue
        }
    }
    $env:Path = $previousPath
}
