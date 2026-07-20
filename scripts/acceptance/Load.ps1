function Get-AcceptanceLibraryPaths {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $root = Join-Path (Split-Path -Parent $EntryPath) 'acceptance'
    return @(
        'Common.ps1',
        'RuntimePaths.ps1',
        'Process.ps1',
        'Bds.ps1',
        'Markers.ps1',
        'Galleries\Common.ps1',
        'Galleries\Leaves.ps1',
        'Galleries\CrossCrop.ps1',
        'Galleries\Aquatic.ps1',
        'Galleries\Water.ps1',
        'Galleries\FlowerBed.ps1',
        'Galleries\SlabStair.ps1',
        'Galleries\Vine.ps1',
        'Proofs.ps1',
        'Resources.ps1',
        'Metrics.ps1',
        'Orchestration\Validate.ps1',
        'Orchestration\Execute.ps1',
        'Orchestrator.ps1'
    ) | ForEach-Object { Join-Path $root $_ }
}

function Get-AcceptanceCompositeSource {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $parts = @([IO.File]::ReadAllText($EntryPath))
    $parts += @(
        Get-AcceptanceLibraryPaths -EntryPath $EntryPath |
            ForEach-Object { [IO.File]::ReadAllText($_) }
    )
    return $parts -join "`n"
}

function Get-Phase2ProjectRoot {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    return [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $EntryPath) '..'))
}

function Assert-Phase2RunId {
    param([Parameter(Mandatory = $true)][string]$RunId)

    if ($RunId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$' -or
        $RunId -eq '.' -or $RunId -eq '..') {
        throw "invalid Phase 2 RunId: $RunId"
    }
}

function Assert-Phase2Duration {
    param([Parameter(Mandatory = $true)][int]$DurationSeconds)

    if ($DurationSeconds -lt 150) {
        throw 'DurationSeconds must be at least 150 for the 30-second warmup and uninterrupted 120-second sample'
    }
}

function Resolve-Phase2ContainedPath {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][ValidateSet('Local', 'Phase2')][string]$Scope
    )

    $root = if ($Scope -eq 'Phase2') {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot '.local\phase2'))
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot '.local'))
    }
    $candidate = if ([IO.Path]::IsPathRooted($Path)) {
        [IO.Path]::GetFullPath($Path)
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot $Path))
    }
    $prefix = $root.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "path must remain beneath $root"
    }
    return $candidate
}

function New-Phase2RunDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][ValidateSet('remote', 'galleries', 'motion')][string]$Kind,
        [Parameter(Mandatory = $true)][string]$RunId
    )

    Assert-Phase2RunId -RunId $RunId
    $root = Resolve-Phase2ContainedPath -ProjectRoot $ProjectRoot -Path ".local\phase2\$Kind\placeholder" -Scope Phase2
    $root = Split-Path -Parent $root
    $runDirectory = Join-Path $root $RunId
    if (Test-Path -LiteralPath $runDirectory) {
        throw "Phase 2 RunId already exists: $RunId"
    }
    New-Item -ItemType Directory -Path $runDirectory -ErrorAction Stop | Out-Null
    return $runDirectory
}

function New-Phase2PerformanceContract {
    return [pscustomobject][ordered]@{
        warmup_seconds = 30
        steady_seconds = 120
        p95_frame_ms_max = 16.6666666667
        p99_frame_ms_max = 16.6666666667
        max_frame_ms_max = 50.0
        lifecycle_ms_max = 2000.0
        resource_sample_count = 120
        max_combined_rss_bytes = 681574400
        mean_cpu_percent_max = 15.0
        p95_cpu_percent_max = 15.0
    }
}

function Write-Phase2Json {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value
    )

    $encoded = $Value | ConvertTo-Json -Depth 20
    [IO.File]::WriteAllText($Path, $encoded + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
}

function Assert-Phase2ExactProperties {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string[]]$Names,
        [Parameter(Mandatory = $true)][string]$Label
    )

    if ($null -eq $Value) { throw "$Label must be a JSON object" }
    $actual = @($Value.PSObject.Properties.Name)
    if ($actual.Count -ne $Names.Count) { throw "$Label has missing or unknown fields" }
    foreach ($name in $actual) {
        if ($Names -cnotcontains $name) { throw "$Label contains unknown field $name" }
    }
}

function Assert-Phase2UnsignedInteger {
    param(
        $Value,
        [Parameter(Mandatory = $true)][string]$Label,
        [uint64]$Maximum = [uint64]::MaxValue,
        [switch]$Positive
    )

    $integral = $Value -is [byte] -or $Value -is [uint16] -or $Value -is [uint32] -or $Value -is [uint64] -or
        $Value -is [sbyte] -or $Value -is [int16] -or $Value -is [int32] -or $Value -is [int64]
    if (-not $integral -or $Value -is [bool]) { throw "$Label must be an exact integral JSON number" }
    if ([decimal]$Value -lt 0 -or [decimal]$Value -gt [decimal]$Maximum) { throw "$Label is outside its unsigned bound" }
    if ($Positive -and [uint64]$Value -eq 0) { throw "$Label must be positive" }
}

function Assert-Phase2SignedInteger32 {
    param($Value, [Parameter(Mandatory = $true)][string]$Label)
    $integral = $Value -is [byte] -or $Value -is [uint16] -or $Value -is [uint32] -or
        $Value -is [sbyte] -or $Value -is [int16] -or $Value -is [int32] -or $Value -is [int64]
    if (-not $integral -or $Value -is [bool] -or
        [decimal]$Value -lt [int32]::MinValue -or [decimal]$Value -gt [int32]::MaxValue) {
        throw "$Label must be an exact signed 32-bit integral JSON number"
    }
}

function Assert-Phase2FiniteNonnegativeNumber {
    param($Value, [Parameter(Mandatory = $true)][string]$Label)
    if ($null -eq $Value -or $Value -is [bool] -or $Value -is [string]) {
        throw "$Label must be numeric"
    }
    try { $number = [double]$Value } catch { throw "$Label must be numeric" }
    if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or $number -lt 0.0) {
        throw "$Label must be finite and nonnegative"
    }
}

. (Join-Path $PSScriptRoot 'Phase2Publication.ps1')

function Get-Phase2PublicationSequenceEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [ValidateSet('debug', 'release')][string]$ExpectedBuildProfile = 'release',
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved,
        [Parameter(Mandatory = $true)][ValidateSet('Lbsg', 'Lunar', 'Zeqa')][string]$Server,
        [switch]$RequireCompleteLocalResetDispatchTrace
    )

    $lines = @(Get-Content -LiteralPath $ClientLogPath | Where-Object {
        $_.StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
    })
    if ($lines.Count -eq 0) {
        throw 'client log contains no PHASE2_PUBLICATION evidence'
    }
    $records = [Collections.Generic.List[object]]::new()
    $graphicsIdentity = $null
    $assetsIdentity = $null
    $previousRecord = $null
    $sequenceIdentity = $null
    $stableSequenceIdentity = $null
    $publisherInitialized = $false
    [uint64]$lastPublisherEpoch = 0
    $lastInitializedRecord = $null
    $pendingDimensionReset = $false
    $pendingLocalResetArmedCount = $null
    $resetCyclesObserved = 0
    foreach ($line in $lines) {
        $lower = $line.ToLowerInvariant()
        foreach ($forbidden in @('"path"', '"token"', '"auth"', '"payload"', '"credential"')) {
            if ($lower.Contains($forbidden)) {
                throw "PHASE2_PUBLICATION leaked forbidden field $forbidden"
            }
        }
        try {
            $record = $line.Substring('PHASE2_PUBLICATION='.Length) | ConvertFrom-Json
        }
        catch {
            throw 'client log contains malformed PHASE2_PUBLICATION JSON'
        }
        $publisherUninitialized = $null -eq $record.publication.publisher_radius_blocks -and
            $null -eq $record.publication.publisher_radius_chunks
        $publisherResetting = $publisherUninitialized -and $publisherInitialized
        Assert-Phase2PublicationRecord -Record $record -ExpectedPresentMode $ExpectedPresentMode `
            -ExpectedBuildProfile $ExpectedBuildProfile `
            -ExpectedGraphicsIdentity $graphicsIdentity -ExpectedAssetsIdentity $assetsIdentity `
            -AllowUninitializedPublisher:$publisherUninitialized -AllowResettingPublisher:$publisherResetting
        if ($RequireCompleteLocalResetDispatchTrace -and
            [bool]$record.publication.local_reset.dispatch_trace_overflowed) {
            throw 'PHASE2_PUBLICATION focused local reset dispatch trace overflowed its exact bound'
        }
        if ($null -eq $graphicsIdentity) {
            $graphicsIdentity = [string]$record.presentation.graphics_identity_sha256
            $assetsIdentity = [string]$record.presentation.assets_manifest_sha256
        }
        $currentStableSequenceIdentity = [pscustomobject][ordered]@{
            session_generation = [uint64]$record.publication.session_generation
            build_profile = [string]$record.presentation.build_profile
            requested_present_mode = [string]$record.presentation.requested_present_mode
            effective_present_mode = [string]$record.presentation.effective_present_mode
            present_mode_proven = [bool]$record.presentation.present_mode_proven
            graphics_identity_sha256 = [string]$record.presentation.graphics_identity_sha256
            assets_manifest_sha256 = [string]$record.presentation.assets_manifest_sha256
            client_blob_cache_enabled = [bool]$record.client_blob_cache_enabled
        } | ConvertTo-Json -Compress
        if ($null -eq $stableSequenceIdentity) {
            $stableSequenceIdentity = $currentStableSequenceIdentity
        }
        elseif ($currentStableSequenceIdentity -cne $stableSequenceIdentity) {
            throw 'PHASE2_PUBLICATION stable sequence attribution changed during the diagnostic capture'
        }
        if ($publisherUninitialized) {
            if (-not $publisherInitialized) {
                if ([uint64]$record.publication.publisher_epoch -ne 0) {
                    throw 'PHASE2_PUBLICATION initial uninitialized publisher has a nonzero epoch'
                }
            }
            else {
                if ([uint64]$record.publication.publisher_epoch -ne $lastPublisherEpoch -or
                    [int32]$record.publication.player_column.dimension -eq
                        [int32]$lastInitializedRecord.publication.player_column.dimension) {
                    throw 'PHASE2_PUBLICATION uninitialized publisher is not attributable to a dimension reset'
                }
                $pendingDimensionReset = $true
                $sequenceIdentity = $null
            }
        }
        else {
            $currentEpoch = [uint64]$record.publication.publisher_epoch
            $currentSequenceIdentity = [pscustomobject][ordered]@{
                session_generation = [uint64]$record.publication.session_generation
                publisher_epoch = $currentEpoch
                dimension = [int32]$record.publication.player_column.dimension
                publisher_radius_blocks = [uint64]$record.publication.publisher_radius_blocks
                publisher_radius_chunks = [uint64]$record.publication.publisher_radius_chunks
                build_profile = [string]$record.presentation.build_profile
                requested_present_mode = [string]$record.presentation.requested_present_mode
                effective_present_mode = [string]$record.presentation.effective_present_mode
                present_mode_proven = [bool]$record.presentation.present_mode_proven
                graphics_identity_sha256 = [string]$record.presentation.graphics_identity_sha256
                assets_manifest_sha256 = [string]$record.presentation.assets_manifest_sha256
                client_blob_cache_enabled = [bool]$record.client_blob_cache_enabled
            } | ConvertTo-Json -Compress
            if (-not $publisherInitialized -or $currentEpoch -gt $lastPublisherEpoch) {
                $sequenceIdentity = $currentSequenceIdentity
                $lastPublisherEpoch = $currentEpoch
                $pendingDimensionReset = $false
            }
            elseif ($currentEpoch -lt $lastPublisherEpoch -or $pendingDimensionReset -or
                $currentSequenceIdentity -cne $sequenceIdentity) {
                throw 'PHASE2_PUBLICATION sequence identity changed during the diagnostic capture'
            }
            $publisherInitialized = $true
            $lastInitializedRecord = $record
        }
        if ($null -ne $previousRecord) {
            $previousUninitialized = $null -eq $previousRecord.publication.publisher_radius_blocks -and
                $null -eq $previousRecord.publication.publisher_radius_chunks
            if (-not $publisherUninitialized -and -not $previousUninitialized -and
                [uint64]$record.publication.publisher_epoch -eq
                    [uint64]$previousRecord.publication.publisher_epoch) {
                $previousRequired = [uint64]$previousRecord.publication.required_columns
                $currentRequired = [uint64]$record.publication.required_columns
                $provenLocalResetArm = $currentRequired -eq 0 -and
                    [uint64]$record.publication.loaded_required_columns -eq 0 -and
                    -not [bool]$record.publication.required_cohort_stable -and
                    [bool]$record.publication.local_reset.armed -and
                    [uint64]$record.publication.local_reset.armed_count -eq
                        ([uint64]$previousRecord.publication.local_reset.armed_count + 1) -and
                    [uint64]$record.publication.local_reset.consumed_count -eq
                        [uint64]$previousRecord.publication.local_reset.consumed_count
                if ($currentRequired -lt $previousRequired -and -not $provenLocalResetArm) {
                    throw 'PHASE2_PUBLICATION publisher-epoch cohort membership regressed without a proven local reset arm'
                }
                if ($provenLocalResetArm) {
                    $pendingLocalResetArmedCount = [uint64]$record.publication.local_reset.armed_count
                    $resetCyclesObserved++
                }
                elseif (
                    ($currentRequired -eq $previousRequired -and
                        [string]$record.publication.required_cohort_hash -cne
                            [string]$previousRecord.publication.required_cohort_hash)) {
                    throw 'PHASE2_PUBLICATION publisher-epoch cohort membership regressed or changed without growth'
                }
            }
            $sameDimension = [int32]$record.publication.player_column.dimension -eq
                [int32]$previousRecord.publication.player_column.dimension
            if ($sameDimension) {
                foreach ($field in @('armed_count', 'consumed_count')) {
                    if ([uint64]$record.publication.local_reset.$field -lt
                        [uint64]$previousRecord.publication.local_reset.$field) {
                        throw "PHASE2_PUBLICATION cumulative local reset counter regressed: $field"
                    }
                }
            }
            if ($null -ne $pendingLocalResetArmedCount -and
                [uint64]$record.publication.publisher_epoch -gt
                    [uint64]$previousRecord.publication.publisher_epoch) {
                if ([bool]$record.publication.local_reset.armed -or
                    [uint64]$record.publication.local_reset.armed_count -ne
                        [uint64]$pendingLocalResetArmedCount -or
                    [uint64]$record.publication.local_reset.consumed_count -ne
                        [uint64]$pendingLocalResetArmedCount) {
                    throw 'PHASE2_PUBLICATION armed local reset was not consumed by the next publisher epoch'
                }
                $pendingLocalResetArmedCount = $null
            }
            elseif ($sameDimension -and
                [uint64]$record.publication.publisher_epoch -gt
                    [uint64]$previousRecord.publication.publisher_epoch -and
                [uint64]$record.publication.local_reset.armed_count -gt
                    [uint64]$previousRecord.publication.local_reset.armed_count) {
                if ([uint64]$record.publication.local_reset.armed_count -ne
                        ([uint64]$previousRecord.publication.local_reset.armed_count + 1) -or
                    [uint64]$record.publication.local_reset.consumed_count -ne
                        ([uint64]$previousRecord.publication.local_reset.consumed_count + 1) -or
                    [bool]$record.publication.local_reset.armed) {
                    throw 'PHASE2_PUBLICATION unobserved local reset arm lacks a persistent consumed counter proof'
                }
                $resetCyclesObserved++
            }
            foreach ($field in @('requests_constructed', 'requests_sent', 'responses_admitted',
                'subchunks_committed', 'decode_jobs_dispatched', 'decode_jobs_completed',
                'light_jobs_dispatched', 'light_jobs_completed', 'mesh_changes_queued',
                'mesh_changes_dequeued', 'mesh_jobs_dispatched', 'mesh_jobs_completed',
                'mesh_uploads_acknowledged')) {
                if ([uint64]$record.publication.stages.$field -lt [uint64]$previousRecord.publication.stages.$field) {
                    throw "PHASE2_PUBLICATION cumulative stage counter regressed: $field"
                }
            }
            foreach ($field in @('success', 'all_air', 'unavailable', 'malformed', 'stale', 'timed_out')) {
                if ([uint64]$record.publication.outcomes.$field -lt [uint64]$previousRecord.publication.outcomes.$field) {
                    throw "PHASE2_PUBLICATION cumulative outcome counter regressed: $field"
                }
            }
            foreach ($field in @('hashes_classified', 'hits', 'misses', 'admitted_blobs',
                'rejected_blobs', 'evictions', 'pending_resets', 'reconstructed_level_chunks',
                'reconstructed_sub_chunks')) {
                if ([uint64]$record.client_blob_cache.$field -lt [uint64]$previousRecord.client_blob_cache.$field) {
                    throw "PHASE2_PUBLICATION cumulative client blob cache counter regressed: $field"
                }
            }
        }
        $records.Add($record)
        $previousRecord = $record
    }
    $final = $records[$records.Count - 1]
    if ($null -ne $pendingLocalResetArmedCount -or
        [bool]$final.publication.local_reset.armed -or
        ($resetCyclesObserved -gt 0 -and -not [bool]$final.publication.required_cohort_stable)) {
        throw 'PHASE2_PUBLICATION local reset sequence did not reach a consumed stable publisher epoch'
    }
    $cache = $final.client_blob_cache
    if ([uint64]$cache.rejected_blobs -ne 0 -or
        [uint64]$cache.pending_transactions -ne 0 -or
        [uint64]$cache.pending_bytes -ne 0) {
        throw "PHASE2_PUBLICATION $Server terminal client blob cache state is rejected or pending"
    }
    if ($Server -ceq 'Lunar' -and
        (-not [bool]$final.client_blob_cache_enabled -or [uint64]$cache.hashes_classified -eq 0)) {
        throw 'PHASE2_PUBLICATION Lunar did not prove an enabled cache-backed route with attributable hashes'
    }
    $clientBlobCacheRoute = if ([bool]$final.client_blob_cache_enabled -and
        [uint64]$cache.hashes_classified -gt 0) { 'cache_backed' } else { 'ordinary_payload' }
    $stages = $final.publication.stages
    $attributable = [uint64]$stages.requests_constructed + [uint64]$stages.responses_admitted +
        [uint64]$stages.subchunks_committed + [uint64]$stages.decode_jobs_completed +
        [uint64]$stages.light_jobs_completed + [uint64]$stages.mesh_jobs_completed +
        [uint64]$stages.mesh_uploads_acknowledged
    if ($attributable -eq 0) {
        throw 'PHASE2_PUBLICATION sequence contains no attributable stage progress'
    }
    $firstStalledStage = Get-Phase2FirstStalledStage -PublicationRecord $final `
        -WorldReadyObserved:$WorldReadyObserved
    $findings = [Collections.Generic.List[string]]::new()
    if ([uint64]$final.publication.loaded_required_columns -lt [uint64]$final.publication.required_columns) {
        $findings.Add('persistent_required_column_hole')
        $findings.Add("first_stalled_stage:$firstStalledStage")
    }
    return [pscustomobject][ordered]@{
        SnapshotCount = $records.Count
        FinalPublication = $final
        FirstStalledStage = $firstStalledStage
        Findings = @($findings)
        ClientBlobCacheRoute = $clientBlobCacheRoute
    }
}

function Get-Phase2LocalResetSequenceEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [ValidateSet('debug', 'release')][string]$ExpectedBuildProfile = 'debug',
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved,
        [Parameter(Mandatory = $true)][ValidateSet('Lbsg', 'Lunar', 'Zeqa')][string]$Server
    )
    if (-not $WorldReadyObserved) {
        throw 'PHASE2_PUBLICATION focused Lifeboat witness requires world-ready observation'
    }
    $evidence = Get-Phase2PublicationSequenceEvidence -ClientLogPath $ClientLogPath `
        -ExpectedPresentMode $ExpectedPresentMode -ExpectedBuildProfile $ExpectedBuildProfile `
        -WorldReadyObserved:$WorldReadyObserved -Server $Server `
        -RequireCompleteLocalResetDispatchTrace
    if ([string]$evidence.FirstStalledStage -cne 'none') {
        throw "PHASE2_PUBLICATION focused Lifeboat terminal stage is stalled: $($evidence.FirstStalledStage)"
    }
    $records = @(Get-Content -LiteralPath $ClientLogPath | Where-Object {
        $_.StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
    } | ForEach-Object {
        $_.Substring('PHASE2_PUBLICATION='.Length) | ConvertFrom-Json
    })
    $first = $records[0]
    $final = $records[$records.Count - 1]
    if ([uint64]$final.publication.local_reset.armed_count -ne
            ([uint64]$first.publication.local_reset.armed_count + 1) -or
        [uint64]$final.publication.local_reset.consumed_count -ne
            ([uint64]$first.publication.local_reset.consumed_count + 1)) {
        throw 'PHASE2_PUBLICATION focused Lifeboat witness requires exactly one reset arm and consume cycle'
    }
    $firstColumn = $first.publication.player_column
    $finalColumn = $final.publication.player_column
    $columnChanged = [int32]$firstColumn.dimension -ne [int32]$finalColumn.dimension -or
        [int32]$firstColumn.x -ne [int32]$finalColumn.x -or
        [int32]$firstColumn.z -ne [int32]$finalColumn.z
    $centerChanged = (@($first.publication.publisher_center) -join ',') -cne
        (@($final.publication.publisher_center) -join ',')
    if (-not $columnChanged -or -not $centerChanged) {
        throw 'PHASE2_PUBLICATION focused Lifeboat witness did not change player column and publisher center'
    }
    if (-not [bool]$final.publication.player_column_required -or
        -not [bool]$final.publication.player_column_loaded -or
        [uint64]$final.publication.inactive_level_chunks -ne [uint64]$first.publication.inactive_level_chunks -or
        [uint64]$final.publication.outcomes.timed_out -ne [uint64]$first.publication.outcomes.timed_out) {
        throw 'PHASE2_PUBLICATION focused Lifeboat terminal player residency or zero-error deltas failed'
    }
    $player = $final.presentation.player_column
    foreach ($field in @('resident_subchunks', 'allocated_subchunks', 'submitted_subchunks', 'gpu_presented_subchunks')) {
        if ($null -eq $player.$field -or [uint64]$player.$field -eq 0) {
            throw "PHASE2_PUBLICATION focused Lifeboat terminal player $field is not positive"
        }
    }
    $dispatches = @($final.publication.local_reset.dispatch_classes)
    if ([uint64]$final.publication.local_reset.dispatch_total -eq 0 -or
        [bool]$final.publication.local_reset.dispatch_trace_overflowed -or
        $dispatches.Count -eq 0 -or
        [string]$dispatches[0] -cnotin @('player_initial', 'player_retry')) {
        throw 'PHASE2_PUBLICATION focused Lifeboat first successful dispatch is not player-class'
    }
    return $evidence
}

function Complete-Phase2DiagnosticEvidence {
    param(
        [Parameter(Mandatory = $true)]$Manifest,
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [string]$CoreLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved,
        [Parameter(Mandatory = $true)][ValidateSet('Lunar', 'Zeqa')][string]$Server,
        [Nullable[double]]$JoinMilliseconds
    )

    if ([string]$Manifest.mode -cne 'Diagnostic') {
        throw 'diagnostic evidence completion is valid only for Diagnostic mode'
    }
    $cacheBoundary = $null
    if (-not [string]::IsNullOrWhiteSpace($CoreLogPath)) {
        $cacheBoundary = Get-Phase2CacheBoundaryEvidence -CoreLogPath $CoreLogPath
        $Manifest | Add-Member -MemberType NoteProperty -Name cache_boundary_evidence `
            -Value $cacheBoundary -Force
    }
    $evidence = Get-Phase2PublicationSequenceEvidence -ClientLogPath $ClientLogPath `
        -ExpectedPresentMode $ExpectedPresentMode -WorldReadyObserved:$WorldReadyObserved -Server $Server
    Assert-Phase2CacheBoundaryConsistency -Server $Server `
        -ClientBlobCacheRoute $evidence.ClientBlobCacheRoute -BoundaryEvidence $cacheBoundary
    $findings = [Collections.Generic.List[string]]::new()
    foreach ($finding in @($evidence.Findings)) { $findings.Add($finding) }
    if (-not $WorldReadyObserved) { $findings.Insert(0, 'world_ready_not_observed') }
    $unavailableReason = if ($WorldReadyObserved) { 'diagnostic_mode_is_non_binding' } else { 'world_ready_not_observed' }
    $Manifest | Add-Member -MemberType NoteProperty -Name status -Value 'passed' -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name diagnostic_complete -Value $true -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name behavior_gate_passed -Value $false -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name world_ready_observed -Value $WorldReadyObserved -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name join_milliseconds -Value $(if ($WorldReadyObserved) { $JoinMilliseconds } else { $null }) -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name publication_snapshot_count -Value $evidence.SnapshotCount -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name first_stalled_stage -Value $evidence.FirstStalledStage -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name final_publication -Value $evidence.FinalPublication -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name client_blob_cache_route -Value $evidence.ClientBlobCacheRoute -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name findings -Value @($findings) -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name metrics_evidence -Value ([pscustomobject][ordered]@{ status = 'unavailable'; reason = $unavailableReason }) -Force
    $Manifest | Add-Member -MemberType NoteProperty -Name resources_evidence -Value ([pscustomobject][ordered]@{ status = 'unavailable'; reason = $unavailableReason }) -Force
}

function Find-Phase2CompletedLunarPrerequisite {
    param(
        [Parameter(Mandatory = $true)][string]$RemoteRoot,
        [Parameter(Mandatory = $true)][ValidateSet('Diagnostic', 'Candidate', 'Final')][string]$Mode,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo')][string]$ExpectedPresentMode,
        [Parameter(Mandatory = $true)][int]$ExpectedInitialRadius,
        [Parameter(Mandatory = $true)][bool]$RequireFullView
    )

    $allowedStages = @('none', 'required_cohort_identity', 'request_order', 'transport', 'wire_contract', 'response_semantics',
        'decode', 'lighting', 'meshing', 'main_apply', 'gpu_upload', 'extraction', 'submission', 'presentation')
    $manifestProperties = @(
        'auth_cache_scope', 'behavior_gate_passed', 'cache_boundary_evidence', 'client_arguments', 'client_blob_cache_route',
        'client_shutdown_grace_seconds',
        'diagnostic_complete', 'duration_seconds', 'final_publication', 'findings', 'first_stalled_stage',
        'full_view_teleport_gate', 'initial_radius', 'join_milliseconds', 'lunar_prerequisite_manifest_sha256',
        'lunar_prerequisite_mode', 'metrics_evidence', 'mode', 'performance', 'publication_snapshot_count',
        'requested_present_mode', 'require_effective_present_mode_proof', 'require_release_build',
        'resources_evidence', 'schema', 'server', 'status', 'upstream', 'world_ready_observed'
    )
    $performanceProperties = @(
        'lifecycle_ms_max', 'max_combined_rss_bytes', 'max_frame_ms_max', 'mean_cpu_percent_max',
        'p95_cpu_percent_max', 'p95_frame_ms_max', 'p99_frame_ms_max', 'resource_sample_count',
        'steady_seconds', 'warmup_seconds'
    )
    foreach ($file in @(Get-ChildItem -LiteralPath $RemoteRoot -Filter manifest.json -File -Recurse -ErrorAction SilentlyContinue | Sort-Object FullName)) {
        try {
            $candidate = Get-Content -Raw -LiteralPath $file.FullName | ConvertFrom-Json
            Assert-Phase2ExactProperties -Value $candidate -Names $manifestProperties -Label 'Phase 2 remote manifest'
            Assert-Phase2ExactProperties -Value $candidate.metrics_evidence -Names @('reason', 'status') `
                -Label 'Phase 2 remote manifest metrics_evidence'
            Assert-Phase2ExactProperties -Value $candidate.resources_evidence -Names @('reason', 'status') `
                -Label 'Phase 2 remote manifest resources_evidence'
            Assert-Phase2ExactProperties -Value $candidate.cache_boundary_evidence -Names @(
                'cached_level_chunks', 'cached_sub_chunks', 'classification', 'ordinary_level_chunks',
                'ordinary_sub_chunks', 'upstream_status_enabled', 'upstream_status_seen'
            ) -Label 'Phase 2 remote manifest cache_boundary_evidence'
            Assert-Phase2ExactProperties -Value $candidate.performance -Names $performanceProperties `
                -Label 'Phase 2 remote manifest performance'
            Assert-Phase2UnsignedInteger -Value $candidate.initial_radius -Label 'manifest.initial_radius' -Maximum 64 -Positive
            Assert-Phase2UnsignedInteger -Value $candidate.publication_snapshot_count `
                -Label 'manifest.publication_snapshot_count' -Maximum ([uint32]::MaxValue) -Positive
            Assert-Phase2UnsignedInteger -Value $candidate.duration_seconds -Label 'manifest.duration_seconds' `
                -Maximum ([uint32]::MaxValue) -Positive
            Assert-Phase2UnsignedInteger -Value $candidate.client_shutdown_grace_seconds `
                -Label 'manifest.client_shutdown_grace_seconds' -Maximum ([uint32]::MaxValue)
            foreach ($field in @('cached_level_chunks', 'ordinary_level_chunks', 'cached_sub_chunks', 'ordinary_sub_chunks')) {
                Assert-Phase2UnsignedInteger -Value $candidate.cache_boundary_evidence.$field `
                    -Label "manifest.cache_boundary_evidence.$field"
            }
            foreach ($field in @('warmup_seconds', 'steady_seconds', 'resource_sample_count', 'max_combined_rss_bytes')) {
                Assert-Phase2UnsignedInteger -Value $candidate.performance.$field `
                    -Label "manifest.performance.$field" -Positive
            }
            foreach ($field in @('p95_frame_ms_max', 'p99_frame_ms_max', 'max_frame_ms_max', 'lifecycle_ms_max',
                'mean_cpu_percent_max', 'p95_cpu_percent_max')) {
                Assert-Phase2FiniteNonnegativeNumber -Value $candidate.performance.$field `
                    -Label "manifest.performance.$field"
            }
            $contract = New-Phase2PerformanceContract
            foreach ($field in $performanceProperties) {
                if ([decimal]$candidate.performance.$field -ne [decimal]$contract.$field) {
                    throw "Phase 2 remote manifest performance contract changed at $field"
                }
            }
            if ([uint64]$candidate.duration_seconds -lt 150 -or
                [uint64]$candidate.client_shutdown_grace_seconds -ne 5) {
                throw 'Phase 2 remote manifest duration or shutdown grace is invalid'
            }
            if ($candidate.client_arguments -isnot [System.Array] -or $candidate.findings -isnot [System.Array]) {
                throw 'Phase 2 remote manifest arguments and findings must be JSON arrays'
            }
            foreach ($argument in @($candidate.client_arguments)) {
                if ($argument -isnot [string] -or [string]::IsNullOrWhiteSpace($argument)) {
                    throw 'Phase 2 remote manifest contains an invalid client argument'
                }
            }
            foreach ($finding in @($candidate.findings)) {
                if ($finding -isnot [string] -or [string]::IsNullOrWhiteSpace($finding)) {
                    throw 'Phase 2 remote manifest contains an invalid finding'
                }
            }
            if ([string]$candidate.schema -cne 'rust-mcbe-phase2-remote-v1' -or
                [string]$candidate.server -cne 'Lunar' -or
                [string]$candidate.upstream -cne 'pvp.lunarbedrock.com:19134' -or
                [string]$candidate.mode -cne $Mode -or
                [string]$candidate.status -cne 'passed' -or
                [uint64]$candidate.initial_radius -ne [uint64]$ExpectedInitialRadius -or
                [string]$candidate.requested_present_mode -cne 'Fifo' -or
                [string]$candidate.client_blob_cache_route -cne 'cache_backed' -or
                [string]$candidate.cache_boundary_evidence.classification -cne 'cache_backed' -or
                $candidate.cache_boundary_evidence.upstream_status_seen -isnot [bool] -or
                -not [bool]$candidate.cache_boundary_evidence.upstream_status_seen -or
                $candidate.cache_boundary_evidence.upstream_status_enabled -isnot [bool] -or
                -not [bool]$candidate.cache_boundary_evidence.upstream_status_enabled -or
                ([uint64]$candidate.cache_boundary_evidence.cached_level_chunks +
                    [uint64]$candidate.cache_boundary_evidence.cached_sub_chunks) -eq 0 -or
                $candidate.full_view_teleport_gate -isnot [bool] -or
                [bool]$candidate.full_view_teleport_gate -ne $RequireFullView -or
                ($Mode -cne 'Diagnostic' -and -not $RequireFullView) -or
                $allowedStages -cnotcontains [string]$candidate.first_stalled_stage -or
                $candidate.world_ready_observed -isnot [bool] -or
                $candidate.require_effective_present_mode_proof -isnot [bool] -or
                -not [bool]$candidate.require_effective_present_mode_proof -or
                $candidate.require_release_build -isnot [bool] -or
                -not [bool]$candidate.require_release_build -or
                [string]$candidate.auth_cache_scope -cne '.local' -or
                $null -ne $candidate.lunar_prerequisite_mode -or
                $null -ne $candidate.lunar_prerequisite_manifest_sha256 -or
                $null -eq $candidate.final_publication) {
                continue
            }
            if ([bool]$candidate.world_ready_observed) {
                Assert-Phase2FiniteNonnegativeNumber -Value $candidate.join_milliseconds -Label 'manifest.join_milliseconds'
                if ([double]$candidate.join_milliseconds -gt 2000.0) { continue }
            }
            elseif ($null -ne $candidate.join_milliseconds) { continue }
            foreach ($evidenceName in @('metrics_evidence', 'resources_evidence')) {
                $evidence = $candidate.$evidenceName
                if ([string]$evidence.status -ceq 'passed') {
                    if ($null -ne $evidence.reason) { throw "manifest.$evidenceName passed with a reason" }
                }
                elseif ([string]$evidence.status -ceq 'unavailable') {
                    if ([string]$evidence.reason -cnotin @('world_ready_not_observed', 'diagnostic_mode_is_non_binding')) {
                        throw "manifest.$evidenceName has an invalid unavailable reason"
                    }
                }
                else { throw "manifest.$evidenceName has an invalid status" }
            }
            Assert-Phase2PublicationRecord -Record $candidate.final_publication `
                -ExpectedPresentMode Fifo
            $terminalCache = $candidate.final_publication.client_blob_cache
            if (-not [bool]$candidate.final_publication.client_blob_cache_enabled -or
                [uint64]$terminalCache.hashes_classified -eq 0 -or
                [uint64]$terminalCache.rejected_blobs -ne 0 -or
                [uint64]$terminalCache.pending_transactions -ne 0 -or
                [uint64]$terminalCache.pending_bytes -ne 0) {
                continue
            }
            $computedStage = Get-Phase2FirstStalledStage -PublicationRecord $candidate.final_publication `
                -WorldReadyObserved:([bool]$candidate.world_ready_observed)
            if ([string]$candidate.first_stalled_stage -cne $computedStage -or
                ($Mode -cne 'Diagnostic' -and $computedStage -cne 'none')) {
                continue
            }
            if ($Mode -eq 'Diagnostic') {
                if ($candidate.diagnostic_complete -isnot [bool] -or -not [bool]$candidate.diagnostic_complete -or
                    $candidate.behavior_gate_passed -isnot [bool] -or [bool]$candidate.behavior_gate_passed -or
                    $candidate.world_ready_observed -isnot [bool]) {
                    continue
                }
            }
            elseif ($candidate.diagnostic_complete -isnot [bool] -or [bool]$candidate.diagnostic_complete -or
                $candidate.behavior_gate_passed -isnot [bool] -or -not [bool]$candidate.behavior_gate_passed -or
                $candidate.world_ready_observed -isnot [bool] -or -not [bool]$candidate.world_ready_observed -or
                [string]$candidate.metrics_evidence.status -cne 'passed' -or
                [string]$candidate.resources_evidence.status -cne 'passed') {
                continue
            }
            $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            if ($hash -notmatch '^[0-9A-F]{64}$') { continue }
            return [pscustomobject][ordered]@{ Path = $file.FullName; Sha256 = $hash; Mode = $Mode }
        }
        catch { continue }
    }
    return $null
}

function Find-Phase2CompletedLunarDiagnostic {
    param([Parameter(Mandatory = $true)][string]$RemoteRoot)
    return Find-Phase2CompletedLunarPrerequisite -RemoteRoot $RemoteRoot -Mode Diagnostic `
        -ExpectedPresentMode Fifo -ExpectedInitialRadius 16 -RequireFullView:$false
}

. (Join-Path $PSScriptRoot 'Phase2Evidence.ps1')
