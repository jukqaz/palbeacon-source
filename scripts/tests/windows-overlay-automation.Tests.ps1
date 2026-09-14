#Requires -Version 5.1

BeforeAll {
    $script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Runner = Get-Content -LiteralPath (
        Join-Path $RepoRoot 'scripts\run-windows-overlay-automation.ps1') -Raw
}

Describe 'Windows overlay automation boundary' {
    It 'provides bounded quick, graphics, and live profiles' {
        $Runner | Should -Match "ValidateSet\('quick', 'graphics', 'live'\)"
        $Runner | Should -Match '\[string\] \$AutomationProfile'
        $Runner | Should -Not -Match '\[string\] \$Profile'
        $Runner | Should -Match "'--duration-seconds', '3'"
        $Runner | Should -Match 'WaitForExit\(15000\)'
    }

    It 'tests DX11 and DX12 windowed and borderless presentation' {
        foreach ($case in @(
                "@\('dx11', 'windowed'\)",
                "@\('dx11', 'borderless'\)",
                "@\('dx12', 'windowed'\)",
                "@\('dx12', 'borderless'\)"
            )) {
            $Runner | Should -Match $case
        }
        $Runner | Should -Match 'pal-test-rhi-host\.exe'
        $Runner | Should -Match 'Save-WindowScreenshot'
        $Runner | Should -Match 'CopyFromScreen'
        $Runner | Should -Match 'Window capture contains no visible pixels'
        $Runner | Should -Not -Match 'Start-Process.+Palworld-Win64-Shipping'
    }

    It 'requires explicit opt-in for exclusive fullscreen' {
        $Runner | Should -Match 'AllowExclusiveFullscreen'
        $Runner | Should -Match 'Exclusive fullscreen requires -AllowExclusiveFullscreen'
        $Runner | Should -Match '--allow-exclusive-fullscreen'
    }

    It 'keeps the native visual regression gate offline' {
        $Runner | Should -Match 'visual_regression_contract'
        $Runner | Should -Match "'--locked', '--offline'"
        $Runner | Should -Not -Match '(?i)flutter|\.dart'
    }

    It 'detects the live graphics API and matching injected module' {
        $Runner | Should -Match "d3d12core\.dll"
        $Runner | Should -Match 'loaded_overlay_module'
        $Runner | Should -Match 'injector_probe_log'
        $Runner | Should -Match 'single_loaded_runtime_module'
        $Runner | Should -Match 'pal-fullscreen-overlay-\$api\.dll'
        $Runner | Should -Match 'n_overlay\.x64\.dll'
        $Runner | Should -Match 'exactly one Palworld process'
    }
}
