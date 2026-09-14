#Requires -Version 5.1

BeforeAll {
    $script:RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
    $script:Publisher = Join-Path $RepoRoot (
        'scripts\cloudflare\publish-public-knowledge-seed.ps1'
    )
    $script:Seed = Join-Path $RepoRoot (
        'artifacts\cloudflare\public-knowledge-steam-24467282-v1'
    )

    function Write-RemoteState {
        param(
            [string]$Path,
            [string]$ActiveHash = '',
            [int]$TargetExists = 0,
            [int]$TargetVerified = 0,
            [int]$TargetActivated = 0,
            [string]$TargetHash = '',
            [string]$ManifestStatus = '',
            [long]$RowsWritten = 0,
            [object[]]$SeedCheckpoints = @()
        )

        [ordered]@{
            active_dataset_version = 'active-fixture'
            active_source_hash = $ActiveHash
            target_exists = $TargetExists
            target_source_hash = $TargetHash
            target_verified = $TargetVerified
            target_activated = $TargetActivated
            target_manifest_status = $ManifestStatus
            rows_written_24h = $RowsWritten
            seed_checkpoints = $SeedCheckpoints
        } | ConvertTo-Json | Set-Content -Encoding UTF8 -LiteralPath $Path
    }

    function Write-ResumableSeed {
        param(
            [Parameter(Mandatory)][string]$Root,
            [long[]]$FileCosts = @(30000, 30000, 30000, 30000)
        )

        New-Item -ItemType Directory -Path $Root | Out-Null
        $records = New-Object Collections.Generic.List[object]
        for ($index = 0; $index -lt $FileCosts.Count; $index++) {
            $name = 'seed_{0:D4}.sql' -f $index
            $path = Join-Path $Root $name
            "SELECT $index;" | Set-Content -Encoding UTF8 -LiteralPath $path
            $records.Add([pscustomobject][ordered]@{
                path = $name
                sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).
                    Hash.ToLowerInvariant()
                payload_sha256 = ('{0:x}' -f ($index + 1)).PadLeft(64, '0')
                bytes = (Get-Item -LiteralPath $path).Length
                apply_order = $index
                base_rows_written_upper_bound = [long]($FileCosts[$index] / 3)
                conservative_rows_written = $FileCosts[$index]
            })
        }
        $promotionPath = Join-Path $Root 'OPERATOR_PROMOTE_AFTER_REVIEW.sql'
        'SELECT 1;' | Set-Content -Encoding UTF8 -LiteralPath $promotionPath
        $projection = 'c' * 64
        [ordered]@{
            schema = 'pal-public-knowledge-seed-v2'
            projection_sha256 = $projection
            dataset_version = 'fixture-resumable-v2'
            dataset_scope = 'public_metadata'
            source_artifacts = @()
            counts = [ordered]@{
                graph_nodes = 0
                graph_edges = 0
                wiki_pages = 0
            }
            publish_strategy = [ordered]@{
                schema = 'resumable-prefix-v1'
                checkpoint_pipeline_name = (
                    'public-metadata-seed-checkpoint-v1'
                )
                checkpoint_base_rows_per_file = 2
            }
            d1_cost_projection = [ordered]@{
                model = 'resumable-file-upper-bound-x3-v1'
                base_rows_written_upper_bound = [long](
                    ($records |
                        Measure-Object base_rows_written_upper_bound -Sum).Sum
                )
                conservative_rows_written = [long](
                    ($records |
                        Measure-Object conservative_rows_written -Sum).Sum
                )
            }
            sql_files = @($records | ForEach-Object { $_ })
            operator_promotion = [ordered]@{
                path = 'OPERATOR_PROMOTE_AFTER_REVIEW.sql'
                sha256 = (
                    Get-FileHash -Algorithm SHA256 -LiteralPath $promotionPath
                ).Hash.ToLowerInvariant()
                bytes = (Get-Item -LiteralPath $promotionPath).Length
            }
        } | ConvertTo-Json -Depth 20 |
            Set-Content -Encoding UTF8 -LiteralPath (
                Join-Path $Root 'seed-manifest.json'
            )
        return [pscustomobject]@{
            Root = $Root
            Projection = $projection
            Records = @($records | ForEach-Object { $_ })
        }
    }

    function New-Checkpoint {
        param([Parameter(Mandatory)][object]$Record)

        return [ordered]@{
            path = $Record.path
            payload_sha256 = $Record.payload_sha256
            apply_order = $Record.apply_order
            base_rows_written_upper_bound = (
                $Record.base_rows_written_upper_bound
            )
            conservative_rows_written = $Record.conservative_rows_written
        }
    }
}

Describe 'Public knowledge seed publishing guard' {
    It 'skips an already active byte-identical projection' {
        $manifest = Get-Content -Raw -Encoding UTF8 (
            Join-Path $Seed 'seed-manifest.json'
        ) | ConvertFrom-Json
        $state = Join-Path $TestDrive 'active.json'
        Write-RemoteState -Path $state -ActiveHash $manifest.projection_sha256

        $plan = & $Publisher -SeedDirectory $Seed -RemoteStateFile $state

        $plan.status | Should -Be 'up_to_date'
        $plan.estimated_rows_written | Should -Be 124326
        $plan.apply_requested | Should -BeFalse
    }

    It 'blocks a full snapshot that exceeds the remaining write budget' {
        $state = Join-Path $TestDrive 'budget.json'
        Write-RemoteState -Path $state -RowsWritten 1000

        $plan = & $Publisher -SeedDirectory $Seed -RemoteStateFile $state

        $plan.status | Should -Be 'blocked_write_budget'
        $plan.remaining_rows_before_seed | Should -Be 89000
    }

    It 'fails closed when a partial target dataset already exists' {
        $state = Join-Path $TestDrive 'partial.json'
        Write-RemoteState `
            -Path $state `
            -TargetExists 1 `
            -TargetHash ('a' * 64) `
            -ManifestStatus 'candidate'

        $plan = & $Publisher -SeedDirectory $Seed -RemoteStateFile $state

        $plan.status | Should -Be 'blocked_partial_dataset'
    }

    It 'schedules only the contiguous prefix inside the daily guard' {
        $seed = Write-ResumableSeed -Root (Join-Path $TestDrive 'resumable')
        $state = Join-Path $TestDrive 'resumable-budget.json'
        Write-RemoteState -Path $state -RowsWritten 1000

        $plan = & $Publisher -SeedDirectory $seed.Root -RemoteStateFile $state

        $plan.status | Should -Be 'ready_partial'
        $plan.completed_sql_file_count | Should -Be 0
        $plan.pending_sql_file_count | Should -Be 4
        $plan.scheduled_sql_file_count | Should -Be 2
        $plan.scheduled_rows_written | Should -Be 60000
        $plan.remaining_rows_after_scheduled | Should -Be 29000
    }

    It 'resumes after an exact contiguous checkpoint prefix' {
        $seed = Write-ResumableSeed -Root (Join-Path $TestDrive 'resume')
        $state = Join-Path $TestDrive 'resume.json'
        $checkpoints = @(
            New-Checkpoint $seed.Records[0]
            New-Checkpoint $seed.Records[1]
        )
        Write-RemoteState `
            -Path $state `
            -TargetExists 1 `
            -TargetHash $seed.Projection `
            -ManifestStatus 'candidate' `
            -SeedCheckpoints $checkpoints

        $plan = & $Publisher -SeedDirectory $seed.Root -RemoteStateFile $state

        $plan.status | Should -Be 'ready'
        $plan.completed_sql_file_count | Should -Be 2
        $plan.pending_sql_file_count | Should -Be 2
        $plan.scheduled_sql_file_count | Should -Be 2
    }

    It 'waits for reset when the next file does not fit' {
        $seed = Write-ResumableSeed -Root (Join-Path $TestDrive 'wait')
        $state = Join-Path $TestDrive 'wait.json'
        Write-RemoteState -Path $state -RowsWritten 70000

        $plan = & $Publisher -SeedDirectory $seed.Root -RemoteStateFile $state

        $plan.status | Should -Be 'wait_for_write_reset'
        $plan.scheduled_sql_file_count | Should -Be 0
        $plan.scheduled_rows_written | Should -Be 0
    }

    It 'fails closed when a remote checkpoint differs from the seed' {
        $seed = Write-ResumableSeed -Root (Join-Path $TestDrive 'mismatch')
        $state = Join-Path $TestDrive 'checkpoint-mismatch.json'
        $checkpoint = New-Checkpoint $seed.Records[0]
        $checkpoint.payload_sha256 = 'f' * 64
        Write-RemoteState `
            -Path $state `
            -TargetExists 1 `
            -TargetHash $seed.Projection `
            -ManifestStatus 'candidate' `
            -SeedCheckpoints @($checkpoint)

        $plan = & $Publisher -SeedDirectory $seed.Root -RemoteStateFile $state

        $plan.status | Should -Be 'blocked_partial_dataset'
        $plan.reason | Should -Match 'does not match'
    }

    It 'requires manifest validation after every file is checkpointed' {
        $seed = Write-ResumableSeed -Root (Join-Path $TestDrive 'validate')
        $state = Join-Path $TestDrive 'validate.json'
        $checkpoints = @(
            $seed.Records | ForEach-Object { New-Checkpoint $_ }
        )
        Write-RemoteState `
            -Path $state `
            -TargetExists 1 `
            -TargetHash $seed.Projection `
            -ManifestStatus 'candidate' `
            -SeedCheckpoints $checkpoints

        $plan = & $Publisher -SeedDirectory $seed.Root -RemoteStateFile $state

        $plan.status | Should -Be 'blocked_validation'
        $plan.reason | Should -Match 'not validated'
    }

    It 'offers promotion only after checkpoint and validation completion' {
        $seed = Write-ResumableSeed -Root (Join-Path $TestDrive 'promote')
        $state = Join-Path $TestDrive 'promote.json'
        $checkpoints = @(
            $seed.Records | ForEach-Object { New-Checkpoint $_ }
        )
        Write-RemoteState `
            -Path $state `
            -TargetExists 1 `
            -TargetHash $seed.Projection `
            -ManifestStatus 'validated' `
            -SeedCheckpoints $checkpoints

        $plan = & $Publisher -SeedDirectory $seed.Root -RemoteStateFile $state

        $plan.status | Should -Be 'ready_to_promote'
        $plan.completed_sql_file_count | Should -Be 4
        $plan.pending_sql_file_count | Should -Be 0
        $plan.scheduled_sql_file_count | Should -Be 0
    }
}
