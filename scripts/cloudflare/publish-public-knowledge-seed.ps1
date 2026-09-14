#Requires -Version 5.1

[CmdletBinding()]
param(
    [Parameter(Mandatory)]
    [string]$SeedDirectory,
    [switch]$Apply,
    [switch]$Promote,
    [switch]$AllowBudgetOverride,
    [ValidateRange(1, 100000)]
    [long]$DailyWriteBudget = 90000,
    [string]$RemoteStateFile,
    [string]$DatabaseBinding = 'DB',
    [string]$WranglerConfig
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-FileSha256 {
    param([Parameter(Mandatory)][string]$Path)

    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).
        Hash.ToLowerInvariant()
}

function ConvertFrom-WranglerJson {
    param([Parameter(Mandatory)][AllowEmptyCollection()][object[]]$Output)

    $text = ($Output | ForEach-Object { [string]$_ }) -join "`n"
    try {
        return $text | ConvertFrom-Json
    }
    catch {
        throw "Wrangler did not return valid JSON: $text"
    }
}

function Invoke-WranglerJson {
    param([Parameter(Mandatory)][string[]]$Argument)

    $output = & $miseRunner `
        -Workspace $repoRoot `
        -Run node `
        -RunArgument (@($wranglerEntry) + $Argument) 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "Wrangler failed: $($output -join "`n")"
    }
    return ConvertFrom-WranglerJson -Output @($output)
}

function Get-FirstQueryRow {
    param([Parameter(Mandatory)][object]$Payload)

    $response = @($Payload)[0]
    if (-not $response.success) {
        throw 'D1 query was not successful.'
    }
    $rows = @($response.results)
    if ($rows.Count -ne 1) {
        throw "Expected one D1 state row, received $($rows.Count)."
    }
    return $rows[0]
}

function Get-RemoteState {
    param(
        [Parameter(Mandatory)][string]$DatasetVersion,
        [Parameter(Mandatory)][string]$CheckpointPipelineName
    )

    $datasetSql = $DatasetVersion.Replace("'", "''")
    $checkpointPipelineSql = $CheckpointPipelineName.Replace("'", "''")
    $query = @"
SELECT
  COALESCE((SELECT dataset_version FROM data_versions
    WHERE dataset_scope='public_metadata' AND verified=1
      AND activated_at IS NOT NULL ORDER BY activated_at DESC LIMIT 1), '')
    AS active_dataset_version,
  COALESCE((SELECT source_hash FROM data_versions
    WHERE dataset_scope='public_metadata' AND verified=1
      AND activated_at IS NOT NULL ORDER BY activated_at DESC LIMIT 1), '')
    AS active_source_hash,
  EXISTS(SELECT 1 FROM data_versions
    WHERE dataset_version='$datasetSql') AS target_exists,
  COALESCE((SELECT source_hash FROM data_versions
    WHERE dataset_version='$datasetSql'), '') AS target_source_hash,
  COALESCE((SELECT verified FROM data_versions
    WHERE dataset_version='$datasetSql'), 0) AS target_verified,
  EXISTS(SELECT 1 FROM data_versions
    WHERE dataset_version='$datasetSql' AND activated_at IS NOT NULL)
    AS target_activated,
  COALESCE((SELECT publication_status FROM knowledge_dataset_manifests
    WHERE dataset_version='$datasetSql'), '') AS target_manifest_status,
  COALESCE((SELECT json_group_array(json_object(
      'path', json_extract(checkpoint.input_facets_json, '$.path'),
      'payload_sha256', json_extract(
        checkpoint.input_facets_json, '$.payload_sha256'),
      'apply_order', json_extract(
        checkpoint.input_facets_json, '$.apply_order'),
      'base_rows_written_upper_bound', json_extract(
        checkpoint.output_facets_json, '$.base_rows_written_upper_bound'),
      'conservative_rows_written', json_extract(
        checkpoint.output_facets_json, '$.conservative_rows_written')
    )) FROM (
      SELECT input_facets_json, output_facets_json
      FROM knowledge_pipeline_runs
      WHERE dataset_version='$datasetSql'
        AND pipeline_name='$checkpointPipelineSql'
        AND status='completed'
      ORDER BY CAST(json_extract(input_facets_json, '$.apply_order') AS INTEGER)
    ) checkpoint), '[]') AS seed_checkpoints_json;
"@
    $queryPayload = Invoke-WranglerJson -Argument @(
        'd1', 'execute', $DatabaseBinding,
        '--remote', '--command', $query, '--json',
        '--config', $WranglerConfig
    )
    $state = Get-FirstQueryRow -Payload $queryPayload
    $info = Invoke-WranglerJson -Argument @(
        'd1', 'info', $DatabaseBinding, '--json',
        '--config', $WranglerConfig
    )
    $state | Add-Member -NotePropertyName rows_written_24h `
        -NotePropertyValue ([long]$info.rows_written_24h) -Force
    return $state
}

function Get-SeedCheckpoints {
    param([Parameter(Mandatory)][object]$State)

    if ($State.PSObject.Properties.Name -contains 'seed_checkpoints') {
        return @($State.seed_checkpoints)
    }
    if (
        $State.PSObject.Properties.Name -contains 'seed_checkpoints_json' -and
        -not [string]::IsNullOrWhiteSpace(
            [string]$State.seed_checkpoints_json
        )
    ) {
        return @(
            [string]$State.seed_checkpoints_json | ConvertFrom-Json
        )
    }
    return @()
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$resolvedSeed = (Resolve-Path -LiteralPath $SeedDirectory).Path
$manifestPath = Join-Path $resolvedSeed 'seed-manifest.json'
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "Seed manifest is missing: $manifestPath"
}
$manifest = Get-Content -Raw -Encoding UTF8 -LiteralPath $manifestPath |
    ConvertFrom-Json
if (
    $manifest.schema -notin @(
        'pal-public-knowledge-seed-v1',
        'pal-public-knowledge-seed-v2'
    )
) {
    throw "Unsupported seed manifest schema: $($manifest.schema)"
}
$resumableSeed = $manifest.schema -eq 'pal-public-knowledge-seed-v2'
if ($resumableSeed) {
    if ($manifest.publish_strategy.schema -ne 'resumable-prefix-v1') {
        throw (
            'Unsupported resumable publish strategy: ' +
            $manifest.publish_strategy.schema
        )
    }
    $checkpointPipelineName = [string](
        $manifest.publish_strategy.checkpoint_pipeline_name
    )
    if ([string]::IsNullOrWhiteSpace($checkpointPipelineName)) {
        throw 'Resumable seed is missing its checkpoint pipeline name.'
    }
}
else {
    $checkpointPipelineName = 'public-metadata-seed-checkpoint-v1'
}
if ($manifest.dataset_scope -ne 'public_metadata') {
    throw "Refusing non-public metadata seed: $($manifest.dataset_scope)"
}
if ($manifest.dataset_version -notmatch '^[A-Za-z0-9_.:-]+$') {
    throw "Unsafe dataset version: $($manifest.dataset_version)"
}
if ($Promote -and -not $Apply) {
    throw '-Promote requires -Apply.'
}

$orderedFiles = @($manifest.sql_files | Sort-Object apply_order)
if ($orderedFiles.Count -eq 0) {
    throw 'Seed manifest has no SQL files.'
}
foreach ($record in $orderedFiles) {
    if ($record.path -notmatch '^[A-Za-z0-9_.-]+\.sql$') {
        throw "Unsafe SQL seed path: $($record.path)"
    }
    $path = Join-Path $resolvedSeed $record.path
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        throw "Seed SQL file is missing: $path"
    }
    $actualHash = Get-FileSha256 -Path $path
    if ($actualHash -ne $record.sha256) {
        throw "Seed SQL checksum mismatch: $($record.path)"
    }
    if ($resumableSeed) {
        if (
            $record.payload_sha256 -notmatch '^[a-f0-9]{64}$' -or
            [long]$record.conservative_rows_written -lt 1 -or
            [long]$record.base_rows_written_upper_bound -lt 1
        ) {
            throw "Invalid resumable cost metadata: $($record.path)"
        }
    }
}
$promotionPath = Join-Path $resolvedSeed $manifest.operator_promotion.path
if (-not (Test-Path -LiteralPath $promotionPath -PathType Leaf)) {
    throw "Promotion SQL file is missing: $promotionPath"
}
if ((Get-FileSha256 -Path $promotionPath) -ne $manifest.operator_promotion.sha256) {
    throw 'Promotion SQL checksum mismatch.'
}

if ($manifest.PSObject.Properties.Name -contains 'd1_cost_projection') {
    $estimatedRowsWritten = [long](
        $manifest.d1_cost_projection.conservative_rows_written
    )
}
else {
    $counts = $manifest.counts
    $sourceCount = @($manifest.source_artifacts).Count
    $baseRows = (
        [long]$counts.graph_nodes +
        (2 * [long]$counts.graph_edges) +
        (4 * [long]$counts.wiki_pages) +
        (4 * $sourceCount) +
        4
    )
    $estimatedRowsWritten = [long][Math]::Ceiling($baseRows * 3.0)
}

if ([string]::IsNullOrWhiteSpace($WranglerConfig)) {
    $WranglerConfig = Join-Path $repoRoot 'cloudflare\wrangler.toml'
}
else {
    $WranglerConfig = [IO.Path]::GetFullPath($WranglerConfig)
}
$miseRunner = Join-Path $repoRoot 'scripts\run-mise.ps1'
$wranglerEntry = Join-Path $repoRoot (
    'cloudflare\node_modules\wrangler\bin\wrangler.js'
)

if (-not [string]::IsNullOrWhiteSpace($RemoteStateFile)) {
    if ($Apply) {
        throw '-RemoteStateFile is plan-only and cannot be combined with -Apply.'
    }
    $state = Get-Content -Raw -Encoding UTF8 -LiteralPath $RemoteStateFile |
        ConvertFrom-Json
}
else {
    $state = Get-RemoteState `
        -DatasetVersion $manifest.dataset_version `
        -CheckpointPipelineName $checkpointPipelineName
}

$sameActiveProjection = (
    $state.active_source_hash -eq $manifest.projection_sha256
)
$completeTarget = (
    [bool][int]$state.target_exists -and
    $state.target_source_hash -eq $manifest.projection_sha256 -and
    [bool][int]$state.target_verified -and
    [bool][int]$state.target_activated -and
    $state.target_manifest_status -eq 'published'
)
$observedRowsWritten = [long]$state.rows_written_24h
$remainingRows = [Math]::Max(0, $DailyWriteBudget - $observedRowsWritten)
$scheduledFiles = @()
$completedFileCount = 0
$checkpointError = ''

if ($resumableSeed) {
    $checkpoints = @(Get-SeedCheckpoints -State $state)
    if (
        $checkpoints.Count -gt 0 -and
        -not [bool][int]$state.target_exists
    ) {
        $checkpointError = (
            'remote checkpoints exist without their target dataset'
        )
    }
    if ($checkpoints.Count -gt $orderedFiles.Count) {
        $checkpointError = 'remote checkpoint count exceeds the seed file count'
    }
    for ($index = 0; $index -lt $checkpoints.Count; $index++) {
        if (-not [string]::IsNullOrWhiteSpace($checkpointError)) {
            break
        }
        $checkpoint = $checkpoints[$index]
        $expected = $orderedFiles[$index]
        if (
            [int]$checkpoint.apply_order -ne $index -or
            [string]$checkpoint.path -ne [string]$expected.path -or
            [string]$checkpoint.payload_sha256 -ne
                [string]$expected.payload_sha256 -or
            [long]$checkpoint.base_rows_written_upper_bound -ne
                [long]$expected.base_rows_written_upper_bound -or
            [long]$checkpoint.conservative_rows_written -ne
                [long]$expected.conservative_rows_written
        ) {
            $checkpointError = (
                "remote checkpoint $index does not match the local seed"
            )
        }
    }
    if ([string]::IsNullOrWhiteSpace($checkpointError)) {
        $completedFileCount = $checkpoints.Count
    }
}

$pendingFiles = @($orderedFiles | Select-Object -Skip $completedFileCount)
$scheduledRowsWritten = [long]0
if ($resumableSeed) {
    foreach ($record in $pendingFiles) {
        $fileCost = [long]$record.conservative_rows_written
        if (
            -not $AllowBudgetOverride -and
            ($scheduledRowsWritten + $fileCost) -gt $remainingRows
        ) {
            break
        }
        $scheduledFiles += $record
        $scheduledRowsWritten += $fileCost
    }
}

$status = 'ready'
$reason = 'verified seed is within the configured D1 write budget'
if ($sameActiveProjection -or $completeTarget) {
    $status = 'up_to_date'
    $reason = 'the same projection is already active; no D1 write is required'
}
elseif (
    $resumableSeed -and
    [bool][int]$state.target_exists -and
    $state.target_source_hash -ne $manifest.projection_sha256
) {
    $status = 'blocked_target_mismatch'
    $reason = 'the target dataset exists with a different projection hash'
}
elseif ($resumableSeed) {
    if (-not [string]::IsNullOrWhiteSpace($checkpointError)) {
        $status = 'blocked_partial_dataset'
        $reason = $checkpointError
    }
    elseif (
        [bool][int]$state.target_exists -and
        $completedFileCount -eq 0
    ) {
        $status = 'blocked_partial_dataset'
        $reason = 'the target exists without a valid first-file checkpoint'
    }
    elseif ($pendingFiles.Count -eq 0) {
        if ($state.target_manifest_status -eq 'validated') {
            $status = 'ready_to_promote'
            $reason = 'all files are checkpointed and validation passed'
        }
        else {
            $status = 'blocked_validation'
            $reason = (
                'all files are checkpointed but the manifest is not validated'
            )
        }
    }
    elseif ($scheduledFiles.Count -eq 0) {
        $status = 'wait_for_write_reset'
        $reason = 'the next SQL file does not fit in the remaining daily budget'
    }
    elseif ($scheduledFiles.Count -lt $pendingFiles.Count) {
        $status = 'ready_partial'
        $reason = 'a contiguous seed prefix fits in the remaining daily budget'
    }
    else {
        $status = 'ready'
        $reason = 'all pending seed files fit in the remaining daily budget'
    }
}
elseif ([bool][int]$state.target_exists) {
    $status = 'blocked_partial_dataset'
    $reason = 'the legacy target dataset already exists but is not published'
}
elseif ($estimatedRowsWritten -gt $remainingRows -and -not $AllowBudgetOverride) {
    $status = 'blocked_write_budget'
    $reason = 'the conservative write estimate exceeds the remaining budget'
}

$plan = [ordered]@{
    status = $status
    reason = $reason
    dataset_version = $manifest.dataset_version
    projection_sha256 = $manifest.projection_sha256
    active_dataset_version = $state.active_dataset_version
    sql_file_count = $orderedFiles.Count
    observed_rows_written_24h = $observedRowsWritten
    daily_write_budget = $DailyWriteBudget
    estimated_rows_written = $estimatedRowsWritten
    remaining_rows_before_seed = $remainingRows
    completed_sql_file_count = $completedFileCount
    pending_sql_file_count = $pendingFiles.Count
    scheduled_sql_file_count = $scheduledFiles.Count
    scheduled_rows_written = $scheduledRowsWritten
    remaining_rows_after_scheduled = [Math]::Max(
        0,
        $remainingRows - $scheduledRowsWritten
    )
    next_budget_reset_utc = (
        [DateTime]::UtcNow.Date.AddDays(1).ToString('yyyy-MM-ddTHH:mm:ssZ')
    )
    apply_requested = [bool]$Apply
    promote_requested = [bool]$Promote
}

if ($status -eq 'up_to_date' -or -not $Apply) {
    Write-Output ([pscustomobject]$plan)
    return
}
if ($Promote -and $pendingFiles.Count -gt 0) {
    Write-Output ([pscustomobject]$plan)
    throw (
        'Promotion requires a separate review after every seed file is ' +
        'checkpointed.'
    )
}
if (
    $status -notin @('ready', 'ready_partial', 'ready_to_promote')
) {
    Write-Output ([pscustomobject]$plan)
    throw "Public knowledge seed is blocked: $reason"
}

if (-not $resumableSeed) {
    $scheduledFiles = $orderedFiles
}
foreach ($record in $scheduledFiles) {
    $path = Join-Path $resolvedSeed $record.path
    [void](Invoke-WranglerJson -Argument @(
        'd1', 'execute', $DatabaseBinding,
        '--remote', '--file', $path, '--json',
        '--config', $WranglerConfig
    ))
}
if ($Promote -and $pendingFiles.Count -eq $scheduledFiles.Count) {
    [void](Invoke-WranglerJson -Argument @(
        'd1', 'execute', $DatabaseBinding,
        '--remote', '--file', $promotionPath, '--json',
        '--config', $WranglerConfig
    ))
}
$plan.status = if ($Promote) {
    'applied_and_promoted'
}
elseif ($scheduledFiles.Count -lt $pendingFiles.Count) {
    'applied_partial'
}
else {
    'applied'
}
$plan.reason = if ($plan.status -eq 'applied_partial') {
    'the budgeted seed prefix was applied; rerun after the UTC reset'
}
else {
    'all scheduled checksum-verified SQL files were applied once'
}
Write-Output ([pscustomobject]$plan)
