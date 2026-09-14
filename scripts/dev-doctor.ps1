#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $LockFile,
    [switch] $Json
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($LockFile)) {
    $LockFile = Join-Path $workspace 'toolchains.lock.json'
}

function New-Check {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [bool] $Found,
        [AllowNull()] [string] $Version,
        [AllowNull()] [string] $Path,
        [bool] $Locked
    )
    [pscustomobject] [ordered] @{
        name = $Name
        found = $Found
        version = $Version
        path = $Path
        locked = $Locked
    }
}

function Get-Application {
    param(
        [Parameter(Mandatory)] [string[]] $Names,
        [string[]] $PreferredPaths = @()
    )
    foreach ($path in $PreferredPaths) {
        if (Test-Path -LiteralPath $path -PathType Leaf) {
            return (Resolve-Path -LiteralPath $path).Path
        }
    }
    foreach ($name in $Names) {
        $command = Get-Command -Name $name -CommandType Application `
            -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($null -ne $command) {
            return $command.Path
        }
    }
    return $null
}

function Get-Version {
    param(
        [AllowNull()] [string] $Executable,
        [string[]] $Arguments,
        [Parameter(Mandatory)] [string] $Pattern
    )
    if ($null -eq $Executable) {
        return $null
    }
    try {
        $output = (& $Executable @Arguments 2>&1 | Out-String).Trim()
        $match = [regex]::Match($output, $Pattern)
        if ($match.Success) {
            return $match.Groups['version'].Value
        }
    }
    catch {
        return $null
    }
    return $null
}

try {
    if (-not (Test-Path -LiteralPath $LockFile -PathType Leaf)) {
        throw [IO.FileNotFoundException]::new('Toolchain lock is missing.')
    }
    $lock = Get-Content -LiteralPath $LockFile -Raw | ConvertFrom-Json
    if ($lock.schema_version -ne 1) {
        throw [IO.InvalidDataException]::new('Unsupported toolchain lock schema.')
    }
    foreach ($entry in @(
            @($lock.rust, 'version'),
            @($lock.rust, 'target'),
            @($lock.node, 'version'),
            @($lock.node, 'pnpm'),
            @($lock.dotnet, 'sdk')
        )) {
        $value = $entry[0].PSObject.Properties[[string] $entry[1]].Value
        if ($value -isnot [string] -or [string]::IsNullOrWhiteSpace($value)) {
            throw [IO.InvalidDataException]::new('Toolchain lock shape is invalid.')
        }
    }
}
catch {
    $failure = @(New-Check -Name 'Toolchain lock' -Found $false `
        -Version $null -Path $LockFile -Locked $false)
    if ($Json) { $failure | ConvertTo-Json -Depth 3 } else { $failure | Format-Table -AutoSize }
    exit 1
}

$cargo = Get-Application -Names @('cargo.exe', 'cargo') -PreferredPaths @(
    (Join-Path $workspace '.tools\cargo\bin\cargo.exe')
)
$rustc = Get-Application -Names @('rustc.exe', 'rustc') -PreferredPaths @(
    (Join-Path $workspace '.tools\cargo\bin\rustc.exe')
)
$node = Get-Application -Names @('node.exe', 'node')
$pnpm = Get-Application -Names @('pnpm.cmd', 'pnpm.exe', 'pnpm')
$dotnet = Get-Application -Names @('dotnet.exe', 'dotnet')
$git = Get-Application -Names @('git.exe', 'git')

$rustVersion = Get-Version -Executable $rustc -Arguments @('--version') `
    -Pattern '^rustc\s+(?<version>\d+\.\d+\.\d+)'
$cargoVersion = Get-Version -Executable $cargo -Arguments @('--version') `
    -Pattern '^cargo\s+(?<version>\d+\.\d+\.\d+)'
$nodeVersion = Get-Version -Executable $node -Arguments @('--version') `
    -Pattern '^v(?<version>\d+\.\d+\.\d+)'
$pnpmVersion = Get-Version -Executable $pnpm -Arguments @('--version') `
    -Pattern '^(?<version>\d+\.\d+\.\d+)'
$dotnetVersion = Get-Version -Executable $dotnet -Arguments @('--version') `
    -Pattern '^(?<version>\d+\.\d+\.\d+)'
$gitVersion = Get-Version -Executable $git -Arguments @('--version') `
    -Pattern '^git version\s+(?<version>\S+)'

$checks = @(
    New-Check -Name 'Rust' -Found ($null -ne $rustVersion) `
        -Version $rustVersion -Path $rustc `
        -Locked ($rustVersion -ceq [string] $lock.rust.version)
    New-Check -Name 'Cargo' -Found ($null -ne $cargoVersion) `
        -Version $cargoVersion -Path $cargo `
        -Locked ($null -ne $cargoVersion -and $rustVersion -ceq [string] $lock.rust.version)
    New-Check -Name 'Node' -Found ($null -ne $nodeVersion) `
        -Version $nodeVersion -Path $node `
        -Locked ($nodeVersion -ceq [string] $lock.node.version)
    New-Check -Name 'pnpm' -Found ($null -ne $pnpmVersion) `
        -Version $pnpmVersion -Path $pnpm `
        -Locked ($pnpmVersion -ceq [string] $lock.node.pnpm)
    New-Check -Name '.NET SDK' -Found ($null -ne $dotnetVersion) `
        -Version $dotnetVersion -Path $dotnet `
        -Locked ($dotnetVersion -ceq [string] $lock.dotnet.sdk)
    New-Check -Name 'Git' -Found ($null -ne $gitVersion) `
        -Version $gitVersion -Path $git -Locked ($null -ne $gitVersion)
)

if ($Json) {
    $checks | ConvertTo-Json -Depth 3
}
else {
    $checks | Format-Table -AutoSize
}
if (@($checks | Where-Object { -not $_.found -or -not $_.locked }).Count -gt 0) {
    exit 1
}
