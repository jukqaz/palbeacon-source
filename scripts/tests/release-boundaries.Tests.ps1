#Requires -Version 5.1

BeforeAll {
    $script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
}

Describe 'Release mutation boundaries' {
    It 'runs native CI at verified standard integrity without changing system policy' {
        $path = Join-Path $RepoRoot 'scripts\invoke-palbeacon-ci-native.ps1'
        $tokens = $null
        $parseErrors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $path, [ref] $tokens, [ref] $parseErrors
        )
        @($parseErrors).Count | Should -Be 0
        $ast.Extent.Text | Should -Match '/trustlevel:0x20000'
        $ast.Extent.Text | Should -Match 'S-1-16-8192'
        $ast.Extent.Text | Should -Match 'WindowsBuiltInRole\]::Administrator'
        $ast.Extent.Text | Should -Match 'Value -gt 8192'
        $ast.Extent.Text | Should -Match '-WindowStyle Hidden'
        $ast.Extent.Text | Should -Not -Match 'Set-ExecutionPolicy|Set-ItemProperty|New-LocalUser|AdjustTokenPrivileges'
        $source = $ast.Find({
                param($node)
                $node -is [System.Management.Automation.Language.StringConstantExpressionAst] -and
                $node.Value.Contains('public static class PalBeaconRestrictedIntegrity')
            }, $true)
        { Add-Type -TypeDefinition $source.Value } | Should -Not -Throw
    }

    It 'rejects the native privilege launcher outside an ephemeral CI runner' {
        $previous = $env:GITHUB_ACTIONS
        try {
            $env:GITHUB_ACTIONS = 'false'
            { & (Join-Path $RepoRoot 'scripts\invoke-palbeacon-ci-native.ps1') } |
                Should -Throw '*restricted to ephemeral GitHub Windows runners*'
        } finally {
            $env:GITHUB_ACTIONS = $previous
        }
    }

    It 'keeps private resource builds and deployment out of public source CI' {
        $workflow = Get-Content -LiteralPath (
            Join-Path $RepoRoot '.github\workflows\source-snapshot.yml'
        ) -Raw
        $workflow | Should -Match 'publication-and-dependencies:'
        $workflow | Should -Match 'run: node verify-repository.mjs'
        $workflow | Should -Match 'run: pnpm quality:security'
        $workflow | Should -Match 'contents: read'
        $workflow | Should -Match 'persist-credentials: false'
        $workflow | Should -Not -Match 'continue-on-error: true|--no-sandbox'
        $workflow | Should -Not -Match 'cf:deploy|desktop:build|upload-artifact|secrets\.'
    }

    It 'keeps bundle installation limited to CI without deleting existing directories' {
        $scriptPath = Join-Path $RepoRoot 'scripts\install-palbeacon-ci-bundle.ps1'
        $tokens = $null
        $parseErrors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $scriptPath, [ref] $tokens, [ref] $parseErrors
        )
        @($parseErrors).Count | Should -Be 0
        $ast.Extent.Text | Should -Match 'GITHUB_ACTIONS'
        $ast.Extent.Text | Should -Match 'refusing to overwrite'
        $ast.Extent.Text | Should -Match '-WindowStyle Hidden'
        $ast.Extent.Text | Should -Not -Match 'Remove-Item|--no-sandbox|Set-ExecutionPolicy'
    }

    It 'requires an installed WebView2 runtime rather than an Edge browser' {
        $tokens = $null
        $parseErrors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            (Join-Path $RepoRoot 'scripts\install-palbeacon-ci-bundle.ps1'),
            [ref] $tokens, [ref] $parseErrors
        )
        $function = $ast.Find({
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq 'Get-WebView2RuntimeVersion'
            }, $true)
        . ([scriptblock]::Create($function.Extent.Text))
        Mock Get-ItemProperty { $null }
        Get-WebView2RuntimeVersion | Should -BeNullOrEmpty
        Mock Get-ItemProperty { [pscustomobject]@{ pv = '0.0.0.0' } }
        Get-WebView2RuntimeVersion | Should -BeNullOrEmpty
        Mock Get-ItemProperty { [pscustomobject]@{ pv = '152.0.4191.66' } }
        Get-WebView2RuntimeVersion | Should -Be '152.0.4191.66'
    }

    It 'requires explicit bundle validation and every packaged native runtime resource' {
        $gate = Get-Content -LiteralPath (
            Join-Path $RepoRoot 'scripts\verify-palbeacon-cutover.ps1'
        ) -Raw
        $gate | Should -Match '\[switch\] \$RequireWindowsBundle'
        $gate | Should -Match 'if \(\$RequireWindowsBundle -or \$RequireSignedWindowsBundle\)'
        $gate | Should -Match 'The Tauri NSIS bundle is missing'
        $gate | Should -Match 'The self-contained Windows runtime is incomplete'
        $gate | Should -Match 'pal-core\.exe'
        $gate | Should -Match 'pal-overlay\.exe'
        $gate | Should -Match 'pal-fullscreen-overlay-dx11\.dll'
        $gate | Should -Match 'pal-fullscreen-overlay-dx12\.dll'
    }

    It 'rejects leftover files before packaging the approved map snapshot' {
        $scriptPath = Join-Path $RepoRoot 'scripts\prepare-palbeacon-desktop.ps1'
        $tokens = $null
        $parseErrors = $null
        $ast = [System.Management.Automation.Language.Parser]::ParseFile(
            $scriptPath, [ref] $tokens, [ref] $parseErrors
        )
        $function = $ast.Find({
                param($node)
                $node -is [System.Management.Automation.Language.FunctionDefinitionAst] -and
                $node.Name -eq 'Assert-NoExtraFiles'
            }, $true)
        . ([scriptblock]::Create($function.Extent.Text))
        $source = New-Item -ItemType Directory -Path (Join-Path $TestDrive 'source')
        $destination = New-Item -ItemType Directory -Path (Join-Path $TestDrive 'destination')
        { Assert-NoExtraFiles -Source $source.FullName -Destination $destination.FullName } |
            Should -Not -Throw
        New-Item -ItemType File -Path (Join-Path $destination 'old-report.json') | Out-Null
        { Assert-NoExtraFiles -Source $source.FullName -Destination $destination.FullName } |
            Should -Throw '*outside the approved source*'
    }

    It 'keeps Wrangler local secrets ignored while allowing examples' {
        foreach ($path in @(
                'cloudflare/.dev.vars',
                'cloudflare/.dev.vars.local',
                'cloudflare/.env',
                'cloudflare/.env.production',
                'cloudflare/operator.secrets.json'
            )) {
            & git -C $RepoRoot check-ignore --no-index --quiet $path
            $LASTEXITCODE | Should -Be 0 -Because "$path must never be committed"
        }
        foreach ($path in @(
                'cloudflare/.dev.vars.example',
                'cloudflare/.env.example',
                'cloudflare/operator.secrets.example'
            )) {
            & git -C $RepoRoot check-ignore --no-index --quiet $path
            $LASTEXITCODE | Should -Be 1 -Because "$path is a sanitized template"
        }
    }

    It 'keeps removed Flutter surfaces in Git history only' {
        $cutover = Get-Content -LiteralPath (
            Join-Path $RepoRoot 'contracts\palbeacon\cutover.v1.json'
        ) -Raw | ConvertFrom-Json
        [string] $cutover.history_reference.checkpoint_commit |
            Should -Match '^[a-f0-9]{40}$'
        foreach ($path in @($cutover.history_reference.removed_executable_surfaces)) {
            Test-Path -LiteralPath (Join-Path $RepoRoot $path) | Should -BeFalse
        }
    }

    It 'owns game data outside the UI implementation' {
        foreach ($relative in @(
                'assets\palbeacon\game\catalog\world.v1.json',
                'assets\palbeacon\game\map\pois.v1.json',
                'assets\palbeacon\fonts\PretendardVariable.ttf',
                'assets\palbeacon\brand\generated\v3\app-mark-square-v3.png'
            )) {
            Test-Path -LiteralPath (Join-Path $RepoRoot $relative) -PathType Leaf |
                Should -BeTrue -Because $relative
        }
    }

    It 'contains no Flutter map compatibility parser' {
        foreach ($script in @(
                'scripts\new-cloudflare-public-catalog.ps1',
                'scripts\new-cloudflare-public-map-data.ps1',
                'scripts\run-windows-overlay-automation.ps1'
            )) {
            $text = Get-Content -LiteralPath (Join-Path $RepoRoot $script) -Raw
            $text | Should -Not -Match '(?i)flutter|\.dart'
        }
    }

    It 'projects the verified public catalog before every Worker runtime or publish' {
        $package = Get-Content -LiteralPath (
            Join-Path $RepoRoot 'cloudflare\package.json'
        ) -Raw | ConvertFrom-Json
        [string] $package.scripts.'catalog:build' |
            Should -Match 'new-cloudflare-public-catalog\.ps1'
        foreach ($scriptName in @('cf:dev', 'cf:check', 'cf:deploy')) {
            [string] $package.scripts.$scriptName |
                Should -Match 'pnpm catalog:build && wrangler'
        }
    }

    It 'packages tracked approved map data without local extraction reports' {
        $prepare = Get-Content -LiteralPath (
            Join-Path $RepoRoot 'scripts\prepare-palbeacon-desktop.ps1'
        ) -Raw
        $prepare | Should -Match 'assets\\palbeacon\\game\\native-map-pack'
        $prepare | Should -Not -Match 'artifacts\\live-overlay-smoke'
        $contract = Get-Content -LiteralPath (
            Join-Path $RepoRoot 'contracts\runtime-builds.json'
        ) -Raw | ConvertFrom-Json
        $build = [string] $contract.current_game_build_id
        $source = Join-Path $RepoRoot "assets\palbeacon\game\native-map-pack\$build"
        Test-Path -LiteralPath (Join-Path $source "$build.active.json") |
            Should -BeTrue
        $privateDirectories = @(
            Get-ChildItem -LiteralPath $source -Recurse -Force -Directory |
                Where-Object { $_.Name -in @('reports', '.pipeline') }
        )
        $privateDirectories.Count | Should -Be 0
    }

    It 'bundles one self-contained PalBeacon Windows runtime set' {
        $config = Get-Content -LiteralPath (
            Join-Path $RepoRoot 'apps\palbeacon-desktop\src-tauri\tauri.conf.json'
        ) -Raw | ConvertFrom-Json
        @($config.bundle.externalBin) | Should -Be @(
            'binaries/pal-save-parser-worker',
            'binaries/pal-core',
            'binaries/pal-overlay',
            'binaries/pal-fullscreen-injector'
        )
        $resources = $config.bundle.resources
        foreach ($resource in @(
                '../../../target/palbeacon-runtime-resources/map-pack/',
                'binaries/pal-fullscreen-rhi-probe.dll',
                'binaries/pal-fullscreen-overlay-dx11.dll',
                'binaries/pal-fullscreen-overlay-dx12.dll'
            )) {
            $resources.PSObject.Properties.Name | Should -Contain $resource
        }
    }
}
