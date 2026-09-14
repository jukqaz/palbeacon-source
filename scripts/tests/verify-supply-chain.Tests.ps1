#Requires -Version 5.1

BeforeAll {
    $script:Workspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Verifier = Join-Path $Workspace 'scripts\verify-supply-chain.ps1'
    $script:Generator = Join-Path $Workspace 'scripts\generate-sbom.ps1'

    function Invoke-SupplyChainVerifier {
        param(
            [string] $CargoLock = (Join-Path $Workspace 'Cargo.lock'),
            [switch] $SkipExternalTools
        )

        $arguments = @(
            '-NoProfile',
            '-ExecutionPolicy', 'Bypass',
            '-File', $Verifier,
            '-Workspace', $Workspace,
            '-CargoLock', $CargoLock
        )
        if ($SkipExternalTools) {
            $arguments += '-SkipExternalTools'
        }
        $output = @(& powershell.exe @arguments 2>&1)
        [pscustomobject] @{
            ExitCode = $LASTEXITCODE
            Text = ($output | Out-String).Trim()
        }
    }
}

Describe 'supply-chain verification' {
    It 'rejects a Git or branch dependency' {
        $lock = Join-Path $TestDrive 'Cargo.lock'
        @'
version = 4

[[package]]
name = "bad"
version = "1.0.0"
source = "git+https://example.invalid/repo?branch=main"
'@ | Set-Content -LiteralPath $lock -Encoding utf8

        $run = Invoke-SupplyChainVerifier -CargoLock $lock -SkipExternalTools

        $run.ExitCode | Should -Be 1
        $run.Text | Should -Match 'Git dependencies are forbidden'
    }

    It 'writes no component without a version and package URL' {
        $run = Invoke-SupplyChainVerifier -SkipExternalTools
        $run.ExitCode | Should -Be 0 -Because $run.Text

        $sbom = Get-Content (
            Join-Path $Workspace 'artifacts\sbom\palbeacon.cdx.json'
        ) -Raw | ConvertFrom-Json
        @($sbom.components).Count | Should -BeGreaterThan 0
        @($sbom.components | Where-Object {
            [string]::IsNullOrWhiteSpace([string] $_.version) -or
            [string]::IsNullOrWhiteSpace([string] $_.purl)
        }).Count | Should -Be 0
        @($sbom.components | Where-Object {
            $_.purl -eq 'pkg:cargo/hudhook@0.9.2'
        }).Count | Should -Be 1
    }

    It 'has an exact SPDX review record for every direct runtime component' {
        $run = Invoke-SupplyChainVerifier -SkipExternalTools
        $run.ExitCode | Should -Be 0 -Because $run.Text

        $inventory = Get-Content (
            Join-Path $Workspace 'third_party\runtime-dependencies.json'
        ) -Raw | ConvertFrom-Json
        @($inventory.components).Count | Should -BeGreaterThan 0
        @($inventory.components | Where-Object {
            [string]::IsNullOrWhiteSpace([string] $_.version) -or
            [string]::IsNullOrWhiteSpace([string] $_.license) -or
            [string]::IsNullOrWhiteSpace([string] $_.purl)
        }).Count | Should -Be 0
    }

    It 'generates byte-identical SBOM output from unchanged locks' {
        $run = Invoke-SupplyChainVerifier -SkipExternalTools
        $run.ExitCode | Should -Be 0 -Because $run.Text
        $sbom = Join-Path $Workspace 'artifacts\sbom\palbeacon.cdx.json'
        $first = (Get-FileHash -LiteralPath $sbom -Algorithm SHA256).Hash

        & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Generator `
            -Workspace $Workspace
        $LASTEXITCODE | Should -Be 0
        $second = (Get-FileHash -LiteralPath $sbom -Algorithm SHA256).Hash

        $second | Should -Be $first
    }
}
