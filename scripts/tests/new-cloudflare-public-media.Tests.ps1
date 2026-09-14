#Requires -Version 5.1

Describe 'Cloudflare public media projection' {
BeforeAll {
    $script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Projector = Join-Path (
        $RepoRoot
    ) 'scripts\new-cloudflare-public-media.ps1'
    $script:Policy = Join-Path (
        $RepoRoot
    ) 'cloudflare\public-media-policy.json'
}

    It 'emits only reviewed owned brand files under immutable keys' {
        $output = Join-Path $TestDrive 'public-media'
        $result = & $Projector -OutputDirectory $output

        $result.asset_count | Should -Be 3
        $manifest = Get-Content -LiteralPath $result.manifest -Raw |
            ConvertFrom-Json
        $manifest.immutable | Should -BeTrue
        $manifest.bucket_name | Should -Be 'palbeacon-media'
        @($manifest.assets).Count | Should -Be 3
        foreach ($asset in @($manifest.assets)) {
            $asset.distribution_scope | Should -Be 'public_web_approved'
            $asset.object_key | Should -Match (
                '^public/v1/[a-f0-9]{64}/[A-Za-z0-9._-]+$'
            )
            $asset.request_path | Should -Match (
                '^/media/v2/[a-f0-9]{64}/[A-Za-z0-9._-]+$'
            )
            Test-Path (
                Join-Path (
                    Join-Path $output 'objects'
                ) ([string] $asset.object_key).Replace('/', '\')
            ) | Should -BeTrue
        }
    }

    It 'rejects any entry that is not approved for public Web use' {
        $policyCopy = Join-Path $TestDrive 'blocked-policy.json'
        $policyDocument = Get-Content -LiteralPath $Policy -Raw |
            ConvertFrom-Json
        $policyDocument.assets[0].distribution_scope = 'local_windows_only'
        $policyDocument | ConvertTo-Json -Depth 10 |
            Set-Content -LiteralPath $policyCopy -Encoding utf8

        {
            & $Projector `
                -PolicyPath $policyCopy `
                -OutputDirectory (Join-Path $TestDrive 'blocked-output')
        } | Should -Throw '*not approved for Web distribution*'
    }
}
