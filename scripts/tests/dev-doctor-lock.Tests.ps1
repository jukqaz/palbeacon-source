#Requires -Version 5.1

BeforeAll {
    $script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Doctor = Join-Path $RepoRoot 'scripts\dev-doctor.ps1'
    $script:LockFile = Join-Path $RepoRoot 'toolchains.lock.json'
}

Describe 'dev-doctor canonical toolchains' {
    It 'reports the Rust, Node, pnpm, .NET and Git toolchains only' {
        $result = @(& $Doctor -LockFile $LockFile -Json 2>&1) | Out-String |
            ConvertFrom-Json
        @($result.name) | Should -Contain 'Rust'
        @($result.name) | Should -Contain 'Node'
        @($result.name) | Should -Contain 'pnpm'
        @($result.name) | Should -Contain '.NET SDK'
        @($result.name) | Should -Contain 'Git'
        @($result.name) | Should -Not -Contain 'Flutter'
        @($result.name) | Should -Not -Contain 'Dart'
    }

    It 'fails with a typed result when the lock is absent' {
        $missing = Join-Path $TestDrive 'missing.json'
        $output = @(& $Doctor -LockFile $missing -Json 2>&1)
        $LASTEXITCODE | Should -Be 1
        $result = ($output | Out-String) | ConvertFrom-Json
        $result.name | Should -Be 'Toolchain lock'
        $result.found | Should -BeFalse
        $result.locked | Should -BeFalse
    }

    It 'pins the package manager versions used by the workspace' {
        $lock = Get-Content -LiteralPath $LockFile -Raw | ConvertFrom-Json
        $package = Get-Content -LiteralPath (Join-Path $RepoRoot 'package.json') -Raw |
            ConvertFrom-Json
        [string] $package.engines.node | Should -Be '24.19.x'
        [string] $package.engines.pnpm | Should -Be '11.19.x'
        [string] $package.packageManager | Should -Be "pnpm@$($lock.node.pnpm)"
    }
}
