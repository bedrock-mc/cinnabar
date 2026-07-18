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

function Get-Phase2ExactCohortColumnCount {
    param([Parameter(Mandatory = $true)][uint64]$PublisherRadiusBlocks)
    $retention = [int][Math]::Ceiling($PublisherRadiusBlocks / 16.0)
    $count = [uint64]0
    for ($x = -$retention; $x -le $retention; $x++) {
        for ($z = -$retention; $z -le $retention; $z++) {
            $blockX = [int64]$x * 16
            $blockZ = [int64]$z * 16
            if (($blockX * $blockX) + ($blockZ * $blockZ) -le $PublisherRadiusBlocks * $PublisherRadiusBlocks) {
                $count++
            }
        }
    }
    return $count
}

function Assert-Phase2PublicationRecord {
    param(
        [Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [string]$ExpectedGraphicsIdentity,
        [string]$ExpectedAssetsIdentity,
        [switch]$AllowUninitializedPublisher
    )

    Assert-Phase2ExactProperties -Value $Record `
        -Names @('client_blob_cache', 'client_blob_cache_enabled', 'presentation', 'publication') `
        -Label 'PHASE2_PUBLICATION root'
    if ($Record.client_blob_cache_enabled -isnot [bool]) {
        throw 'PHASE2_PUBLICATION client_blob_cache_enabled must be a Boolean'
    }
    $cache = $Record.client_blob_cache
    $requiredCacheFields = @(
        'admitted_blobs', 'evictions', 'hashes_classified', 'hits', 'misses', 'pending_bytes',
        'pending_resets', 'pending_transactions', 'reconstructed_level_chunks',
        'reconstructed_sub_chunks', 'rejected_blobs'
    )
    Assert-Phase2ExactProperties -Value $cache -Names $requiredCacheFields `
        -Label 'PHASE2_PUBLICATION client_blob_cache'
    foreach ($field in $requiredCacheFields) {
        Assert-Phase2UnsignedInteger -Value $cache.$field -Label "client_blob_cache.$field"
    }
    if ([decimal]$cache.hits + [decimal]$cache.misses -ne [decimal]$cache.hashes_classified) {
        throw 'PHASE2_PUBLICATION client blob cache hit and miss totals disagree with hashes_classified'
    }
    if (-not [bool]$Record.client_blob_cache_enabled) {
        foreach ($field in $requiredCacheFields) {
            if ([uint64]$cache.$field -ne 0) {
                throw 'PHASE2_PUBLICATION disabled client blob cache contains nonzero counters'
            }
        }
    }
    $presentation = $Record.presentation
    $publication = $Record.publication
    Assert-Phase2ExactProperties -Value $presentation -Names @(
        'allocation', 'assets_manifest_sha256', 'build_profile', 'effective_present_mode', 'gpu_presented',
        'graphics_identity_sha256', 'present_mode_proven', 'publisher_disk', 'requested_present_mode',
        'resident', 'submitted', 'visible'
    ) -Label 'PHASE2_PUBLICATION presentation'
    Assert-Phase2ExactProperties -Value $publication -Names @(
        'loaded_required_columns', 'max_queue_wait_us', 'max_worker_time_us', 'outcomes', 'player_column',
        'publisher_radius_blocks', 'publisher_radius_chunks', 'required_cohort_hash', 'required_columns',
        'session_generation', 'stages'
    ) -Label 'PHASE2_PUBLICATION publication'
    $mode = $ExpectedPresentMode.ToLowerInvariant()
    if ($null -eq $presentation -or $null -eq $publication -or
        [string]$presentation.build_profile -cne 'release' -or
        [string]$presentation.requested_present_mode -cne $mode -or
        [string]$presentation.effective_present_mode -cne $mode -or
        $presentation.present_mode_proven -isnot [bool] -or
        -not [bool]$presentation.present_mode_proven -or
        [string]$presentation.graphics_identity_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$presentation.assets_manifest_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'PHASE2_PUBLICATION did not prove release build, effective present mode, graphics identity, and asset identity'
    }
    if (-not [string]::IsNullOrEmpty($ExpectedGraphicsIdentity) -and
        [string]$presentation.graphics_identity_sha256 -cne $ExpectedGraphicsIdentity) {
        throw 'PHASE2_PUBLICATION graphics identity changed during the diagnostic sequence'
    }
    if (-not [string]::IsNullOrEmpty($ExpectedAssetsIdentity) -and
        [string]$presentation.assets_manifest_sha256 -cne $ExpectedAssetsIdentity) {
        throw 'PHASE2_PUBLICATION assets identity changed during the diagnostic sequence'
    }
    Assert-Phase2UnsignedInteger -Value $publication.session_generation -Label 'publication.session_generation' -Positive
    Assert-Phase2UnsignedInteger -Value $publication.required_columns -Label 'publication.required_columns'
    Assert-Phase2UnsignedInteger -Value $publication.loaded_required_columns -Label 'publication.loaded_required_columns'
    $publisherUninitialized = $null -eq $publication.publisher_radius_blocks -and
        $null -eq $publication.publisher_radius_chunks
    if ($publisherUninitialized) {
        if (-not $AllowUninitializedPublisher) {
            throw 'publication.publisher_radius_blocks must be an exact integral JSON number'
        }
    }
    else {
        Assert-Phase2UnsignedInteger -Value $publication.publisher_radius_blocks -Label 'publication.publisher_radius_blocks' -Maximum 1024 -Positive
        Assert-Phase2UnsignedInteger -Value $publication.publisher_radius_chunks -Label 'publication.publisher_radius_chunks' -Maximum 64 -Positive
        $derivedRetentionRadius = [uint64][Math]::Ceiling([uint64]$publication.publisher_radius_blocks / 16.0)
        if ([uint64]$publication.publisher_radius_chunks -ne $derivedRetentionRadius) {
            throw 'PHASE2_PUBLICATION raw block radius disagrees with its derived retention radius'
        }
    }
    if ($null -eq $publication.stages -or $null -eq $publication.outcomes -or
        [string]$publication.required_cohort_hash -notmatch '^[0-9a-f]{16}$' -or
        [uint64]$publication.loaded_required_columns -gt [uint64]$publication.required_columns) {
        throw 'PHASE2_PUBLICATION lacks a coherent publication identity and bounded cohort counts'
    }
    Assert-Phase2ExactProperties -Value $publication.player_column -Names @('dimension', 'x', 'z') `
        -Label 'PHASE2_PUBLICATION player_column'
    foreach ($field in @('dimension', 'x', 'z')) {
        Assert-Phase2SignedInteger32 -Value $publication.player_column.$field -Label "publication.player_column.$field"
    }
    Assert-Phase2ExactProperties -Value $publication.outcomes `
        -Names @('all_air', 'malformed', 'stale', 'success', 'timed_out', 'unavailable') `
        -Label 'PHASE2_PUBLICATION outcomes'
    foreach ($field in @('all_air', 'malformed', 'stale', 'success', 'timed_out', 'unavailable')) {
        Assert-Phase2UnsignedInteger -Value $publication.outcomes.$field -Label "publication.outcomes.$field"
    }
    $requiredStageFields = @(
        'requests_constructed', 'requests_ready', 'requests_transport_pending', 'requests_sent',
        'responses_admitted',
        'subchunks_awaiting_response', 'subchunks_committed', 'decode_jobs_queued',
        'decode_jobs_dispatched', 'decode_jobs_in_flight', 'decode_jobs_completed',
        'light_jobs_queued', 'light_jobs_dispatched', 'light_jobs_in_flight',
        'light_jobs_completed', 'mesh_changes_queued', 'mesh_changes_pending',
        'mesh_changes_dequeued', 'mesh_jobs_queued', 'mesh_jobs_dispatched',
        'mesh_jobs_in_flight', 'mesh_jobs_completed', 'mesh_uploads_unacknowledged',
        'mesh_uploads_acknowledged'
    )
    Assert-Phase2ExactProperties -Value $publication.stages -Names $requiredStageFields `
        -Label 'PHASE2_PUBLICATION stages'
    foreach ($field in $requiredStageFields) {
        Assert-Phase2UnsignedInteger -Value $publication.stages.$field -Label "publication.stages.$field"
    }
    foreach ($prefix in @('decode_jobs', 'light_jobs', 'mesh_jobs')) {
        $dispatched = [uint64]$publication.stages.("${prefix}_dispatched")
        $completed = [uint64]$publication.stages.("${prefix}_completed")
        $inFlight = [uint64]$publication.stages.("${prefix}_in_flight")
        if ($completed -gt $dispatched) {
            throw "PHASE2_PUBLICATION ${prefix}_completed exceeds ${prefix}_dispatched"
        }
        if ($dispatched -ne [uint64]::MaxValue -and $completed -ne [uint64]::MaxValue -and
            $inFlight -ne ($dispatched - $completed)) {
            throw "PHASE2_PUBLICATION ${prefix}_in_flight disagrees with unsaturated dispatch counters"
        }
    }
    $constructed = [uint64]$publication.stages.requests_constructed
    $sent = [uint64]$publication.stages.requests_sent
    if ($sent -gt $constructed) {
        throw 'PHASE2_PUBLICATION requests_sent exceeds requests_constructed'
    }
    $ready = [uint64]$publication.stages.requests_ready
    $transportPending = [uint64]$publication.stages.requests_transport_pending
    if ($constructed -ne [uint64]::MaxValue -and $sent -ne [uint64]::MaxValue -and
        $ready -ne [uint64]::MaxValue -and $transportPending -ne [uint64]::MaxValue -and
        ([decimal]$ready + [decimal]$transportPending) -ne ([decimal]$constructed - [decimal]$sent)) {
        throw 'PHASE2_PUBLICATION ready and transport-pending gauges disagree with unsaturated request counters'
    }
    $changesQueued = [uint64]$publication.stages.mesh_changes_queued
    $changesDequeued = [uint64]$publication.stages.mesh_changes_dequeued
    if ($changesDequeued -gt $changesQueued) {
        throw 'PHASE2_PUBLICATION mesh_changes_dequeued exceeds mesh_changes_queued'
    }
    if ($changesQueued -ne [uint64]::MaxValue -and $changesDequeued -ne [uint64]::MaxValue -and
        [uint64]$publication.stages.mesh_changes_pending -ne ($changesQueued - $changesDequeued)) {
        throw 'PHASE2_PUBLICATION mesh_changes_pending disagrees with unsaturated change counters'
    }
    $outcomeTotal = [decimal]0
    foreach ($field in @('success', 'all_air', 'unavailable', 'malformed', 'stale', 'timed_out')) {
        $outcomeTotal += [decimal]$publication.outcomes.$field
    }
    $maximumCommittableOutcomes = [decimal]$publication.outcomes.success +
        [decimal]$publication.outcomes.all_air + [decimal]$publication.outcomes.unavailable
    if ($outcomeTotal -gt [decimal]$publication.stages.responses_admitted -or
        [decimal]$publication.stages.subchunks_committed -gt $maximumCommittableOutcomes) {
        throw 'PHASE2_PUBLICATION response outcomes exceed admitted or committable responses'
    }
    foreach ($timingName in @('max_queue_wait_us', 'max_worker_time_us')) {
        Assert-Phase2ExactProperties -Value $publication.$timingName -Names @('decode', 'lighting', 'meshing') `
            -Label "PHASE2_PUBLICATION $timingName"
        foreach ($field in @('decode', 'lighting', 'meshing')) {
            Assert-Phase2FiniteNonnegativeNumber -Value $publication.$timingName.$field `
                -Label "publication.$timingName.$field"
        }
    }
    foreach ($identityName in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
        $identity = $presentation.$identityName
        Assert-Phase2ExactProperties -Value $identity `
            -Names @('entry_count', 'generation_manifest_hash', 'required_cohort_hash', 'session_generation') `
            -Label "PHASE2_PUBLICATION presentation.$identityName"
        Assert-Phase2UnsignedInteger -Value $identity.entry_count -Label "presentation.$identityName.entry_count"
        Assert-Phase2UnsignedInteger -Value $identity.session_generation -Label "presentation.$identityName.session_generation" -Positive
        if ([uint64]$identity.session_generation -ne [uint64]$publication.session_generation -or
            [string]$identity.required_cohort_hash -cne [string]$publication.required_cohort_hash -or
            [string]$identity.generation_manifest_hash -notmatch '^[0-9a-f]{16}$') {
            throw "PHASE2_PUBLICATION contains incoherent $identityName identity"
        }
    }
    if ($publisherUninitialized) {
        if ([uint64]$publication.required_columns -ne 0 -or
            [uint64]$publication.loaded_required_columns -ne 0) {
            throw 'PHASE2_PUBLICATION uninitialized publisher contains a nonempty cohort'
        }
        foreach ($field in $requiredStageFields) {
            if ([uint64]$publication.stages.$field -ne 0) {
                throw 'PHASE2_PUBLICATION uninitialized publisher contains stage progress'
            }
        }
        foreach ($field in @('success', 'all_air', 'unavailable', 'malformed', 'stale', 'timed_out')) {
            if ([uint64]$publication.outcomes.$field -ne 0) {
                throw 'PHASE2_PUBLICATION uninitialized publisher contains response outcomes'
            }
        }
        foreach ($identityName in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
            if ([uint64]$presentation.$identityName.entry_count -ne 0) {
                throw 'PHASE2_PUBLICATION uninitialized publisher contains presented entries'
            }
        }
    }
}

function Get-Phase2FirstStalledStage {
    param(
        [Parameter(Mandatory = $true)]$PublicationRecord,
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved
    )

    $publication = $PublicationRecord.publication
    $stages = $publication.stages
    $presentation = $PublicationRecord.presentation
    $exactRawRadiusCount = Get-Phase2ExactCohortColumnCount `
        -PublisherRadiusBlocks ([uint64]$publication.publisher_radius_blocks)
    $cohortComplete = [uint64]$publication.loaded_required_columns -ge [uint64]$publication.required_columns
    $outcomeTotal = [decimal]0
    foreach ($field in @('success', 'all_air', 'unavailable', 'malformed', 'stale', 'timed_out')) {
        $outcomeTotal += [decimal]$publication.outcomes.$field
    }
    $committedOutcomeTotal = [decimal]$publication.outcomes.success + [decimal]$publication.outcomes.all_air
    if ($outcomeTotal -ne [decimal]$stages.responses_admitted -or
        $committedOutcomeTotal -ne [decimal]$stages.subchunks_committed) {
        return 'response_semantics'
    }
    if ([uint64]$publication.outcomes.malformed -gt 0 -or [uint64]$publication.outcomes.stale -gt 0 -or
        [uint64]$publication.outcomes.timed_out -gt 0 -or [uint64]$publication.outcomes.unavailable -gt 0) {
        return 'response_semantics'
    }
    if ([uint64]$publication.required_columns -ne $exactRawRadiusCount -and
        [uint64]$publication.loaded_required_columns -eq $exactRawRadiusCount -and
        [uint64]$stages.requests_constructed -eq $exactRawRadiusCount -and
        [uint64]$stages.requests_sent -eq $exactRawRadiusCount -and
        [uint64]$stages.requests_ready -eq 0 -and
        [uint64]$stages.subchunks_awaiting_response -eq 0 -and
        [uint64]$stages.responses_admitted -eq [uint64]$stages.subchunks_committed -and
        [uint64]$stages.subchunks_committed -gt 0) {
        return 'required_cohort_identity'
    }
    if (-not $cohortComplete -and
        [uint64]$stages.requests_constructed -eq [uint64]$publication.loaded_required_columns -and
        [uint64]$stages.requests_sent -eq [uint64]$stages.requests_constructed -and
        [uint64]$stages.subchunks_awaiting_response -eq 0 -and
        [uint64]$stages.responses_admitted -eq [uint64]$stages.subchunks_committed -and
        [uint64]$stages.subchunks_committed -gt 0) {
        return 'required_cohort_identity'
    }
    if ([uint64]$stages.requests_ready -gt 0 -or
        (-not $cohortComplete -and [uint64]$stages.requests_constructed -eq 0)) { return 'request_order' }
    if ([uint64]$stages.requests_sent -lt [uint64]$stages.requests_constructed) { return 'transport' }
    if ([uint64]$stages.subchunks_awaiting_response -gt 0 -or
        (-not $cohortComplete -and [uint64]$stages.responses_admitted -eq 0)) { return 'response_semantics' }
    if ([uint64]$stages.decode_jobs_completed -gt [uint64]$stages.decode_jobs_dispatched -or
        [uint64]$stages.decode_jobs_queued -gt 0 -or [uint64]$stages.decode_jobs_in_flight -gt 0 -or
        [uint64]$stages.decode_jobs_completed -lt [uint64]$stages.decode_jobs_dispatched) { return 'decode' }
    if ([uint64]$stages.light_jobs_completed -gt [uint64]$stages.light_jobs_dispatched -or
        [uint64]$stages.light_jobs_queued -gt 0 -or [uint64]$stages.light_jobs_in_flight -gt 0 -or
        [uint64]$stages.light_jobs_completed -lt [uint64]$stages.light_jobs_dispatched) { return 'lighting' }
    if ([uint64]$stages.mesh_changes_dequeued -gt [uint64]$stages.mesh_changes_queued -or
        ([uint64]$stages.mesh_changes_queued -ne [uint64]::MaxValue -and
            [uint64]$stages.mesh_changes_dequeued -ne [uint64]::MaxValue -and
            [uint64]$stages.mesh_changes_pending -ne
                ([uint64]$stages.mesh_changes_queued - [uint64]$stages.mesh_changes_dequeued)) -or
        [uint64]$stages.mesh_jobs_completed -gt [uint64]$stages.mesh_jobs_dispatched -or
        [uint64]$stages.mesh_changes_pending -gt 0 -or [uint64]$stages.mesh_jobs_queued -gt 0 -or
        [uint64]$stages.mesh_jobs_in_flight -gt 0 -or
        [uint64]$stages.mesh_jobs_completed -lt [uint64]$stages.mesh_jobs_dispatched) { return 'meshing' }
    if ([uint64]$stages.mesh_uploads_unacknowledged -gt 0) { return 'gpu_upload' }
    if (-not $cohortComplete) { return 'required_cohort_identity' }
    $publisher = $presentation.publisher_disk
    $resident = $presentation.resident
    $allocation = $presentation.allocation
    $visible = $presentation.visible
    $submitted = $presentation.submitted
    $presented = $presentation.gpu_presented
    if ([uint64]$resident.entry_count -ne [uint64]$publisher.entry_count -or
        [string]$resident.generation_manifest_hash -cne [string]$publisher.generation_manifest_hash) { return 'main_apply' }
    if ([uint64]$allocation.entry_count -ne [uint64]$publisher.entry_count -or
        [string]$allocation.generation_manifest_hash -cne [string]$publisher.generation_manifest_hash) { return 'gpu_upload' }
    if ([uint64]$visible.entry_count -ne [uint64]$publisher.entry_count -or
        [string]$visible.generation_manifest_hash -cne [string]$publisher.generation_manifest_hash) { return 'extraction' }
    if ([uint64]$submitted.entry_count -ne [uint64]$visible.entry_count -or
        [string]$submitted.generation_manifest_hash -cne [string]$visible.generation_manifest_hash) { return 'submission' }
    if ([uint64]$presented.entry_count -ne [uint64]$submitted.entry_count -or
        [string]$presented.generation_manifest_hash -cne [string]$submitted.generation_manifest_hash) { return 'presentation' }
    if ($WorldReadyObserved) { return 'none' }
    return 'presentation'
}

function Get-Phase2PublicationSequenceEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved,
        [Parameter(Mandatory = $true)][ValidateSet('Lunar', 'Zeqa')][string]$Server
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
    $publisherInitialized = $false
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
        if ($publisherUninitialized -and $publisherInitialized) {
            throw 'PHASE2_PUBLICATION publisher became uninitialized during the diagnostic capture'
        }
        Assert-Phase2PublicationRecord -Record $record -ExpectedPresentMode $ExpectedPresentMode `
            -ExpectedGraphicsIdentity $graphicsIdentity -ExpectedAssetsIdentity $assetsIdentity `
            -AllowUninitializedPublisher:$publisherUninitialized
        if ($null -eq $graphicsIdentity) {
            $graphicsIdentity = [string]$record.presentation.graphics_identity_sha256
            $assetsIdentity = [string]$record.presentation.assets_manifest_sha256
        }
        if (-not $publisherUninitialized) {
            $publisherInitialized = $true
            $currentSequenceIdentity = [pscustomobject][ordered]@{
                session_generation = [uint64]$record.publication.session_generation
                required_cohort_hash = [string]$record.publication.required_cohort_hash
                required_columns = [uint64]$record.publication.required_columns
                dimension = [int32]$record.publication.player_column.dimension
                player_x = [int32]$record.publication.player_column.x
                player_z = [int32]$record.publication.player_column.z
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
            if ($null -eq $sequenceIdentity) {
                $sequenceIdentity = $currentSequenceIdentity
            }
            elseif ($currentSequenceIdentity -cne $sequenceIdentity) {
                throw 'PHASE2_PUBLICATION sequence identity changed during the diagnostic capture'
            }
        }
        if ($null -ne $previousRecord) {
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

function Complete-Phase2DiagnosticEvidence {
    param(
        [Parameter(Mandatory = $true)]$Manifest,
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved,
        [Parameter(Mandatory = $true)][ValidateSet('Lunar', 'Zeqa')][string]$Server,
        [Nullable[double]]$JoinMilliseconds
    )

    if ([string]$Manifest.mode -cne 'Diagnostic') {
        throw 'diagnostic evidence completion is valid only for Diagnostic mode'
    }
    $evidence = Get-Phase2PublicationSequenceEvidence -ClientLogPath $ClientLogPath `
        -ExpectedPresentMode $ExpectedPresentMode -WorldReadyObserved:$WorldReadyObserved -Server $Server
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
        'auth_cache_scope', 'behavior_gate_passed', 'client_arguments', 'client_blob_cache_route',
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
            Assert-Phase2ExactProperties -Value $candidate.performance -Names $performanceProperties `
                -Label 'Phase 2 remote manifest performance'
            Assert-Phase2UnsignedInteger -Value $candidate.initial_radius -Label 'manifest.initial_radius' -Maximum 64 -Positive
            Assert-Phase2UnsignedInteger -Value $candidate.publication_snapshot_count `
                -Label 'manifest.publication_snapshot_count' -Maximum ([uint32]::MaxValue) -Positive
            Assert-Phase2UnsignedInteger -Value $candidate.duration_seconds -Label 'manifest.duration_seconds' `
                -Maximum ([uint32]::MaxValue) -Positive
            Assert-Phase2UnsignedInteger -Value $candidate.client_shutdown_grace_seconds `
                -Label 'manifest.client_shutdown_grace_seconds' -Maximum ([uint32]::MaxValue)
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

function Assert-Phase2Evidence {
    param(
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][string]$ResourcesPath,
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [Parameter(Mandatory = $true)][double]$JoinMilliseconds,
        [switch]$RequireFullView
    )

    $metrics = Get-Content -Raw -LiteralPath $MetricsPath | ConvertFrom-Json
    $frameLimits = [ordered]@{
        p95_frame_ms = 16.6666666667
        p99_frame_ms = 16.6666666667
        max_frame_ms = 50.0
    }
    foreach ($field in $frameLimits.Keys) {
        if ($null -eq $metrics.PSObject.Properties[$field]) {
            throw "Phase 2 metrics are missing $field"
        }
        $value = $metrics.$field
        if ($null -eq $value -or $value -is [string] -or $value -is [bool]) {
            throw "Phase 2 frame gate contains a nonnumeric $field"
        }
        $number = [double]$value
        if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or
            $number -lt 0.0 -or $number -gt [double]$frameLimits[$field]) {
            throw "Phase 2 frame gate failed for $field`: $number"
        }
    }
    if ([double]::IsNaN($JoinMilliseconds) -or [double]::IsInfinity($JoinMilliseconds) -or
        $JoinMilliseconds -lt 0.0 -or $JoinMilliseconds -gt 2000.0) {
        throw "Phase 2 join gate failed: $JoinMilliseconds ms"
    }
    if ($RequireFullView) {
        foreach ($field in @('teleport_settle_ms', 'forced_full_view_remesh_ms')) {
            if ($null -eq $metrics.PSObject.Properties[$field] -or $null -eq $metrics.$field -or
                [double]$metrics.$field -gt 2000.0) {
                throw "Phase 2 full-view lifecycle gate failed for $field"
            }
        }
    }

    $resources = Get-Content -Raw -LiteralPath $ResourcesPath | ConvertFrom-Json
    $samples = @($resources.samples)
    if ([string]$resources.schema -cne 'rust-mcbe-phase2-resources-v1' -or
        [int]$resources.warmup_seconds -ne 30 -or
        [int]$resources.duration_seconds -ne 120 -or
        [int]$resources.processor_count -le 0 -or
        $samples.Count -ne 120 -or
        [int]$resources.summary.sample_count -ne 120) {
        throw 'Phase 2 resource evidence must contain a 30-second warmup and exactly 120 one-second samples'
    }
    $previous = 0.0
    $rss = [Collections.Generic.List[uint64]]::new()
    $cpu = [Collections.Generic.List[double]]::new()
    foreach ($sample in $samples) {
        $elapsed = [double]$sample.elapsed_seconds
        $delta = $elapsed - $previous
        if ($delta -lt 0.5 -or $delta -gt 1.5) {
            throw "Phase 2 resource cadence was not one second: delta=$delta"
        }
        $rss.Add([uint64]$sample.combined_rss_bytes)
        $cpu.Add([double]$sample.cpu_percent)
        $previous = $elapsed
    }
    $maxRss = [uint64](($rss | Measure-Object -Maximum).Maximum)
    $meanCpu = [double](($cpu | Measure-Object -Average).Average)
    $sortedCpu = @($cpu | Sort-Object)
    $p95Cpu = [double]$sortedCpu[[Math]::Ceiling(($sortedCpu.Count - 1) * 0.95)]
    if ($maxRss -gt 650MB -or $meanCpu -gt 15.0 -or $p95Cpu -gt 15.0) {
        throw "Phase 2 resource gate failed: rss=$maxRss mean_cpu=$meanCpu p95_cpu=$p95Cpu"
    }
    if ([uint64]$resources.summary.max_combined_rss_bytes -ne $maxRss -or
        [Math]::Abs([double]$resources.summary.mean_cpu_percent - $meanCpu) -gt 0.000001 -or
        [Math]::Abs([double]$resources.summary.p95_cpu_percent - $p95Cpu) -gt 0.000001) {
        throw 'Phase 2 resource summary does not match its samples'
    }

    $publicationLines = @(Get-Content -LiteralPath $ClientLogPath | Where-Object {
        $_.StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
    })
    if ($publicationLines.Count -eq 0) {
        throw 'client log contains no PHASE2_PUBLICATION evidence'
    }
    $publication = $publicationLines[-1].Substring('PHASE2_PUBLICATION='.Length) | ConvertFrom-Json
    Assert-Phase2PublicationRecord -Record $publication -ExpectedPresentMode $ExpectedPresentMode
    $presentation = $publication.presentation
    if ([string]$presentation.build_profile -cne 'release' -or
        [string]$presentation.requested_present_mode -cne $ExpectedPresentMode.ToLowerInvariant() -or
        [string]$presentation.effective_present_mode -cne $ExpectedPresentMode.ToLowerInvariant() -or
        $presentation.present_mode_proven -isnot [bool] -or
        -not [bool]$presentation.present_mode_proven -or
        [string]$presentation.graphics_identity_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$presentation.assets_manifest_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'PHASE2_PUBLICATION did not prove release build, effective present mode, graphics identity, and asset identity'
    }
    $publicationText = $publicationLines[-1].ToLowerInvariant()
    foreach ($forbidden in @('"path"', '"token"', '"auth"', '"payload"', '"credential"')) {
        if ($publicationText.Contains($forbidden)) {
            throw "PHASE2_PUBLICATION leaked forbidden field $forbidden"
        }
    }
    return [pscustomobject][ordered]@{
        metrics = $metrics
        resources = $resources
        publication = $publication
        join_milliseconds = $JoinMilliseconds
    }
}

function Measure-Phase2Resources {
    param(
        [Parameter(Mandatory = $true)]$ClientHandle,
        [Parameter(Mandatory = $true)]$CoreHandle,
        [Parameter(Mandatory = $true)][string]$OutputPath
    )

    $client = $ClientHandle.Process
    $core = $CoreHandle.Process
    $client.Refresh()
    $core.Refresh()
    $previousCpu = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
    $previousWall = 0.0
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $samples = [Collections.Generic.List[object]]::new()
    for ($second = 1; $second -le 150; $second++) {
        Start-Sleep -Seconds 1
        if ($client.HasExited -or $core.HasExited) {
            throw 'client or core exited before the Phase 2 performance window completed'
        }
        $client.Refresh()
        $core.Refresh()
        $wall = $stopwatch.Elapsed.TotalSeconds
        $cpuTotal = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
        $wallDelta = $wall - $previousWall
        $cpuDelta = $cpuTotal - $previousCpu
        if ($second -gt 30) {
            $samples.Add([pscustomobject][ordered]@{
                elapsed_seconds = $wall - 30.0
                combined_rss_bytes = [uint64]($client.WorkingSet64 + $core.WorkingSet64)
                cpu_percent = [Math]::Max(0.0, 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount))
            })
        }
        $previousWall = $wall
        $previousCpu = $cpuTotal
    }
    $stopwatch.Stop()
    $rss = @($samples | ForEach-Object { [uint64]$_.combined_rss_bytes })
    $cpu = @($samples | ForEach-Object { [double]$_.cpu_percent } | Sort-Object)
    $document = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-phase2-resources-v1'
        warmup_seconds = 30
        duration_seconds = 120
        processor_count = [Environment]::ProcessorCount
        samples = @($samples)
        summary = [pscustomobject][ordered]@{
            sample_count = $samples.Count
            max_combined_rss_bytes = [uint64](($rss | Measure-Object -Maximum).Maximum)
            mean_cpu_percent = [double](($cpu | Measure-Object -Average).Average)
            p95_cpu_percent = [double]$cpu[[Math]::Ceiling(($cpu.Count - 1) * 0.95)]
        }
    }
    Write-Phase2Json -Path $OutputPath -Value $document
    return $document
}
