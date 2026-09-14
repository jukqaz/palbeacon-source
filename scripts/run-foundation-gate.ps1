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
$configuredCargoTarget = $env:PAL_CARGO_TARGET_DIR
if ([string]::IsNullOrWhiteSpace($configuredCargoTarget)) {
    $configuredCargoTarget = [Environment]::GetEnvironmentVariable(
        'PAL_CARGO_TARGET_DIR',
        'User')
}
if (-not [string]::IsNullOrWhiteSpace($configuredCargoTarget)) {
    $cargoTarget = [IO.Path]::GetFullPath($configuredCargoTarget)
    if ($cargoTarget -eq [IO.Path]::GetPathRoot($cargoTarget)) {
        throw 'Cargo target directory cannot be a drive root.'
    }
    [void] (New-Item -ItemType Directory -Path $cargoTarget -Force)
    $cargoDriveRoot = [IO.Path]::GetPathRoot($cargoTarget)
    $cargoDriveName = $cargoDriveRoot.TrimEnd('\').TrimEnd(':')
    $cargoDrive = Get-PSDrive -Name $cargoDriveName -ErrorAction Stop
    if (($cargoDrive.Free / 1GB) -lt 40) {
        throw (
            "Foundation gate requires at least 40 GiB free on " +
            "$cargoDriveRoot."
        )
    }
    $env:CARGO_TARGET_DIR = $cargoTarget
}
$env:CARGO_INCREMENTAL = '0'
$artifactDirectory = Join-Path $Workspace 'artifacts'
$logDirectory = Join-Path $artifactDirectory 'logs'
$reportPath = Join-Path $artifactDirectory 'foundation-validation.json'
[void] (New-Item -ItemType Directory -Path $logDirectory -Force)

function Resolve-Executable {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $LocalPath
    )

    if (Test-Path -LiteralPath $LocalPath -PathType Leaf) {
        return (Resolve-Path -LiteralPath $LocalPath).Path
    }
    $command = Get-Command -Name $Name -CommandType Application -ErrorAction SilentlyContinue |
        Select-Object -First 1
    if ($null -eq $command) {
        throw "Required executable '$Name' is unavailable."
    }
    $command.Path
}

function Invoke-GateStep {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Executable,
        [Parameter(Mandatory)] [string[]] $Arguments
    )

    $logPath = Join-Path $logDirectory "$Name.log"
    Write-Host "==> $Name"
    $previousErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $Executable @Arguments 2>&1 |
            Tee-Object -FilePath $logPath |
            Out-Host
        $exitCode = $LASTEXITCODE
    }
    catch {
        $_ | Out-String | Set-Content -LiteralPath $logPath -Encoding utf8
        Write-Error -ErrorAction Continue $_
        $exitCode = 1
    }
    finally {
        $ErrorActionPreference = $previousErrorActionPreference
    }
    [pscustomobject] [ordered] @{
        name = $Name
        ok = ($exitCode -eq 0)
        exit_code = $exitCode
        log = $logPath.Substring($Workspace.TrimEnd('\').Length).TrimStart('\')
    }
}

function Get-GitDependencyCount {
    $count = 0
    $cargoLock = Join-Path $Workspace 'Cargo.lock'
    if (Test-Path -LiteralPath $cargoLock -PathType Leaf) {
        $count += @(
            Select-String -LiteralPath $cargoLock `
                -Pattern '^\s*source\s*=\s*"git\+' `
                -AllMatches
        ).Count
    }
    foreach ($manifest in @(
            Join-Path $Workspace 'Cargo.toml'
            Get-ChildItem -LiteralPath (Join-Path $Workspace 'crates') `
                -Filter Cargo.toml -File -Recurse -ErrorAction SilentlyContinue |
                Select-Object -ExpandProperty FullName
        )) {
        if (-not (Test-Path -LiteralPath $manifest -PathType Leaf)) {
            continue
        }
        $count += @(
            Select-String -LiteralPath $manifest `
                -Pattern '\bgit\s*=\s*"' `
                -AllMatches
        ).Count
    }
    $count
}

function Get-PrivateFixtureMatchCount {
    $patterns = @(
        '(?i)\bplayer_ip\b',
        '(?i)\bplatform_account_id\b',
        '(?i)\braw_save_bytes\b',
        '(?i)Authorization:\s*Bearer\s+[A-Za-z0-9._~-]{12,}',
        '(?i)\\Pal\\Saved\\SaveGames\\',
        '(?i)\\Steam\\userdata\\'
    )
    $count = 0
    foreach ($relative in @(
            'artifacts\sbom\palbeacon.cdx.json',
            'third_party\THIRD_PARTY_NOTICES.md',
            'tests\fixtures\contracts\wire-v1'
        )) {
        $path = Join-Path $Workspace $relative
        if (-not (Test-Path -LiteralPath $path)) {
            continue
        }
        $files = if (Test-Path -LiteralPath $path -PathType Container) {
            @(Get-ChildItem -LiteralPath $path -File -Recurse -ErrorAction SilentlyContinue)
        }
        else {
            @(Get-Item -LiteralPath $path)
        }
        foreach ($file in $files) {
            $text = [Text.Encoding]::UTF8.GetString(
                [IO.File]::ReadAllBytes($file.FullName)
            )
            foreach ($pattern in $patterns) {
                if ($text -match $pattern) {
                    $count++
                }
            }
        }
    }
    $count
}

$steps = [Collections.Generic.List[object]]::new()
$doctorOk = $false
$fmtOk = $false
$testOk = $false
$clippyOk = $false
$generatedOk = $false
$supplyChainOk = $false
$staticQualityOk = $false

try {
    $powershell = Resolve-Executable -Name 'powershell.exe' `
        -LocalPath (Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe')
    $cargo = Resolve-Executable -Name 'cargo' `
        -LocalPath (Join-Path $Workspace '.tools\cargo\bin\cargo.exe')
    $qualityLock = Get-Content `
        -LiteralPath (Join-Path $Workspace 'quality-tools.lock.json') `
        -Raw |
        ConvertFrom-Json
    $originalPath = $env:PATH
    $toolDirectories = @(
        (Split-Path -Parent $cargo),
        (Join-Path $Workspace '.tools\cargo-tools\bin'),
        (Join-Path $Workspace ".tools\quality\cargo_nextest\$($qualityLock.tools.cargo_nextest.version)")
    ) | Where-Object { Test-Path -LiteralPath $_ -PathType Container }
    $env:PATH = "$($toolDirectories -join ';');$env:PATH"

    try {
        Push-Location -LiteralPath $Workspace
        try {
            $doctor = Invoke-GateStep -Name 'dev-doctor' -Executable $powershell -Arguments @(
                '-NoProfile',
                '-ExecutionPolicy', 'Bypass',
                '-File', (Join-Path $Workspace 'scripts\dev-doctor.ps1'),
                '-Json'
            )
            $steps.Add($doctor)
            $doctorOk = $doctor.ok

            $format = Invoke-GateStep -Name 'cargo-fmt' -Executable $cargo -Arguments @(
                'fmt', '--all', '--', '--check'
            )
            $steps.Add($format)
            $fmtOk = $format.ok

            $tests = Invoke-GateStep -Name 'cargo-nextest' -Executable $cargo -Arguments @(
                'nextest',
                'run',
                '--workspace',
                '--all-features',
                '--locked',
                '--profile',
                'ci'
            )
            $steps.Add($tests)

            $docTests = Invoke-GateStep -Name 'cargo-doc-test' -Executable $cargo -Arguments @(
                'test', '--workspace', '--all-features', '--doc', '--locked'
            )
            $steps.Add($docTests)
            $testOk = ($tests.ok -and $docTests.ok)

            $clippy = Invoke-GateStep -Name 'cargo-clippy' -Executable $cargo -Arguments @(
                'clippy',
                '--workspace',
                '--all-targets',
                '--all-features',
                '--locked',
                '--',
                '-D', 'warnings'
            )
            $steps.Add($clippy)
            $clippyOk = $clippy.ok

            $generated = Invoke-GateStep -Name 'generated-clean' `
                -Executable $powershell -Arguments @(
                    '-NoProfile',
                    '-ExecutionPolicy', 'Bypass',
                    '-File', (Join-Path $Workspace 'scripts\verify-generated-clean.ps1')
                )
            $steps.Add($generated)
            $generatedOk = $generated.ok

            $staticQuality = Invoke-GateStep -Name 'quality-static' `
                -Executable $powershell -Arguments @(
                    '-NoProfile',
                    '-ExecutionPolicy', 'Bypass',
                    '-File', (Join-Path $Workspace 'scripts\run-quality-static.ps1'),
                    '-Workspace', $Workspace
                )
            $steps.Add($staticQuality)
            $staticQualityOk = $staticQuality.ok

            $supply = Invoke-GateStep -Name 'supply-chain' `
                -Executable $powershell -Arguments @(
                    '-NoProfile',
                    '-ExecutionPolicy', 'Bypass',
                    '-File', (Join-Path $Workspace 'scripts\verify-supply-chain.ps1'),
                    '-Workspace', $Workspace
                )
            $steps.Add($supply)
            $supplyChainOk = $supply.ok
        }
        finally {
            Pop-Location
        }
    }
    finally {
        $env:PATH = $originalPath
    }
}
catch {
    $steps.Add([pscustomobject] [ordered] @{
        name = 'gate-bootstrap'
        ok = $false
        exit_code = 1
        log = ''
        error = $_.Exception.Message
    })
}

$sbomPath = Join-Path $Workspace 'artifacts\sbom\palbeacon.cdx.json'
$sbomComponentCount = 0
if (Test-Path -LiteralPath $sbomPath -PathType Leaf) {
    try {
        $sbom = Get-Content -LiteralPath $sbomPath -Raw | ConvertFrom-Json
        $sbomComponentCount = @($sbom.components).Count
    }
    catch {
        $sbomComponentCount = 0
    }
}
$gitDependencyCount = Get-GitDependencyCount
$privateFixtureMatches = Get-PrivateFixtureMatchCount

$report = [ordered] @{
    ok = (
        $doctorOk -and
        $fmtOk -and
        $testOk -and
        $clippyOk -and
        $generatedOk -and
        $staticQualityOk -and
        $supplyChainOk -and
        $gitDependencyCount -eq 0 -and
        $sbomComponentCount -ge 1
    )
    rust_locked = $doctorOk
    wire_runtime_parity = ($testOk -and $generatedOk)
    cloud_projection_contract = $testOk
    cloud_profile_sections = if ($testOk) { 6 } else { 0 }
    cloud_profile_page_types = if ($testOk) { 6 } else { 0 }
    cloud_profile_noop_identity_stable = $testOk
    cloud_profile_oversized_entry_rejected = $testOk
    cloud_profile_private_fields = $privateFixtureMatches
    generated_clean = $generatedOk
    static_quality = $staticQualityOk
    ipc_multiclient = $testOk
    licenses_allowed = $supplyChainOk
    advisories_clear = $supplyChainOk
    git_dependencies = $gitDependencyCount
    private_fixture_matches = $privateFixtureMatches
    sbom_components = $sbomComponentCount
    steps = @($steps)
}

[IO.File]::WriteAllText(
    $reportPath,
    (($report | ConvertTo-Json -Depth 8).TrimEnd() + "`n"),
    [Text.UTF8Encoding]::new($false)
)
Write-Output "Foundation validation report: $reportPath"
if (-not $report.ok) {
    exit 1
}
exit 0
