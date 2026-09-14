#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $Workspace,

    [ValidateSet(
        'buf',
        'cargo_nextest',
        'cargo_llvm_cov',
        'cargo_hack',
        'cargo_machete',
        'actionlint',
        'psscriptanalyzer',
        'pester',
        'osv_scanner',
        'mise',
        'rustup_init'
    )]
    [string[]] $Tool = @(
        'buf',
        'cargo_nextest',
        'cargo_llvm_cov',
        'cargo_hack',
        'cargo_machete',
        'actionlint',
        'psscriptanalyzer',
        'pester',
        'osv_scanner',
        'mise',
        'rustup_init'
    )
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = Join-Path $PSScriptRoot '..'
}
$Workspace = (Resolve-Path -LiteralPath $Workspace).Path
$lockPath = Join-Path $Workspace 'quality-tools.lock.json'
$lock = Get-Content -LiteralPath $lockPath -Raw | ConvertFrom-Json
if ($lock.schema_version -ne 1 -or $null -eq $lock.tools) {
    throw 'Unsupported quality-tools lock.'
}

$qualityRoot = Join-Path $Workspace '.tools\quality'
$downloadRoot = Join-Path $qualityRoot 'downloads'
[void] (New-Item -ItemType Directory -Path $downloadRoot -Force)

[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

function Get-VerifiedDownload {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [pscustomobject] $Metadata
    )

    $extension = switch ([string] $Metadata.kind) {
        'file' { '.exe' }
        'tar_gz' { '.tar.gz' }
        default { '.zip' }
    }
    $destination = Join-Path $downloadRoot "$Name-$($Metadata.version)$extension"
    if (-not (Test-Path -LiteralPath $destination -PathType Leaf)) {
        Invoke-WebRequest -Uri ([string] $Metadata.url) `
            -OutFile $destination `
            -UseBasicParsing
    }
    $actual = (Get-FileHash -LiteralPath $destination -Algorithm SHA256).
        Hash.ToLowerInvariant()
    $expected = ([string] $Metadata.sha256).ToLowerInvariant()
    if ($actual -ne $expected) {
        throw "Quality tool checksum mismatch for $Name`: $actual"
    }
    return $destination
}

function Remove-StaleToolVersion {
    [CmdletBinding(SupportsShouldProcess)]
    param(
        [Parameter(Mandatory)] [string] $ToolName,
        [Parameter(Mandatory)] [string] $CurrentVersion,
        [Parameter(Mandatory)] [string] $CurrentDownload
    )

    $toolRoot = Join-Path $qualityRoot $ToolName
    if (Test-Path -LiteralPath $toolRoot -PathType Container) {
        $resolvedToolRoot = [IO.Path]::GetFullPath($toolRoot)
        Get-ChildItem -LiteralPath $resolvedToolRoot -Directory |
            Where-Object { $_.Name -ne $CurrentVersion } |
            ForEach-Object {
                $candidate = [IO.Path]::GetFullPath($_.FullName)
                $parent = [IO.Directory]::GetParent($candidate)
                if (
                    $null -eq $parent -or
                    $parent.FullName -ne $resolvedToolRoot
                ) {
                    throw "Refusing stale tool path outside $resolvedToolRoot"
                }
                if (
                    $PSCmdlet.ShouldProcess(
                        $candidate,
                        "Remove stale $ToolName version"
                    )
                ) {
                    Remove-Item -LiteralPath $candidate -Recurse -Force
                    Write-Output "Removed stale $ToolName version: $candidate"
                }
            }
    }

    $resolvedDownload = [IO.Path]::GetFullPath($CurrentDownload)
    Get-ChildItem -LiteralPath $downloadRoot -File -Filter "$ToolName-*" |
        Where-Object {
            [IO.Path]::GetFullPath($_.FullName) -ne $resolvedDownload
        } |
        ForEach-Object {
            if (
                $PSCmdlet.ShouldProcess(
                    $_.FullName,
                    "Remove stale $ToolName download"
                )
            ) {
                Remove-Item -LiteralPath $_.FullName -Force
                Write-Output "Removed stale $ToolName download: $($_.FullName)"
            }
        }
}

foreach ($toolName in $Tool) {
    $property = $lock.tools.PSObject.Properties[$toolName]
    if ($null -eq $property -or $property.Value -isnot [pscustomobject]) {
        throw "Quality tool is absent from the lock: $toolName"
    }
    $metadata = $property.Value
    $download = Get-VerifiedDownload -Name $toolName -Metadata $metadata

    if ([string] $metadata.kind -eq 'powershell_module') {
        $moduleName = if ($toolName -eq 'psscriptanalyzer') {
            'PSScriptAnalyzer'
        }
        elseif ($toolName -eq 'pester') {
            'Pester'
        }
        else {
            throw "Unsupported PowerShell module package: $toolName"
        }
        $destination = Join-Path $Workspace `
            ".tools\powershell\$moduleName\$($metadata.version)"
        [void] (New-Item -ItemType Directory -Path $destination -Force)
        Expand-Archive -LiteralPath $download -DestinationPath $destination -Force
        $manifest = Join-Path $destination ([string] $metadata.module_manifest)
        if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
            throw "$moduleName manifest is missing after extraction: $manifest"
        }
        Write-Output "Installed $toolName $($metadata.version): $manifest"
        continue
    }

    $destination = Join-Path $qualityRoot "$toolName\$($metadata.version)"
    [void] (New-Item -ItemType Directory -Path $destination -Force)
    if ([string] $metadata.kind -eq 'zip') {
        Expand-Archive -LiteralPath $download -DestinationPath $destination -Force
    }
    elseif ([string] $metadata.kind -eq 'tar_gz') {
        & tar.exe -xzf $download -C $destination
        if ($LASTEXITCODE -ne 0) {
            throw "Failed to extract quality tool archive: $toolName"
        }
    }
    elseif ([string] $metadata.kind -eq 'file') {
        Copy-Item -LiteralPath $download `
            -Destination (Join-Path $destination ([string] $metadata.executable)) `
            -Force
    }
    else {
        throw "Unsupported quality tool package kind: $($metadata.kind)"
    }

    $executable = Get-ChildItem -LiteralPath $destination `
        -Filter ([string] $metadata.executable) `
        -File `
        -Recurse |
        Select-Object -First 1
    if ($null -eq $executable) {
        throw "Quality tool executable is missing after extraction: $toolName"
    }
    Write-Output "Installed $toolName $($metadata.version): $($executable.FullName)"
    Remove-StaleToolVersion `
        -ToolName $toolName `
        -CurrentVersion ([string] $metadata.version) `
        -CurrentDownload $download
}
