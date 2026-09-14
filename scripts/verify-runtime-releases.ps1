#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $RepositoryRoot,
    [switch] $Online
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($RepositoryRoot)) {
    $RepositoryRoot = Join-Path $PSScriptRoot '..'
}
$RepositoryRoot = (Resolve-Path -LiteralPath $RepositoryRoot).Path
$toolchains = Get-Content `
    -LiteralPath (Join-Path $RepositoryRoot 'toolchains.lock.json') `
    -Raw |
    ConvertFrom-Json
$qualityTools = Get-Content `
    -LiteralPath (Join-Path $RepositoryRoot 'quality-tools.lock.json') `
    -Raw |
    ConvertFrom-Json
$globalJson = Get-Content `
    -LiteralPath (Join-Path $RepositoryRoot 'global.json') `
    -Raw |
    ConvertFrom-Json
$miseConfig = Get-Content `
    -LiteralPath (Join-Path $RepositoryRoot 'mise.toml') `
    -Raw
$rustConfig = Get-Content `
    -LiteralPath (Join-Path $RepositoryRoot 'rust-toolchain.toml') `
    -Raw

$errors = [System.Collections.Generic.List[string]]::new()

function Assert-ReleaseValue {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [AllowEmptyString()] [string] $Actual,
        [Parameter(Mandatory)] [string] $Expected
    )

    if ($Actual -cne $Expected) {
        $errors.Add("$Name expected '$Expected' but found '$Actual'.")
    }
}

function Get-ReleaseText {
    param([Parameter(Mandatory)] [string] $Uri)

    try {
        return $web.DownloadString($Uri)
    }
    catch {
        throw "Runtime release lookup failed for '$Uri': $(
            $_.Exception.InnerException.Message
        )"
    }
}

function Get-GitHubLatestReleaseTag {
    param([Parameter(Mandatory)] [string] $Repository)

    $uri = "https://github.com/$Repository/releases/latest"
    $request = [Net.HttpWebRequest]::Create($uri)
    $request.Method = 'HEAD'
    $request.UserAgent = 'PalCompanion-Runtime-Policy'
    $request.AllowAutoRedirect = $true
    $response = $null
    try {
        $response = $request.GetResponse()
        return $response.ResponseUri.AbsolutePath.TrimEnd('/').Split('/')[-1]
    }
    catch {
        throw "Runtime release lookup failed for '$uri': $(
            $_.Exception.Message
        )"
    }
    finally {
        if ($null -ne $response) {
            $response.Dispose()
        }
    }
}

Assert-ReleaseValue `
    -Name 'Rust release track' `
    -Actual ([string] $toolchains.rust.release_track) `
    -Expected 'stable'
Assert-ReleaseValue `
    -Name '.NET release track' `
    -Actual ([string] $toolchains.dotnet.release_track) `
    -Expected 'lts'
Assert-ReleaseValue `
    -Name 'mise release track' `
    -Actual ([string] $toolchains.mise.release_track) `
    -Expected 'stable'
Assert-ReleaseValue `
    -Name 'mise manager pin' `
    -Actual ([string] $qualityTools.tools.mise.version) `
    -Expected ([string] $toolchains.mise.version)
Assert-ReleaseValue `
    -Name 'rustup manager pin' `
    -Actual ([string] $qualityTools.tools.rustup_init.version) `
    -Expected ([string] $toolchains.rust.manager_version)
Assert-ReleaseValue `
    -Name '.NET global.json pin' `
    -Actual ([string] $globalJson.sdk.version) `
    -Expected ([string] $toolchains.dotnet.sdk)
Assert-ReleaseValue `
    -Name '.NET roll-forward policy' `
    -Actual ([string] $globalJson.sdk.rollForward) `
    -Expected 'disable'

foreach ($entry in @(
    @{
        Name = 'mise .NET pin'
        Text = $miseConfig
        Pattern = "(?m)^dotnet\s*=\s*`"$([regex]::Escape(
            [string] $toolchains.dotnet.sdk
        ))`"\s*$"
    },
    @{
        Name = 'mise minimum version'
        Text = $miseConfig
        Pattern = "(?m)^min_version\s*=\s*`"$([regex]::Escape(
            [string] $toolchains.mise.version
        ))`"\s*$"
    },
    @{
        Name = 'mise Node pin'
        Text = $miseConfig
        Pattern = "(?m)^node\s*=\s*\{\s*version\s*=\s*`"$([regex]::Escape(
            [string] $toolchains.node.version
        ))`""
    },
    @{
        Name = 'mise pnpm pin'
        Text = $miseConfig
        Pattern = "(?m)^pnpm\s*=\s*`"$([regex]::Escape(
            [string] $toolchains.node.pnpm
        ))`"\s*$"
    },
    @{
        Name = 'Rust toolchain pin'
        Text = $rustConfig
        Pattern = "(?m)^channel\s*=\s*`"$([regex]::Escape(
            [string] $toolchains.rust.version
        ))`"\s*$"
    }
)) {
    if ([string] $entry.Text -notmatch [string] $entry.Pattern) {
        $errors.Add("$($entry.Name) does not match the toolchain lock.")
    }
}

if ($Online) {
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
    $web = [System.Net.WebClient]::new()
    $web.Headers['User-Agent'] = 'PalCompanion-Runtime-Policy'
    try {
        $rustManifest = Get-ReleaseText -Uri (
            'https://static.rust-lang.org/dist/channel-rust-stable.toml'
        )
        $rustMatch = [regex]::Match(
            $rustManifest,
            '(?m)^\[pkg\.rust\]\r?\nversion = "(?<version>\d+\.\d+\.\d+) '
        )
        if (-not $rustMatch.Success) {
            $errors.Add('Rust stable manifest did not contain a version.')
        }
        else {
            Assert-ReleaseValue `
                -Name 'Rust latest stable' `
                -Actual ([string] $toolchains.rust.version) `
                -Expected $rustMatch.Groups['version'].Value
        }

        $dotnetIndex = Get-ReleaseText -Uri (
            'https://builds.dotnet.microsoft.com/dotnet/release-metadata/releases-index.json'
        ) | ConvertFrom-Json
        $dotnetChannel = $dotnetIndex.'releases-index' |
            Where-Object {
                $_.'channel-version' -ceq [string] $toolchains.dotnet.channel
            } |
            Select-Object -First 1
        Assert-ReleaseValue `
            -Name '.NET channel release type' `
            -Actual ([string] $dotnetChannel.'release-type') `
            -Expected 'lts'
        $dotnetReleases = Get-ReleaseText -Uri (
            [string] $dotnetChannel.'releases.json'
        ) |
            ConvertFrom-Json
        Assert-ReleaseValue `
            -Name '.NET latest LTS SDK' `
            -Actual ([string] $toolchains.dotnet.sdk) `
            -Expected ([string] $dotnetReleases.'latest-sdk')

        $miseRelease = Get-GitHubLatestReleaseTag -Repository 'jdx/mise'
        Assert-ReleaseValue `
            -Name 'mise latest stable release' `
            -Actual ([string] $toolchains.mise.version) `
            -Expected ([string] $miseRelease).TrimStart('v')

    }
    finally {
        $web.Dispose()
    }
}

if ($errors.Count -gt 0) {
    throw "Runtime release policy verification failed:`n- $(
        $errors -join "`n- "
    )"
}

$mode = if ($Online) { 'online' } else { 'offline' }
Write-Output "Runtime release policy verification passed ($mode)."
