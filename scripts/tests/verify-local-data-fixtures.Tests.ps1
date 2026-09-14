$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

BeforeAll {
    $script:Workspace = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Verifier = Join-Path $script:Workspace 'scripts\verify-local-data-fixtures.ps1'
    $script:FixtureRoot = Join-Path $script:Workspace 'tests\fixtures\local-data'
}

Describe 'local data fixtures' {
    It 'contains every required catalog capability exactly once' {
        $result = & $script:Verifier -Root $script:FixtureRoot -Json |
            ConvertFrom-Json

        ($result.capabilities -join ',') | Should -Be (
            'pals,skills,items,recipes,acquisition,breeding,' +
            'technologies,buildings,locations,pois,aliases,localization,sources'
        )
    }

    It 'rejects an orphan acquisition target' {
        $copyRoot = Join-Path $TestDrive 'local-data'
        Copy-Item -LiteralPath $script:FixtureRoot -Destination $copyRoot -Recurse
        $catalogRoot = Join-Path $copyRoot 'catalog-synthetic-v1'
        Add-Content -LiteralPath (Join-Path $catalogRoot 'acquisition.ndjson') -Value (
            '{"schema_major":1,"game_build_id":"steam:24181527",' +
            '"source_id":"fixture:bad","entity_version":"steam:24181527",' +
            '"method_id":"bad","kind":"pal_drop","item_id":"missing",' +
            '"pal_drop":{"pal_id":"FixturePal","minimum_quantity":1,' +
            '"maximum_quantity":1,"probability_ppm":1000000}}'
        )

        & $script:Verifier -Root $catalogRoot *> $null
        $LASTEXITCODE | Should -Be 1
    }

    It 'contains one full redacted Cloud profile projection bound to the save fixture' {
        $result = & $script:Verifier -Root $script:FixtureRoot -Json |
            ConvertFrom-Json

        $result.cloud_profile.section_count | Should -Be 6
        $result.cloud_profile.source_snapshot_matches | Should -BeTrue
        $result.cloud_profile.identity_matches | Should -BeTrue
        $result.cloud_profile.private_field_count | Should -Be 0
    }

    It 'contains one bounded private tenant catalog candidate' {
        $result = & $script:Verifier -Root $script:FixtureRoot -Json |
            ConvertFrom-Json

        $result.private_catalog.distribution_scope |
            Should -Be 'private_tenant_candidate'
        $result.private_catalog.asset_field_count | Should -Be 0
        $result.private_catalog.hash_mismatch_count | Should -Be 0
    }
}
