[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $MapPath,

    [ValidateRange(1, 30)]
    [int] $TimeoutSeconds = 15
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0

$ExactBuildId = '24181527'
$ExactMapSha256 =
    'aa81bdddbc525898b0a70b238cc936d0f8b0416c4ead922797b066d871938d8b'
$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$calibratorPath = Join-Path $workspace (
    'tools\pal-map-pack\src\PalMapPack.Calibrator\bin\Release\' +
    'net10.0-windows\PalMapPack.Calibrator.exe')
$rustPath = Join-Path $workspace (
    'artifacts\pal-overlay-build-24181527-dev\pal-overlay.exe')

function ConvertTo-WindowsCommandLineArgument {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string] $Argument)

    if ($Argument.Length -gt 0 -and $Argument -notmatch '[\s"]') {
        return $Argument
    }

    $builder = New-Object Text.StringBuilder
    [void]$builder.Append('"')
    $backslashes = 0
    foreach ($character in $Argument.ToCharArray()) {
        if ($character -eq '\') {
            $backslashes += 1
            continue
        }
        if ($character -eq '"') {
            [void]$builder.Append(('\' * ($backslashes * 2 + 1)))
            [void]$builder.Append('"')
            $backslashes = 0
            continue
        }
        if ($backslashes -gt 0) {
            [void]$builder.Append(('\' * $backslashes))
            $backslashes = 0
        }
        [void]$builder.Append($character)
    }
    if ($backslashes -gt 0) {
        [void]$builder.Append(('\' * ($backslashes * 2)))
    }
    [void]$builder.Append('"')
    return $builder.ToString()
}

function Invoke-ExactChild {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Executable,
        [Parameter(Mandatory = $true)]
        [string[]] $Arguments
    )

    $startInfo = New-Object Diagnostics.ProcessStartInfo
    $startInfo.FileName = $Executable
    $startInfo.Arguments = (($Arguments | ForEach-Object {
        ConvertTo-WindowsCommandLineArgument $_
    }) -join ' ')
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    $process = New-Object Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw 'child process did not start'
        }
        $stdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $process.WaitForExit()
        $stdout = $stdoutTask.GetAwaiter().GetResult()
        $stderr = $stderrTask.GetAwaiter().GetResult()
        return [pscustomobject]@{
            ExitCode = $process.ExitCode
            Stdout = $stdout
            Stderr = $stderr
        }
    }
    finally {
        $process.Dispose()
    }
}

function New-OwnerOnlyTemporaryDirectory {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $user = $identity.User
        if ($null -eq $user) {
            throw 'current token has no user SID'
        }

        $security = New-Object Security.AccessControl.DirectorySecurity
        $security.SetOwner($user)
        $security.SetAccessRuleProtection($true, $false)
        $rule = New-Object Security.AccessControl.FileSystemAccessRule(
            $user,
            [Security.AccessControl.FileSystemRights]::FullControl,
            (
                [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor
                [Security.AccessControl.InheritanceFlags]::ObjectInherit
            ),
            [Security.AccessControl.PropagationFlags]::None,
            [Security.AccessControl.AccessControlType]::Allow)
        [void]$security.AddAccessRule($rule)

        $path = Join-Path ([IO.Path]::GetTempPath()) (
            'pal-local-alignment-' + [Guid]::NewGuid().ToString('N'))
        $directory = New-Object IO.DirectoryInfo($path)
        $directory.Create($security)
        Assert-OwnerOnlyPath $path $true
        return $directory.FullName
    }
    finally {
        $identity.Dispose()
    }
}

function Assert-OwnerOnlyPath {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Path,
        [Parameter(Mandatory = $true)]
        [bool] $Directory
    )

    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    try {
        $user = $identity.User
        if ($null -eq $user) {
            throw 'current token has no user SID'
        }
        if ($Directory) {
            $security = (New-Object IO.DirectoryInfo($Path)).GetAccessControl(
                [Security.AccessControl.AccessControlSections]::Owner -bor
                [Security.AccessControl.AccessControlSections]::Access)
        }
        else {
            $security = (New-Object IO.FileInfo($Path)).GetAccessControl(
                [Security.AccessControl.AccessControlSections]::Owner -bor
                [Security.AccessControl.AccessControlSections]::Access)
        }
        $rules = @($security.GetAccessRules(
            $true,
            $true,
            [Security.Principal.SecurityIdentifier]))
        if (-not $security.AreAccessRulesProtected -or
            -not $user.Equals($security.GetOwner(
                [Security.Principal.SecurityIdentifier])) -or
            $rules.Count -ne 1) {
            throw 'owner-only security descriptor is required'
        }
        $rule = $rules[0]
        if (-not $user.Equals($rule.IdentityReference) -or
            $rule.AccessControlType -ne
                [Security.AccessControl.AccessControlType]::Allow -or
            $rule.FileSystemRights -ne
                [Security.AccessControl.FileSystemRights]::FullControl -or
            $rule.IsInherited) {
            throw 'owner-only security descriptor is required'
        }
    }
    finally {
        $identity.Dispose()
    }
}

function Assert-RegularObservationFile {
    param([Parameter(Mandatory = $true)][string] $Path)

    $item = Get-Item -LiteralPath $Path -Force
    if ($item.PSIsContainer -or
        ($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw 'capture output is not a regular file'
    }
    Assert-OwnerOnlyPath $Path $false
}

function ConvertTo-IdentityFreeSuccessLine {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string] $Output)

    $line = $Output
    if ($line.EndsWith("`r`n", [StringComparison]::Ordinal)) {
        $line = $line.Substring(0, $line.Length - 2)
    }
    elseif ($line.EndsWith("`n", [StringComparison]::Ordinal)) {
        $line = $line.Substring(0, $line.Length - 1)
    }
    if ([string]::IsNullOrEmpty($line) -or
        $line.Contains("`r") -or
        $line.Contains("`n")) {
        throw 'diagnostic output must be exactly one JSON line'
    }
    $record = ConvertFrom-Json -InputObject $line
    $expectedProperties = @(
        'schema',
        'claim',
        'gate_b_approved',
        'build_id',
        'map_sha256',
        'sample_count',
        'sample_window_ms',
        'projected_spread_px',
        'residual_px',
        'within_development_smoke_threshold')
    $actualProperties = @($record.PSObject.Properties.Name)
    if ($actualProperties.Count -ne $expectedProperties.Count) {
        throw 'diagnostic output has an unexpected property set'
    }
    foreach ($property in $expectedProperties) {
        if (-not ($actualProperties -ccontains $property)) {
            throw 'diagnostic output has an unexpected property set'
        }
    }
    if ($record.schema -cne 'pal_companion.local_alignment_smoke.v1' -or
        $record.claim -cne 'development_smoke_only_not_gate_b' -or
        $record.gate_b_approved -ne $false -or
        [uint64]$record.build_id -ne 24181527 -or
        $record.map_sha256 -cne $ExactMapSha256 -or
        [int]$record.sample_count -ne 10 -or
        [uint64]$record.sample_window_ms -lt 900 -or
        [double]$record.projected_spread_px -gt 1.0 -or
        [double]$record.residual_px -gt 8.0 -or
        $record.within_development_smoke_threshold -ne $true) {
        throw 'diagnostic output did not satisfy the development smoke contract'
    }
    return $line
}

function Remove-PrivateObservationDirectory {
    param(
        [string] $ObservationPath,
        [string] $DirectoryPath
    )

    if (-not [string]::IsNullOrEmpty($ObservationPath) -and
        (Test-Path -LiteralPath $ObservationPath)) {
        Remove-Item -LiteralPath $ObservationPath -Force
    }
    if (-not [string]::IsNullOrEmpty($DirectoryPath) -and
        (Test-Path -LiteralPath $DirectoryPath)) {
        if (@(Get-ChildItem -LiteralPath $DirectoryPath -Force).Count -ne 0) {
            throw 'private temporary directory was not empty'
        }
        Remove-Item -LiteralPath $DirectoryPath -Force
    }
}

$privateDirectory = $null
$observationPath = $null
$pendingStdout = $null
$exitCode = 1
$failed = $false
$failureKind = 'local alignment smoke failed'

try {
    if (-not (Test-Path -LiteralPath $calibratorPath -PathType Leaf) -or
        -not (Test-Path -LiteralPath $rustPath -PathType Leaf)) {
        throw 'required release executable is unavailable'
    }
    $resolvedMap = (Resolve-Path -LiteralPath $MapPath).Path
    if (-not (Test-Path -LiteralPath $resolvedMap -PathType Leaf) -or
        [IO.Path]::GetExtension($resolvedMap) -ine '.bmp') {
        throw 'exact map BMP is unavailable'
    }

    $privateDirectory = New-OwnerOnlyTemporaryDirectory
    $observationPath = Join-Path $privateDirectory 'native-marker-observation.json'
    if (Test-Path -LiteralPath $observationPath) {
        throw 'capture output already exists'
    }

    $capture = Invoke-ExactChild $calibratorPath @(
        '--local-alignment-capture',
        '--build',
        $ExactBuildId,
        '--map',
        $resolvedMap,
        '--output',
        $observationPath)
    if ($capture.ExitCode -ne 0) {
        $exitCode = $capture.ExitCode
        $failureKind = 'native marker capture failed'
        throw 'capture failed'
    }
    if (-not (Test-Path -LiteralPath $observationPath -PathType Leaf)) {
        $failureKind = 'native marker capture failed'
        throw 'capture produced no observation'
    }
    Assert-RegularObservationFile $observationPath

    $observationSha = (
        Get-FileHash -LiteralPath $observationPath -Algorithm SHA256
    ).Hash.ToLowerInvariant()
    $diagnostic = Invoke-ExactChild $rustPath @(
        '--development-local-alignment-diagnostic-json',
        '--real-map-bmp',
        $resolvedMap,
        '--observation-file',
        $observationPath,
        '--observation-sha256',
        $observationSha,
        '--timeout-seconds',
        $TimeoutSeconds.ToString(
            [Globalization.CultureInfo]::InvariantCulture))
    if ($diagnostic.ExitCode -ne 0) {
        $exitCode = $diagnostic.ExitCode
        $failureKind = 'local alignment diagnostic failed'
        throw 'diagnostic failed'
    }
    $pendingStdout = ConvertTo-IdentityFreeSuccessLine $diagnostic.Stdout
    $exitCode = 0
}
catch {
    $failed = $true
    if ($exitCode -eq 0) {
        $exitCode = 1
    }
}
finally {
    try {
        Remove-PrivateObservationDirectory $observationPath $privateDirectory
    }
    catch {
        $failed = $true
        $pendingStdout = $null
        if ($exitCode -eq 0) {
            $exitCode = 1
        }
        $failureKind = 'local alignment smoke cleanup failed'
    }
}

if ($failed -or $exitCode -ne 0 -or $null -eq $pendingStdout) {
    [Console]::Error.WriteLine($failureKind)
    exit $exitCode
}

[Console]::Out.WriteLine($pendingStdout)
exit 0
