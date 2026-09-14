#Requires -Version 5.1

BeforeAll {
    $script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Generator = Join-Path (
        $RepoRoot
    ) 'scripts\cloudflare\new-public-knowledge-seed.ps1'

    function Write-CatalogFixture {
        param(
            [string]$Root,
            [string]$BuildId = 'steam:fixture-build',
            [bool]$Verified = $true
        )

        New-Item -ItemType Directory -Path $Root -Force | Out-Null
        $items = [ordered]@{
            schema_version = 1
            game_build_id = $BuildId
            verified = $Verified
            mapping_sha256 = ('a' * 64)
            items = @(
                [ordered]@{
                    item_id = 'Stone'
                    name_ko = '돌'
                    description_ko = '공개하면 안 되는 원문 설명'
                    legal_in_game = $true
                    localization_fallback = $false
                    rarity = 0
                    rank = 1
                    maximum_stack_count = 9999
                    weight_milli = 1000
                    icon_name = 'T_Stone'
                },
                [ordered]@{
                    item_id = 'PalOil'
                    name_ko = '고급 팰 기름'
                    description_ko = '비공개 설명'
                    legal_in_game = $true
                    localization_fallback = $false
                    rarity = 1
                    rank = 2
                    maximum_stack_count = 999
                    weight_milli = 500
                    icon_name = 'T_PalOil'
                },
                [ordered]@{
                    item_id = 'PalOilRare'
                    name_ko = '고급 팰 기름'
                    description_ko = '비공개 희귀 설명'
                    legal_in_game = $true
                    localization_fallback = $false
                    rarity = 2
                    rank = 2
                    maximum_stack_count = 999
                    weight_milli = 500
                    icon_name = 'T_PalOilRare'
                },
                [ordered]@{
                    item_id = 'Fallback'
                    name_ko = 'Fallback'
                    description_ko = '제외'
                    legal_in_game = $true
                    localization_fallback = $true
                    rarity = 0
                    rank = 0
                    maximum_stack_count = 1
                    weight_milli = 0
                    icon_name = 'T_Fallback'
                }
            )
            recipes = @(
                [ordered]@{
                    recipe_id = 'PalOil'
                    output_item_id = 'PalOil'
                    output_quantity = 1
                    ingredients = @(
                        [ordered]@{ item_id = 'Stone'; quantity = 2 }
                    )
                    work_amount = 100
                },
                [ordered]@{
                    recipe_id = 'Excluded'
                    output_item_id = 'Fallback'
                    output_quantity = 1
                    ingredients = @(
                        [ordered]@{ item_id = 'Stone'; quantity = 1 }
                    )
                    work_amount = 1
                }
            )
        }
        $world = [ordered]@{
            schema_version = 1
            game_build_id = $BuildId
            verified = $Verified
            mapping_sha256 = ('a' * 64)
            technologies = @(
                [ordered]@{
                    technology_id = 'PalOil'
                    name_ko = '고급 팰 기름 기술'
                    description_ko = '기술 원문 설명'
                    localization_fallback = $false
                    level = 20
                    cost = 2
                    is_boss_technology = $false
                    required_technology_id = $null
                    unlock_item_ids = @('PalOil')
                    unlock_recipe_ids = @('PalOil')
                }
            )
        }
        $itemsPath = Join-Path $Root 'items.v1.json'
        $worldPath = Join-Path $Root 'world.v1.json'
        $items | ConvertTo-Json -Depth 20 |
            Set-Content -Encoding UTF8 -LiteralPath $itemsPath
        $world | ConvertTo-Json -Depth 20 |
            Set-Content -Encoding UTF8 -LiteralPath $worldPath
        return [pscustomobject]@{
            Items = $itemsPath
            World = $worldPath
        }
    }
}

Describe 'Rights-safe public knowledge seed' {
    It 'emits exact structural graph and Korean Wiki without source prose or icons' {
        $catalogs = Write-CatalogFixture -Root (Join-Path $TestDrive 'catalogs')
        $output = Join-Path $TestDrive 'seed'

        & $Generator `
            -ItemsCatalog $catalogs.Items `
            -WorldCatalog $catalogs.World `
            -OutputDirectory $output `
            -DatasetVersion 'fixture-public-v1' `
            -GameVersion 'fixture' `
            -GameBuildId 'steam:fixture-build' `
            -ChunkSize 25 | Out-Null

        $manifest = Get-Content -Raw -Encoding UTF8 (
            Join-Path $output 'seed-manifest.json'
        ) | ConvertFrom-Json
        $manifest.dataset_scope | Should -Be 'public_metadata'
        $manifest.automatic_activation | Should -BeFalse
        $manifest.counts.items | Should -Be 3
        $manifest.counts.recipes | Should -Be 1
        $manifest.counts.technologies | Should -Be 1
        $manifest.counts.graph_nodes | Should -Be 10
        $manifest.counts.graph_edges | Should -Be 9
        $manifest.counts.wiki_pages | Should -Be 5
        $manifest.schema | Should -Be 'pal-public-knowledge-seed-v2'
        $manifest.publish_strategy.schema |
            Should -Be 'resumable-prefix-v1'
        $manifest.d1_cost_projection.model |
            Should -Be 'resumable-file-upper-bound-x3-v1'
        $manifest.d1_cost_projection.base_rows_written_upper_bound |
            Should -Be 76
        $manifest.d1_cost_projection.conservative_rows_written |
            Should -Be 228
        @($manifest.sql_files).Count | Should -Be 8
        foreach ($record in @($manifest.sql_files)) {
            $record.payload_sha256 | Should -Match '^[a-f0-9]{64}$'
            $record.base_rows_written_upper_bound | Should -BeGreaterThan 1
            $record.conservative_rows_written | Should -Be (
                3 * $record.base_rows_written_upper_bound
            )
        }

        $sql = Get-ChildItem -LiteralPath $output -Filter '*.sql' |
            ForEach-Object { Get-Content -Raw -Encoding UTF8 $_.FullName }
        ($sql -join "`n") | Should -Match "'public_metadata'"
        ($sql -join "`n") | Should -Match '고급 팰 기름 · 희귀도 1'
        ($sql -join "`n") | Should -Match '고급 팰 기름 · 희귀도 2'
        ($sql -join "`n") | Should -Not -Match '공개하면 안 되는 원문 설명'
        ($sql -join "`n") | Should -Not -Match '비공개 설명'
        ($sql -join "`n") | Should -Not -Match '비공개 희귀 설명'
        ($sql -join "`n") | Should -Not -Match '기술 원문 설명'
        ($sql -join "`n") | Should -Not -Match 'T_PalOil'
        ($sql -join "`n") | Should -Not -Match '(?im)^BEGIN\b'
        ($sql -join "`n") | Should -Not -Match '(?im)^COMMIT\b'
        ($sql -join "`n") | Should -Match (
            "'public-metadata-seed-checkpoint-v1'"
        )
    }

    It 'rejects an unverified or different-Build source' {
        $catalogs = Write-CatalogFixture `
            -Root (Join-Path $TestDrive 'wrong') `
            -BuildId 'steam:different'

        {
            & $Generator `
                -ItemsCatalog $catalogs.Items `
                -WorldCatalog $catalogs.World `
                -OutputDirectory (Join-Path $TestDrive 'seed-wrong') `
                -DatasetVersion 'fixture-public-v1' `
                -GameBuildId 'steam:expected'
        } | Should -Throw '*does not match requested*'
    }

    It 'refuses to overwrite an existing output directory' {
        $catalogs = Write-CatalogFixture -Root (Join-Path $TestDrive 'existing')
        $output = Join-Path $TestDrive 'already-there'
        New-Item -ItemType Directory -Path $output | Out-Null

        {
            & $Generator `
                -ItemsCatalog $catalogs.Items `
                -WorldCatalog $catalogs.World `
                -OutputDirectory $output `
                -DatasetVersion 'fixture-public-v1' `
                -GameBuildId 'steam:fixture-build'
        } | Should -Throw '*refusing to overwrite*'
    }
}
