#Requires -Version 5.1

[CmdletBinding()]
param(
    [ValidateSet('quick', 'graphics', 'live')]
    [string] $AutomationProfile = 'quick',

    [string] $ArtifactsDirectory,

    [switch] $AllowExclusiveFullscreen
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$workspace = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
if ([string]::IsNullOrWhiteSpace($ArtifactsDirectory)) {
    $runId = Get-Date -Format 'yyyyMMdd-HHmmss'
    $ArtifactsDirectory = Join-Path $workspace (
        "artifacts\windows-overlay-automation\$runId")
}
$artifacts = [IO.Path]::GetFullPath($ArtifactsDirectory)
if ($artifacts -eq [IO.Path]::GetPathRoot($artifacts)) {
    throw 'ArtifactsDirectory cannot be a drive root.'
}
[void] (New-Item -ItemType Directory -Path $artifacts -Force)

$cargo = Join-Path $workspace '.tools\cargo\bin\cargo.exe'
if (-not (Test-Path -LiteralPath $cargo -PathType Leaf)) {
    throw 'Workspace-managed Cargo is unavailable.'
}
$results = [Collections.Generic.List[object]]::new()
$startedAt = [DateTimeOffset]::Now
$runError = $null

function Initialize-WindowCapture {
    Add-Type -AssemblyName System.Drawing
    if ('PalTestNativeMethods' -as [type]) {
        return
    }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

public static class PalTestNativeMethods
{
    [StructLayout(LayoutKind.Sequential)]
    public struct Rect
    {
        public int Left;
        public int Top;
        public int Right;
        public int Bottom;
    }

    [DllImport("user32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    public static extern bool GetWindowRect(IntPtr hwnd, out Rect rect);
}
'@
}

function Save-WindowScreenshot {
    param(
        [Parameter(Mandatory)] [uint64] $WindowHandle,
        [Parameter(Mandatory)] [string] $OutputPath
    )

    Initialize-WindowCapture
    $rect = New-Object PalTestNativeMethods+Rect
    if (-not [PalTestNativeMethods]::GetWindowRect(
            [IntPtr]::new([int64] $WindowHandle),
            [ref] $rect)) {
        throw "GetWindowRect failed with Win32 error $([Runtime.InteropServices.Marshal]::GetLastWin32Error())."
    }
    $width = $rect.Right - $rect.Left
    $height = $rect.Bottom - $rect.Top
    if ($width -le 0 -or $height -le 0 -or $width -gt 7680 -or $height -gt 4320) {
        throw "Window capture bounds are invalid: ${width}x${height}."
    }

    $bitmap = New-Object Drawing.Bitmap($width, $height)
    $graphics = [Drawing.Graphics]::FromImage($bitmap)
    try {
        $graphics.CopyFromScreen(
            $rect.Left,
            $rect.Top,
            0,
            0,
            [Drawing.Size]::new($width, $height),
            [Drawing.CopyPixelOperation]::SourceCopy)
        $visiblePixelFound = $false
        $sampleStepX = [Math]::Max(1, [int] ($width / 16))
        $sampleStepY = [Math]::Max(1, [int] ($height / 9))
        for ($y = 0; $y -lt $height -and -not $visiblePixelFound; $y += $sampleStepY) {
            for ($x = 0; $x -lt $width; $x += $sampleStepX) {
                $pixel = $bitmap.GetPixel($x, $y)
                if (($pixel.R + $pixel.G + $pixel.B) -gt 12) {
                    $visiblePixelFound = $true
                    break
                }
            }
        }
        if (-not $visiblePixelFound) {
            throw 'Window capture contains no visible pixels; DirectX surface capture is unavailable.'
        }
        $bitmap.Save($OutputPath, [Drawing.Imaging.ImageFormat]::Png)
    }
    finally {
        $graphics.Dispose()
        $bitmap.Dispose()
    }
}

function Invoke-LoggedStep {
    param(
        [Parameter(Mandatory)] [string] $Name,
        [Parameter(Mandatory)] [string] $Executable,
        [Parameter(Mandatory)] [string[]] $Arguments,
        [Parameter(Mandatory)] [string] $WorkingDirectory
    )

    $logPath = Join-Path $artifacts "$Name.log"
    Write-Host "==> $Name"
    Push-Location -LiteralPath $WorkingDirectory
    try {
        $previousPreference = $ErrorActionPreference
        $ErrorActionPreference = 'Continue'
        & $Executable @Arguments 2>&1 |
            ForEach-Object {
                if ($_ -is [Management.Automation.ErrorRecord]) {
                    $_.Exception.Message
                }
                else {
                    $_
                }
            } |
            Tee-Object -FilePath $logPath |
            Out-Host
        $exitCode = $LASTEXITCODE
        $ErrorActionPreference = $previousPreference
    }
    finally {
        $ErrorActionPreference = 'Stop'
        Pop-Location
    }
    $result = [pscustomobject] [ordered] @{
        name = $Name
        ok = ($exitCode -eq 0)
        exit_code = $exitCode
        log = $logPath
    }
    $results.Add($result)
    if (-not $result.ok) {
        throw "Automation step '$Name' failed with exit code $exitCode."
    }
}

function Invoke-TestRhiHost {
    param(
        [Parameter(Mandatory)] [ValidateSet('dx11', 'dx12')]
        [string] $Api,
        [Parameter(Mandatory)] [ValidateSet('windowed', 'borderless', 'exclusive')]
        [string] $WindowMode,
        [Parameter(Mandatory)] [string] $HostPath
    )

    if ($WindowMode -eq 'exclusive' -and -not $AllowExclusiveFullscreen) {
        throw 'Exclusive fullscreen requires -AllowExclusiveFullscreen.'
    }
    $name = "rhi-$Api-$WindowMode"
    $logPath = Join-Path $artifacts "$name.log"
    $screenshotPath = Join-Path $artifacts "$name.png"
    $arguments = @(
        '--api', $Api,
        '--window-mode', $WindowMode,
        '--width', '1280',
        '--height', '720',
        '--duration-seconds', '3'
    )
    if ($WindowMode -eq 'exclusive') {
        $arguments += '--allow-exclusive-fullscreen'
    }

    $startInfo = New-Object Diagnostics.ProcessStartInfo
    $startInfo.FileName = $HostPath
    $startInfo.Arguments = $arguments -join ' '
    $startInfo.WorkingDirectory = $workspace
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $false
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true

    Write-Host "==> $name"
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "Test RHI host '$name' did not start."
        }
        $readyLine = $process.StandardOutput.ReadLine()
        $capabilitiesLine = $process.StandardOutput.ReadLine()
        $remainingStdoutTask = $process.StandardOutput.ReadToEndAsync()
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $captureError = $null
        if ($readyLine -match '^READY \d+ (\d+)\s*$') {
            Start-Sleep -Milliseconds 250
            try {
                Save-WindowScreenshot -WindowHandle ([uint64] $Matches[1]) `
                    -OutputPath $screenshotPath
            }
            catch {
                $captureError = $_.Exception.Message
            }
        }
        else {
            $captureError = 'The RHI host did not emit a valid READY line.'
        }
        if (-not $process.WaitForExit(15000)) {
            $process.Kill()
            throw "Test RHI host '$name' exceeded 15 seconds."
        }
        $stdout = @(
            $readyLine,
            $capabilitiesLine,
            $remainingStdoutTask.GetAwaiter().GetResult()
        ) -join [Environment]::NewLine
        $stderr = $stderrTask.GetAwaiter().GetResult()
        @($stdout, $stderr, "capture_error=$captureError") -join [Environment]::NewLine |
            Set-Content -LiteralPath $logPath -Encoding utf8
        $expected = (
            'CAPABILITIES .*"api":"' + [regex]::Escape($Api) +
            '","window_mode":"' + [regex]::Escape($WindowMode) + '"')
        $ok = ($process.ExitCode -eq 0 -and
            $stdout -match '(?m)^READY \d+ \d+\s*$' -and
            $stdout -match $expected)
        $results.Add([pscustomobject] [ordered] @{
            name = $name
            ok = $ok
            exit_code = $process.ExitCode
            log = $logPath
            screenshot = if (Test-Path -LiteralPath $screenshotPath -PathType Leaf) {
                $screenshotPath
            }
            else {
                $null
            }
            capture_error = $captureError
        })
        if (-not $ok) {
            throw "Test RHI host '$name' did not satisfy its capability contract."
        }
    }
    finally {
        $process.Dispose()
    }
}

function Test-LiveGameOverlay {
    $games = @(Get-Process -Name 'Palworld-Win64-Shipping' -ErrorAction SilentlyContinue)
    if ($games.Count -ne 1) {
        throw "Live profile requires exactly one Palworld process; found $($games.Count)."
    }

    $game = $games[0]
    try {
        $modules = @($game.Modules | ForEach-Object { $_.ModuleName.ToLowerInvariant() })
    }
    catch {
        throw "Cannot inspect Palworld modules: $($_.Exception.Message)"
    }
    $hasDx12 = ($modules -contains 'd3d12.dll' -or $modules -contains 'd3d12core.dll')
    $hasDx11 = $modules -contains 'd3d11.dll'
    $loadedPalOverlays = @(
        @('pal-fullscreen-overlay-dx11.dll', 'pal-fullscreen-overlay-dx12.dll') |
            Where-Object { $modules -contains $_ }
    )
    if ($loadedPalOverlays.Count -gt 1) {
        throw "Multiple PalBeacon fullscreen renderers are loaded: $($loadedPalOverlays -join ', ')."
    }

    $api = 'unknown'
    $selectionEvidence = 'none'
    if ($loadedPalOverlays.Count -eq 1) {
        $api = if ($loadedPalOverlays[0] -eq 'pal-fullscreen-overlay-dx12.dll') {
            'dx12'
        }
        else {
            'dx11'
        }
        $selectionEvidence = 'loaded_overlay_module'
    }
    else {
        $injectorLog = Join-Path $env:LOCALAPPDATA (
            'PalCompanion\logs\fullscreen-injector.log')
        if (Test-Path -LiteralPath $injectorLog -PathType Leaf) {
            $pidPattern = [regex]::Escape([string] $game.Id)
            $probeLine = Get-Content -LiteralPath $injectorLog -Tail 400 |
                Where-Object {
                    $_ -match (
                        'attached DirectX(11|12) renderer to tracked ' +
                        "Palworld process $pidPattern(?:\s|$)")
                } |
                Select-Object -Last 1
            if ($probeLine -match 'attached DirectX(11|12) renderer') {
                $api = if ($Matches[1] -eq '12') { 'dx12' } else { 'dx11' }
                $selectionEvidence = 'injector_probe_log'
            }
        }
    }
    if ($api -eq 'unknown' -and ($hasDx11 -xor $hasDx12)) {
        $api = if ($hasDx12) { 'dx12' } else { 'dx11' }
        $selectionEvidence = 'single_loaded_runtime_module'
    }
    if ($api -eq 'unknown') {
        throw 'The active graphics API is ambiguous; no injected renderer or PID-specific probe evidence is available.'
    }

    $expectedOverlay = "pal-fullscreen-overlay-$api.dll"
    $competingOverlays = @(
        @('n_overlay.x64.dll', 'n_overlay.dll') |
            Where-Object { $modules -contains $_ }
    )
    $overlayLoaded = $modules -contains $expectedOverlay
    $runtimeProcesses = @(
        Get-Process -ErrorAction SilentlyContinue |
            Where-Object {
                $_.ProcessName -in @(
                    'PalCompanion',
                    'PalCompanionUI',
                    'pal-core',
                    'pal-overlay')
            } |
            Select-Object ProcessName, Id, Path
    )
    $observation = [pscustomobject] [ordered] @{
        schema = 'pal_companion.windows_live_overlay.v1'
        game_pid = $game.Id
        detected_api = $api
        selection_evidence = $selectionEvidence
        loaded_d3d11_runtime = $hasDx11
        loaded_d3d12_runtime = $hasDx12
        loaded_overlay_modules = @($loadedPalOverlays)
        expected_overlay_module = $expectedOverlay
        overlay_module_loaded = $overlayLoaded
        competing_overlay_modules = @($competingOverlays)
        runtime_processes = $runtimeProcesses
    }
    $logPath = Join-Path $artifacts 'live-game-overlay.json'
    $observation | ConvertTo-Json -Depth 5 |
        Set-Content -LiteralPath $logPath -Encoding utf8
    $runtimeNames = @($runtimeProcesses | ForEach-Object { $_.ProcessName })
    $runtimeOk = ($runtimeNames -contains 'pal-core' -and
        $runtimeNames -contains 'pal-overlay')
    $ok = ($overlayLoaded -and $competingOverlays.Count -eq 0 -and $runtimeOk)
    $results.Add([pscustomobject] [ordered] @{
        name = 'live-game-overlay'
        ok = $ok
        exit_code = if ($ok) { 0 } else { 1 }
        log = $logPath
    })
    if ($competingOverlays.Count -gt 0) {
        throw "A competing overlay is loaded: $($competingOverlays -join ', ')."
    }
    if (-not $overlayLoaded) {
        throw "Expected overlay module '$expectedOverlay' is not loaded in Palworld."
    }
    if (-not $runtimeOk) {
        throw 'The pal-core and pal-overlay runtime processes must both be running.'
    }
}

$previousRustupHome = $env:RUSTUP_HOME
$previousCargoHome = $env:CARGO_HOME
$previousPath = $env:PATH
try {
    $env:RUSTUP_HOME = Join-Path $workspace '.tools\rustup'
    $env:CARGO_HOME = Join-Path $workspace '.tools\cargo'
    $env:PATH = "$(Split-Path -Parent $cargo);$env:PATH"

    Invoke-LoggedStep -Name 'pal-test-window-unit' -Executable $cargo `
        -Arguments @('test', '-p', 'pal-test-window', '--locked', '--offline') `
        -WorkingDirectory $workspace
    Invoke-LoggedStep -Name 'overlay-pixel-regression' -Executable $cargo `
        -Arguments @(
            'test', '-p', 'pal-overlay-win', '--features', 'test-harness',
            '--test', 'visual_regression_contract', '--locked', '--offline') `
        -WorkingDirectory $workspace
    Invoke-LoggedStep -Name 'pal-test-window-clippy' -Executable $cargo `
        -Arguments @(
            'clippy', '-p', 'pal-test-window', '--all-targets', '--locked', '--offline',
            '--', '-D', 'warnings') `
        -WorkingDirectory $workspace

    if ($AutomationProfile -eq 'graphics') {
        Invoke-LoggedStep -Name 'pal-test-rhi-host-build' -Executable $cargo `
            -Arguments @(
                'build', '-p', 'pal-test-window', '--bin',
                'pal-test-rhi-host', '--locked', '--offline') `
            -WorkingDirectory $workspace
        $targetRoot = if ([string]::IsNullOrWhiteSpace($env:CARGO_TARGET_DIR)) {
            Join-Path $workspace 'target'
        }
        else {
            [IO.Path]::GetFullPath($env:CARGO_TARGET_DIR)
        }
        $hostPath = Join-Path $targetRoot (
            'x86_64-pc-windows-msvc\debug\pal-test-rhi-host.exe')
        if (-not (Test-Path -LiteralPath $hostPath -PathType Leaf)) {
            throw 'Built pal-test-rhi-host executable is unavailable.'
        }
        foreach ($case in @(
                @('dx11', 'windowed'),
                @('dx11', 'borderless'),
                @('dx12', 'windowed'),
                @('dx12', 'borderless')
            )) {
            Invoke-TestRhiHost -Api $case[0] -WindowMode $case[1] `
                -HostPath $hostPath
        }
        if ($AllowExclusiveFullscreen) {
            Invoke-TestRhiHost -Api 'dx11' -WindowMode 'exclusive' `
                -HostPath $hostPath
            Invoke-TestRhiHost -Api 'dx12' -WindowMode 'exclusive' `
                -HostPath $hostPath
        }
    }
    elseif ($AutomationProfile -eq 'live') {
        Test-LiveGameOverlay
    }
}
catch {
    $runError = $_.Exception.Message
    throw
}
finally {
    $env:RUSTUP_HOME = $previousRustupHome
    $env:CARGO_HOME = $previousCargoHome
    $env:PATH = $previousPath
    $report = [pscustomobject] [ordered] @{
        schema = 'pal_companion.windows_overlay_automation.v1'
        profile = $AutomationProfile
        started_at = $startedAt.ToString('o')
        completed_at = [DateTimeOffset]::Now.ToString('o')
        ok = ($null -eq $runError -and @($results | Where-Object { -not $_.ok }).Count -eq 0)
        error = $runError
        steps = @($results)
    }
    $report | ConvertTo-Json -Depth 6 |
        Set-Content -LiteralPath (Join-Path $artifacts 'report.json') -Encoding utf8
}
