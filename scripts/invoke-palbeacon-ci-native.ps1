#Requires -Version 5.1
[CmdletBinding()]
param(
    [switch] $Restricted,
    [string] $EvidenceDirectory
)

$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_OS -ne 'Windows' -or
    $env:GITHUB_RUN_ID -notmatch '^\d+$' -or $env:GITHUB_RUN_ATTEMPT -notmatch '^\d+$') {
    throw 'This standard-user test launcher is restricted to ephemeral GitHub Windows runners.'
}
$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$evidenceRoot = Join-Path $repoRoot 'apps\palbeacon-desktop\test-results\standard-user'

if ($Restricted) {
    if ([string]::IsNullOrWhiteSpace($EvidenceDirectory) -or
        [System.IO.Path]::GetDirectoryName([System.IO.Path]::GetFullPath($EvidenceDirectory)) -ne $evidenceRoot) {
        throw 'A current-run evidence directory is required.'
    }
    $exitCode = 1
    try {
        $principal = [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent())
        if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
            throw 'The native test still has Administrator privileges.'
        }
        $groups = & "$env:SystemRoot\System32\whoami.exe" /groups /fo csv /nh
        $integrity = [regex]::Match(($groups -join "`n"), 'S-1-16-(\d+)')
        if ($LASTEXITCODE -ne 0 -or -not $integrity.Success -or
            [int]$integrity.Groups[1].Value -lt 8192) {
            throw 'The native test requires a readable medium-or-higher source integrity label.'
        }
        if ([int]$integrity.Groups[1].Value -gt 8192) {
            # runas NORMALUSER removes administrator groups, but can retain High IL
            # on UAC-disabled runners. Lower only this restricted process to Medium.
            Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;
public static class PalBeaconRestrictedIntegrity {
    [StructLayout(LayoutKind.Sequential)]
    struct Label { public IntPtr Sid; public uint Attributes; }
    [DllImport("kernel32.dll")] static extern IntPtr GetCurrentProcess();
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr pointer);
    [DllImport("advapi32.dll", SetLastError=true)]
    static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);
    [DllImport("advapi32.dll", CharSet=CharSet.Unicode, SetLastError=true)]
    static extern bool ConvertStringSidToSid(string value, out IntPtr sid);
    [DllImport("advapi32.dll")] static extern uint GetLengthSid(IntPtr sid);
    [DllImport("advapi32.dll", SetLastError=true)]
    static extern bool SetTokenInformation(IntPtr token, int infoClass, ref Label label, uint length);
    public static void LowerToMedium() {
        IntPtr token = IntPtr.Zero, sid = IntPtr.Zero;
        try {
            if (!OpenProcessToken(GetCurrentProcess(), 0x0088, out token))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            if (!ConvertStringSidToSid("S-1-16-8192", out sid))
                throw new Win32Exception(Marshal.GetLastWin32Error());
            var label = new Label { Sid = sid, Attributes = 0x20 };
            if (!SetTokenInformation(token, 25, ref label, (uint)Marshal.SizeOf(typeof(Label)) + GetLengthSid(sid)))
                throw new Win32Exception(Marshal.GetLastWin32Error());
        } finally {
            if (sid != IntPtr.Zero) LocalFree(sid);
            if (token != IntPtr.Zero) CloseHandle(token);
        }
    }
}
'@
            [PalBeaconRestrictedIntegrity]::LowerToMedium()
        }
        $verifiedGroups = & "$env:SystemRoot\System32\whoami.exe" /groups /fo csv /nh
        if ($LASTEXITCODE -ne 0 -or ($verifiedGroups -join "`n") -notmatch 'S-1-16-8192\b') {
            throw 'The native test must run at standard medium integrity.'
        }
        Set-Location -LiteralPath $repoRoot
        'Standard-user medium-integrity context verified.' | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'identity.txt')
        $log = Join-Path $EvidenceDirectory 'native.log'
        & pnpm desktop:test:e2e:windows *> $log
        $exitCode = $LASTEXITCODE
    } catch {
        $_.Exception.Message | Set-Content -LiteralPath (Join-Path $EvidenceDirectory 'error.txt')
    } finally {
        $pendingResult = Join-Path $EvidenceDirectory 'exit-code.pending'
        $exitCode | Set-Content -LiteralPath $pendingResult
        Move-Item -LiteralPath $pendingResult -Destination (Join-Path $EvidenceDirectory 'exit-code.txt')
    }
    exit $exitCode
}

New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$runDirectory = Join-Path $evidenceRoot ([guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $runDirectory | Out-Null
$entry = $PSCommandPath.Replace("'", "''")
$ownedDirectory = $runDirectory.Replace("'", "''")
$childCommand = "& '$entry' -Restricted -EvidenceDirectory '$ownedDirectory'"
$encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($childCommand))
$powershell = (Get-Process -Id $PID).Path
$commandLine = '"' + $powershell + '" -NoProfile -NonInteractive -WindowStyle Hidden -EncodedCommand ' + $encoded

# NORMALUSER removes admin rights for this child; it does not modify UAC, policies,
# users, registry keys, or the product. Do not use /savecred or administrative runas.
& "$env:SystemRoot\System32\runas.exe" /trustlevel:0x20000 $commandLine
if ($LASTEXITCODE -ne 0) { throw 'Could not start the standard-user native test.' }
$result = Join-Path $runDirectory 'exit-code.txt'
$deadline = [DateTime]::UtcNow.AddMinutes(10)
while (-not (Test-Path -LiteralPath $result)) {
    if ([DateTime]::UtcNow -ge $deadline) { throw 'The standard-user native test did not report completion.' }
    Start-Sleep -Milliseconds 500
}
foreach ($name in @('identity.txt', 'native.log', 'error.txt')) {
    $path = Join-Path $runDirectory $name
    if (Test-Path -LiteralPath $path) { Get-Content -LiteralPath $path }
}
exit [int](Get-Content -LiteralPath $result -Raw)
