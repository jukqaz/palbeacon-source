$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

function Assert-True {
    param(
        [Parameter(Mandatory = $true)]
        [bool] $Condition,
        [Parameter(Mandatory = $true)]
        [string] $Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]
        [object] $Expected,
        [Parameter(Mandatory = $true)]
        [object] $Actual,
        [Parameter(Mandatory = $true)]
        [string] $Message
    )
    if ([string]$Expected -cne [string]$Actual) {
        throw "$Message (expected '$Expected', actual '$Actual')"
    }
}

function Invoke-Runner {
    param(
        [Parameter(Mandatory = $true)]
        [string] $RunnerPath,
        [Parameter(Mandatory = $true)]
        [string] $MapPath,
        [Parameter(Mandatory = $true)]
        [string] $StdoutPath,
        [Parameter(Mandatory = $true)]
        [string] $StderrPath
    )
    $previousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        & $script:WindowsPowerShell `
            -NoProfile `
            -NonInteractive `
            -ExecutionPolicy Bypass `
            -File $RunnerPath `
            -MapPath $MapPath `
            -TimeoutSeconds 15 `
            1> $StdoutPath `
            2> $StderrPath
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    return $exitCode
}

function Reset-Scenario {
    param([Parameter(Mandatory = $true)][string[]] $Paths)
    foreach ($path in $Paths) {
        if (Test-Path -LiteralPath $path) {
            Remove-Item -LiteralPath $path -Force
        }
    }
}

function Get-LoggedArguments {
    param([Parameter(Mandatory = $true)][string] $Path)
    if (-not (Test-Path -LiteralPath $Path)) {
        return @()
    }
    return @(Get-Content -LiteralPath $Path)
}

$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$sourceRunner = Join-Path $workspace 'scripts\run-local-alignment-smoke.ps1'
Assert-True (Test-Path -LiteralPath $sourceRunner -PathType Leaf) `
    'runner script must exist before contract tests can execute'

$script:WindowsPowerShell = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
Assert-True (Test-Path -LiteralPath $script:WindowsPowerShell -PathType Leaf) `
    'Windows PowerShell 5.1 is required for the runner contract'

$testRoot = Join-Path ([IO.Path]::GetTempPath()) (
    'pal local alignment runner tests ' + [Guid]::NewGuid().ToString('N'))
$syntheticRoot = Join-Path $testRoot 'synthetic-repo'
$runnerPath = Join-Path $syntheticRoot 'scripts\run-local-alignment-smoke.ps1'
$calibratorPath = Join-Path $syntheticRoot (
    'tools\pal-map-pack\src\PalMapPack.Calibrator\bin\Release\' +
    'net10.0-windows\PalMapPack.Calibrator.exe')
$rustPath = Join-Path $syntheticRoot (
    'artifacts\pal-overlay-build-24181527-dev\pal-overlay.exe')
$mapPath = Join-Path $syntheticRoot (
    'artifacts\pal-overlay-build-24181527-dev\' +
    'palworld-mainmap-24181527-2048.bmp')
$eventLog = Join-Path $testRoot 'events.txt'
$captureArgsLog = Join-Path $testRoot 'capture-args.txt'
$rustArgsLog = Join-Path $testRoot 'rust-args.txt'
$stdoutPath = Join-Path $testRoot 'stdout.txt'
$stderrPath = Join-Path $testRoot 'stderr.txt'

$fakeProgram = @'
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;

public static class FakeSmokeChild
{
    private const string Observation =
        "{\"schema\":\"pal_companion.local_alignment_observation.v1\"," +
        "\"claim\":\"independent_native_marker_observation_not_gate_b\"," +
        "\"game_build_id\":24181527," +
        "\"map_sha256\":\"aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b\"," +
        "\"map_width_px\":2048,\"map_height_px\":2048," +
        "\"observed_marker_x_px\":100.25,\"observed_marker_y_px\":199.75," +
        "\"nonce\":\"000102030405060708090a0b0c0d0e0f\"}";

    public static int Main(string[] args)
    {
        bool capture = Path.GetFileName(
            Environment.GetCommandLineArgs()[0]).IndexOf(
                "Calibrator", StringComparison.OrdinalIgnoreCase) >= 0;
        Append(Environment.GetEnvironmentVariable("PAL_SMOKE_TEST_EVENT_LOG"),
            capture ? "capture" : "rust");
        File.WriteAllLines(
            Environment.GetEnvironmentVariable(
                capture
                    ? "PAL_SMOKE_TEST_CAPTURE_ARGS"
                    : "PAL_SMOKE_TEST_RUST_ARGS"),
            args,
            new UTF8Encoding(false));

        if (capture)
        {
            string output = ValueAfter(args, "--output");
            Append(
                Environment.GetEnvironmentVariable("PAL_SMOKE_TEST_EVENT_LOG"),
                "capture-diracl=" + (IsOwnerOnlyDirectory(
                    Path.GetDirectoryName(output)) ? "private" : "insecure"));
            if (Environment.GetEnvironmentVariable(
                    "PAL_SMOKE_TEST_CAPTURE_WRITE") == "1")
            {
                File.WriteAllBytes(output, new UTF8Encoding(false).GetBytes(Observation));
                ProtectFile(output);
                if (Environment.GetEnvironmentVariable(
                        "PAL_SMOKE_TEST_CAPTURE_RESIDUE") == "1")
                {
                    File.WriteAllText(
                        Path.Combine(Path.GetDirectoryName(output), "unexpected-residue"),
                        "residue",
                        new UTF8Encoding(false));
                }
            }
            return ParseExit("PAL_SMOKE_TEST_CAPTURE_EXIT");
        }

        string observation = ValueAfter(args, "--observation-file");
        string expectedHash = ValueAfter(args, "--observation-sha256");
        Append(
            Environment.GetEnvironmentVariable("PAL_SMOKE_TEST_EVENT_LOG"),
            "rust-diracl=" + (IsOwnerOnlyDirectory(
                Path.GetDirectoryName(observation)) ? "private" : "insecure"));
        Append(
            Environment.GetEnvironmentVariable("PAL_SMOKE_TEST_EVENT_LOG"),
            "rust-fileacl=" + (IsOwnerOnlyFile(observation) ? "private" : "insecure"));
        Append(
            Environment.GetEnvironmentVariable("PAL_SMOKE_TEST_EVENT_LOG"),
            "hash=" + (Hash(observation) == expectedHash ? "match" : "mismatch"));
        Console.Out.Write(Environment.GetEnvironmentVariable("PAL_SMOKE_TEST_RUST_STDOUT"));
        return ParseExit("PAL_SMOKE_TEST_RUST_EXIT");
    }

    private static string ValueAfter(string[] args, string flag)
    {
        int index = Array.IndexOf(args, flag);
        if (index < 0 || index + 1 >= args.Length)
        {
            throw new InvalidOperationException("missing flag");
        }
        return args[index + 1];
    }

    private static int ParseExit(string name)
    {
        int value;
        return int.TryParse(Environment.GetEnvironmentVariable(name), out value)
            ? value
            : 0;
    }

    private static void Append(string path, string value)
    {
        File.AppendAllText(path, value + Environment.NewLine, new UTF8Encoding(false));
    }

    private static string Hash(string path)
    {
        using (SHA256 sha = SHA256.Create())
        using (FileStream stream = File.OpenRead(path))
        {
            return BitConverter.ToString(sha.ComputeHash(stream))
                .Replace("-", "")
                .ToLowerInvariant();
        }
    }

    private static void ProtectFile(string path)
    {
        SecurityIdentifier user = WindowsIdentity.GetCurrent().User;
        FileSecurity security = new FileSecurity();
        security.SetOwner(user);
        security.SetAccessRuleProtection(true, false);
        security.AddAccessRule(new FileSystemAccessRule(
            user,
            FileSystemRights.FullControl,
            AccessControlType.Allow));
        File.SetAccessControl(path, security);
    }

    private static bool IsOwnerOnlyFile(string path)
    {
        return IsOwnerOnly(File.GetAccessControl(
            path,
            AccessControlSections.Owner | AccessControlSections.Access));
    }

    private static bool IsOwnerOnlyDirectory(string path)
    {
        return IsOwnerOnly(Directory.GetAccessControl(
            path,
            AccessControlSections.Owner | AccessControlSections.Access));
    }

    private static bool IsOwnerOnly(FileSystemSecurity security)
    {
        SecurityIdentifier user = WindowsIdentity.GetCurrent().User;
        if (!security.AreAccessRulesProtected ||
            !user.Equals(security.GetOwner(typeof(SecurityIdentifier))))
        {
            return false;
        }
        AuthorizationRuleCollection rules = security.GetAccessRules(
            true,
            true,
            typeof(SecurityIdentifier));
        if (rules.Count != 1)
        {
            return false;
        }
        FileSystemAccessRule rule = (FileSystemAccessRule)rules[0];
        return user.Equals(rule.IdentityReference) &&
            rule.AccessControlType == AccessControlType.Allow &&
            rule.FileSystemRights == FileSystemRights.FullControl &&
            !rule.IsInherited;
    }
}
'@

$savedEnvironment = @{}
foreach ($name in @(
    'PAL_SMOKE_TEST_EVENT_LOG',
    'PAL_SMOKE_TEST_CAPTURE_ARGS',
    'PAL_SMOKE_TEST_RUST_ARGS',
    'PAL_SMOKE_TEST_CAPTURE_WRITE',
    'PAL_SMOKE_TEST_CAPTURE_RESIDUE',
    'PAL_SMOKE_TEST_CAPTURE_EXIT',
    'PAL_SMOKE_TEST_RUST_EXIT',
    'PAL_SMOKE_TEST_RUST_STDOUT')) {
    $savedEnvironment[$name] = [Environment]::GetEnvironmentVariable($name)
}

try {
    New-Item -ItemType Directory -Path (Split-Path $runnerPath) -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path $calibratorPath) -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path $rustPath) -Force | Out-Null
    Copy-Item -LiteralPath $sourceRunner -Destination $runnerPath
    [IO.File]::WriteAllBytes($mapPath, [byte[]](0x42, 0x4d, 0x00, 0x00))
    Add-Type `
        -TypeDefinition $fakeProgram `
        -Language CSharp `
        -OutputAssembly $calibratorPath `
        -OutputType ConsoleApplication
    Copy-Item -LiteralPath $calibratorPath -Destination $rustPath

    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_EVENT_LOG', $eventLog)
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_ARGS', $captureArgsLog)
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_ARGS', $rustArgsLog)

    Reset-Scenario @(
        $eventLog,
        $captureArgsLog,
        $rustArgsLog,
        $stdoutPath,
        $stderrPath)
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_WRITE', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_RESIDUE', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_EXIT', '23')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_EXIT', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_STDOUT', 'must-not-run')
    $exitCode = Invoke-Runner $runnerPath $mapPath $stdoutPath $stderrPath
    Assert-Equal 23 $exitCode 'capture exit code must propagate exactly'
    Assert-Equal 0 (Get-Item $stdoutPath).Length 'capture failure stdout must be empty'
    $events = @(Get-Content $eventLog)
    Assert-Equal 2 $events.Count 'capture failure must not launch Rust'
    Assert-Equal 'capture' $events[0] 'capture failure must run capture once'
    Assert-Equal 'capture-diracl=private' $events[1] `
        'private directory must exist securely before failed capture'
    Assert-True (-not (Test-Path -LiteralPath $rustArgsLog)) `
        'Rust arguments must not exist after capture failure'

    Reset-Scenario @(
        $eventLog,
        $captureArgsLog,
        $rustArgsLog,
        $stdoutPath,
        $stderrPath)
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_WRITE', '1')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_RESIDUE', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_EXIT', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_EXIT', '29')
    [Environment]::SetEnvironmentVariable(
        'PAL_SMOKE_TEST_RUST_STDOUT',
        '{"private":"must-remain-buffered"}')
    $exitCode = Invoke-Runner $runnerPath $mapPath $stdoutPath $stderrPath
    Assert-Equal 29 $exitCode 'Rust exit code must propagate exactly'
    Assert-Equal 0 (Get-Item $stdoutPath).Length 'Rust failure stdout must stay buffered'
    $events = @(Get-Content $eventLog)
    Assert-Equal 'capture' $events[0] 'capture must run first'
    Assert-Equal 'capture-diracl=private' $events[1] `
        'temporary directory must be owner-only before capture'
    Assert-Equal 'rust' $events[2] 'Rust must run only after capture'
    Assert-Equal 'rust-diracl=private' $events[3] `
        'temporary directory must remain owner-only'
    Assert-Equal 'rust-fileacl=private' $events[4] `
        'observation must be owner-only before Rust'
    Assert-Equal 'hash=match' $events[5] `
        'Rust must receive the exact post-capture observation hash'

    $captureArguments = Get-LoggedArguments $captureArgsLog
    Assert-Equal 7 $captureArguments.Count 'capture must receive exactly seven arguments'
    Assert-Equal '--local-alignment-capture' $captureArguments[0] 'capture mode'
    Assert-Equal '--build' $captureArguments[1] 'capture Build flag'
    Assert-Equal '24181527' $captureArguments[2] 'capture exact Build'
    Assert-Equal '--map' $captureArguments[3] 'capture map flag'
    Assert-Equal ([IO.Path]::GetFullPath($mapPath)) $captureArguments[4] 'capture map'
    Assert-Equal '--output' $captureArguments[5] 'capture output flag'
    $observationPath = $captureArguments[6]
    Assert-True (-not (Test-Path -LiteralPath $observationPath)) `
        'observation must be removed after Rust failure'
    Assert-True (-not (Test-Path -LiteralPath (Split-Path $observationPath))) `
        'private temporary directory must be removed after Rust failure'

    $rustArguments = Get-LoggedArguments $rustArgsLog
    Assert-Equal 9 $rustArguments.Count 'Rust must receive exactly nine arguments'
    Assert-Equal '--development-local-alignment-diagnostic-json' $rustArguments[0] `
        'Rust diagnostic mode'
    Assert-Equal '--real-map-bmp' $rustArguments[1] 'Rust map flag'
    Assert-Equal ([IO.Path]::GetFullPath($mapPath)) $rustArguments[2] 'Rust map'
    Assert-Equal '--observation-file' $rustArguments[3] 'Rust observation flag'
    Assert-Equal $observationPath $rustArguments[4] 'Rust observation path'
    Assert-Equal '--observation-sha256' $rustArguments[5] 'Rust hash flag'
    Assert-True ($rustArguments[6] -cmatch '^[0-9a-f]{64}$') 'Rust exact hash'
    Assert-Equal '--timeout-seconds' $rustArguments[7] 'Rust timeout flag'
    Assert-Equal '15' $rustArguments[8] 'Rust timeout'
    foreach ($forbidden in @(
        '--synthetic-map',
        '--position-replay',
        '--development-local-readonly-position',
        '--development-local-live-performance-diagnostic-json')) {
        Assert-True (-not ($rustArguments -contains $forbidden)) `
            "runner must not add $forbidden"
    }

    Reset-Scenario @(
        $eventLog,
        $captureArgsLog,
        $rustArgsLog,
        $stdoutPath,
        $stderrPath)
    $successLine =
        '{"schema":"pal_companion.local_alignment_smoke.v1",' +
        '"claim":"development_smoke_only_not_gate_b",' +
        '"gate_b_approved":false,"build_id":24181527,' +
        '"map_sha256":"aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b",' +
        '"sample_count":10,"sample_window_ms":900,' +
        '"projected_spread_px":0.5,"residual_px":5.0,' +
        '"within_development_smoke_threshold":true}'
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_RESIDUE', '1')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_EXIT', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_STDOUT', $successLine)
    $exitCode = Invoke-Runner $runnerPath $mapPath $stdoutPath $stderrPath
    Assert-Equal 1 $exitCode 'cleanup failure must fail closed'
    Assert-Equal 0 (Get-Item $stdoutPath).Length `
        'cleanup failure must suppress buffered Rust stdout'
    $rustArguments = Get-LoggedArguments $rustArgsLog
    $observationPath = $rustArguments[4]
    $privateDirectory = Split-Path $observationPath
    Assert-True (-not (Test-Path -LiteralPath $observationPath)) `
        'cleanup must remove the known observation nonrecursively'
    Assert-True (Test-Path -LiteralPath (
        Join-Path $privateDirectory 'unexpected-residue')) `
        'cleanup must refuse recursive deletion of unexpected residue'
    Remove-Item -LiteralPath $privateDirectory -Recurse -Force

    Reset-Scenario @(
        $eventLog,
        $captureArgsLog,
        $rustArgsLog,
        $stdoutPath,
        $stderrPath)
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_RESIDUE', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_EXIT', '0')
    [Environment]::SetEnvironmentVariable(
        'PAL_SMOKE_TEST_RUST_STDOUT',
        $successLine + "`n`n")
    $exitCode = Invoke-Runner $runnerPath $mapPath $stdoutPath $stderrPath
    Assert-Equal 1 $exitCode 'multiple diagnostic lines must fail closed'
    Assert-Equal 0 (Get-Item $stdoutPath).Length `
        'noncanonical Rust stdout must remain buffered'

    Reset-Scenario @(
        $eventLog,
        $captureArgsLog,
        $rustArgsLog,
        $stdoutPath,
        $stderrPath)
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_CAPTURE_RESIDUE', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_EXIT', '0')
    [Environment]::SetEnvironmentVariable('PAL_SMOKE_TEST_RUST_STDOUT', $successLine)
    $exitCode = Invoke-Runner $runnerPath $mapPath $stdoutPath $stderrPath
    Assert-Equal 0 $exitCode 'successful Rust exit must propagate'
    Assert-Equal $successLine ((Get-Content $stdoutPath) -join '') `
        'success must pass through exactly one validated identity-free line'
    $rustArguments = Get-LoggedArguments $rustArgsLog
    $observationPath = $rustArguments[4]
    foreach ($secret in @(
        $observationPath,
        $rustArguments[6],
        '000102030405060708090a0b0c0d0e0f')) {
        Assert-True (
            -not ([IO.File]::ReadAllText($stdoutPath).Contains($secret))) `
            'stdout must not contain observation path, hash, or private content'
    }
    Assert-True (-not (Test-Path -LiteralPath $observationPath)) `
        'successful observation must be removed before stdout publication'
    Assert-True (-not (Test-Path -LiteralPath (Split-Path $observationPath))) `
        'successful private directory must be removed before stdout publication'

    Write-Output 'run-local-alignment-smoke contract tests passed'
}
finally {
    foreach ($name in $savedEnvironment.Keys) {
        [Environment]::SetEnvironmentVariable($name, $savedEnvironment[$name])
    }
    if (Test-Path -LiteralPath $testRoot) {
        Remove-Item -LiteralPath $testRoot -Recurse -Force
    }
}
