#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $Workspace,
    [string] $OutputPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($Workspace)) {
    $Workspace = Join-Path $PSScriptRoot '..'
}
$Workspace = (Resolve-Path -LiteralPath $Workspace).Path
if ([string]::IsNullOrWhiteSpace($OutputPath)) {
    $OutputPath = Join-Path $Workspace 'artifacts\sbom\palbeacon.cdx.json'
}
elseif (-not [IO.Path]::IsPathRooted($OutputPath)) {
    $OutputPath = Join-Path $Workspace $OutputPath
}
$OutputPath = [IO.Path]::GetFullPath($OutputPath)
$outputDirectory = Split-Path -Parent $OutputPath
[void] (New-Item -ItemType Directory -Path $outputDirectory -Force)

function Get-RelativeNormalizedPath {
    param([Parameter(Mandatory)] [string] $Path)

    $workspaceUri = [Uri] ($Workspace.TrimEnd('\') + '\')
    $pathUri = [Uri] ([IO.Path]::GetFullPath($Path))
    [Uri]::UnescapeDataString($workspaceUri.MakeRelativeUri($pathUri).ToString())
}

function New-Purl {
    param(
        [Parameter(Mandatory)] [string] $Ecosystem,
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Version
    )

    $encodedName = switch ($Ecosystem) {
        'npm' {
            if ($Name.StartsWith('@')) {
                '%40' + $Name.Substring(1)
            }
            else {
                $Name
            }
        }
        default { [Uri]::EscapeDataString($Name) }
    }
    "pkg:$Ecosystem/$encodedName@$([Uri]::EscapeDataString($Version))"
}

function Add-Component {
    param(
        [Parameter(Mandatory)] [hashtable] $Table,
        [Parameter(Mandatory)] [string] $Ecosystem,
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Version,
        [AllowNull()] [string] $License
    )

    if ([string]::IsNullOrWhiteSpace($Name) -or
        [string]::IsNullOrWhiteSpace($Version)) {
        throw "SBOM component name and version are required."
    }
    $purl = New-Purl -Ecosystem $Ecosystem -Name $Name -Version $Version
    if ($Table.ContainsKey($purl)) {
        return
    }
    $component = [ordered] @{
        type = 'library'
        'bom-ref' = $purl
        name = $Name
        version = $Version
        purl = $purl
    }
    if (-not [string]::IsNullOrWhiteSpace($License)) {
        $component.licenses = @(
            [ordered] @{ expression = $License }
        )
    }
    $Table[$purl] = $component
}

function Get-LockFiles {
    $files = [Collections.Generic.List[string]]::new()
    $git = Get-Command -Name 'git' -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -ne $git) {
        Push-Location -LiteralPath $Workspace
        try {
            $tracked = @(& $git.Path ls-files 2>$null)
        }
        finally {
            Pop-Location
        }
        foreach ($relative in $tracked) {
            if ([IO.Path]::GetFileName($relative) -notin @(
                    'Cargo.lock',
                    'packages.lock.json',
                    'pnpm-lock.yaml'
                )) {
                continue
            }
            $path = Join-Path $Workspace $relative
            if (Test-Path -LiteralPath $path -PathType Leaf) {
                $files.Add((Resolve-Path -LiteralPath $path).Path)
            }
        }
    }
    else {
        foreach ($relative in @('Cargo.lock', 'pnpm-lock.yaml')) {
            $path = Join-Path $Workspace $relative
            if (Test-Path -LiteralPath $path -PathType Leaf) {
                $files.Add((Resolve-Path -LiteralPath $path).Path)
            }
        }
    }
    foreach ($relativeRoot in @('packages', 'tools', 'cloudflare')) {
        $searchRoot = Join-Path $Workspace $relativeRoot
        if (-not (Test-Path -LiteralPath $searchRoot -PathType Container)) {
            continue
        }
        foreach ($pattern in @('packages.lock.json')) {
            Get-ChildItem -LiteralPath $searchRoot -Filter $pattern -File -Recurse `
                -ErrorAction SilentlyContinue |
                Where-Object {
                    $_.FullName -notmatch '[\\/](?:target|artifacts|node_modules|bin|obj)[\\/]'
                } |
                ForEach-Object { $files.Add($_.FullName) }
        }
    }
    $unique = @{}
    foreach ($file in $files) {
        $unique[$file] = $true
    }
    [string[]] $sorted = @($unique.Keys)
    [Array]::Sort($sorted, [StringComparer]::Ordinal)
    @($sorted)
}

function Get-DeterministicSerial {
    param([Parameter(Mandatory)] [string[]] $LockFiles)

    $material = [Text.StringBuilder]::new()
    foreach ($lock in $LockFiles) {
        $relative = Get-RelativeNormalizedPath -Path $lock
        $hash = (Get-FileHash -LiteralPath $lock -Algorithm SHA256).Hash.ToLowerInvariant()
        [void] $material.Append($relative).Append('=').Append($hash).Append("`n")
    }
    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes($material.ToString())
        $hex = ([BitConverter]::ToString($sha.ComputeHash($bytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha.Dispose()
    }
    $uuid = '{0}-{1}-5{2}-{3}{4}-{5}' -f
        $hex.Substring(0, 8),
        $hex.Substring(8, 4),
        $hex.Substring(13, 3),
        @('8', '9', 'a', 'b')[[Convert]::ToInt32($hex.Substring(16, 1), 16) % 4],
        $hex.Substring(17, 3),
        $hex.Substring(20, 12)
    "urn:uuid:$uuid"
}

$localCargo = Join-Path $Workspace '.tools\cargo\bin\cargo.exe'
$cargo = if (Test-Path -LiteralPath $localCargo -PathType Leaf) {
    Get-Item -LiteralPath $localCargo
}
else {
    Get-Command -Name 'cargo' -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
}
if ($null -eq $cargo) {
    throw "Required executable 'cargo' is unavailable."
}
$cargoPath = if ($cargo -is [IO.FileInfo]) {
    $cargo.FullName
}
else {
    $cargo.Path
}

Push-Location -LiteralPath $Workspace
try {
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $metadataText = (& $cargoPath metadata --locked --format-version 1 | Out-String)
        $metadataExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    if ($metadataExitCode -ne 0) {
        throw "cargo metadata --locked failed: $metadataText"
    }
}
finally {
    Pop-Location
}
$metadata = $metadataText | ConvertFrom-Json
$components = @{}
$workspaceMembers = @{}
foreach ($member in @($metadata.workspace_members)) {
    $workspaceMembers[[string] $member] = $true
}
foreach ($package in @($metadata.packages)) {
    if ([string]::IsNullOrWhiteSpace([string] $package.source) -and
        $workspaceMembers.ContainsKey([string] $package.id)) {
        continue
    }
    Add-Component -Table $components -Ecosystem 'cargo' `
        -Name ([string] $package.name) `
        -Version ([string] $package.version) `
        -License ([string] $package.license)
}

$lockFiles = @(Get-LockFiles)
foreach ($lock in $lockFiles | Where-Object { $_ -like '*packages.lock.json' }) {
    $document = Get-Content -LiteralPath $lock -Raw | ConvertFrom-Json
    foreach ($target in $document.dependencies.PSObject.Properties) {
        foreach ($entry in $target.Value.PSObject.Properties) {
            $value = $entry.Value
            $resolvedProperty = $value.PSObject.Properties['resolved']
            $typeProperty = $value.PSObject.Properties['type']
            if ($null -eq $resolvedProperty -or
                ($null -ne $typeProperty -and $typeProperty.Value -eq 'Project')) {
                continue
            }
            Add-Component -Table $components -Ecosystem 'nuget' `
                -Name ([string] $entry.Name) `
                -Version ([string] $resolvedProperty.Value) `
                -License $null
        }
    }
}

[string[]] $componentPurls = @($components.Keys)
[Array]::Sort($componentPurls, [StringComparer]::Ordinal)
$sortedComponents = @($componentPurls | ForEach-Object { $components[$_] })
$bom = [ordered] @{
    bomFormat = 'CycloneDX'
    specVersion = '1.6'
    serialNumber = Get-DeterministicSerial -LockFiles $lockFiles
    version = 1
    metadata = [ordered] @{
        component = [ordered] @{
            type = 'application'
            'bom-ref' = 'pkg:generic/palbeacon@0.1.0'
            name = 'palbeacon'
            version = '0.1.0'
            purl = 'pkg:generic/palbeacon@0.1.0'
        }
        properties = @(
            [ordered] @{
                name = 'palbeacon:lockfile-count'
                value = [string] $lockFiles.Count
            }
        )
    }
    components = $sortedComponents
}

$json = $bom | ConvertTo-Json -Depth 12
[IO.File]::WriteAllText(
    $OutputPath,
    ($json.TrimEnd() + "`n"),
    [Text.UTF8Encoding]::new($false)
)
Write-Output $OutputPath
