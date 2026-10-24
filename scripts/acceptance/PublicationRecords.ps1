# Get-CacheBoundaryEvidence validates historical boundary markers and reports
# missing instrumentation without inventing packet counts for current builds.
function Get-CacheBoundaryEvidence {
    param([Parameter(Mandatory = $true)][string]$SourceLogPath)

    if (-not (Test-Path -LiteralPath $SourceLogPath -PathType Leaf)) {
        throw 'BLOB_CACHE_BOUNDARY source log is missing'
    }
    $content = Get-Content -LiteralPath $SourceLogPath -Raw
    $markerPattern = '(?:^|\s)msg=BLOB_CACHE_BOUNDARY(?:\s|$)'
    $markerCount = [regex]::Matches(
        [string]$content,
        $markerPattern,
        [Text.RegularExpressions.RegexOptions]::CultureInvariant -bor
            [Text.RegularExpressions.RegexOptions]::Multiline
    ).Count
    if ($markerCount -eq 0) {
        return [pscustomobject][ordered]@{
            classification = 'unavailable'
            reason = 'boundary_marker_not_recorded'
        }
    }
    if ($markerCount -ne 1) {
        throw 'BLOB_CACHE_BOUNDARY requires exactly one summary marker'
    }
    $lines = @(
        Get-Content -LiteralPath $SourceLogPath |
            Where-Object { $_ -match $markerPattern }
    )
    if ($lines.Count -ne 1) {
        throw 'BLOB_CACHE_BOUNDARY requires exactly one summary marker'
    }
    $pattern = '^(?:.*\s)?msg=BLOB_CACHE_BOUNDARY ' +
        'upstream_status_seen=(true|false) upstream_status_enabled=(true|false) ' +
        'cached_level_chunks=([0-9]+) ordinary_level_chunks=([0-9]+) ' +
        'cached_sub_chunks=([0-9]+) ordinary_sub_chunks=([0-9]+)$'
    $match = [regex]::Match([string]$lines[0], $pattern, [Text.RegularExpressions.RegexOptions]::CultureInvariant)
    if (-not $match.Success) {
        throw 'BLOB_CACHE_BOUNDARY summary marker is malformed'
    }
    $seen = $match.Groups[1].Value -ceq 'true'
    $enabled = $match.Groups[2].Value -ceq 'true'
    $values = [Collections.Generic.List[uint64]]::new()
    for ($index = 3; $index -le 6; $index++) {
        [uint64]$value = 0
        if (-not [uint64]::TryParse(
            $match.Groups[$index].Value,
            [Globalization.NumberStyles]::None,
            [Globalization.CultureInfo]::InvariantCulture,
            [ref]$value
        )) {
            throw 'BLOB_CACHE_BOUNDARY counter is outside its unsigned bound'
        }
        $values.Add($value)
    }
    $cachedLevel = $values[0]
    $ordinaryLevel = $values[1]
    $cachedSub = $values[2]
    $ordinarySub = $values[3]
    $cachedPackets = [decimal]$cachedLevel + [decimal]$cachedSub
    $ordinaryPackets = [decimal]$ordinaryLevel + [decimal]$ordinarySub
    if ((-not $seen -and $enabled) -or (-not $enabled -and $cachedPackets -ne 0)) {
        throw 'BLOB_CACHE_BOUNDARY status and packet routes are incoherent'
    }
    $classification = if (-not $seen -or -not $enabled) {
        'negotiation_failure'
    }
    elseif ($cachedPackets -ne 0) {
        'cache_backed'
    }
    elseif ($ordinaryPackets -ne 0) {
        'server_ordinary_despite_cache_capability'
    }
    else {
        throw 'BLOB_CACHE_BOUNDARY contains no attributable world packet route'
    }
    return [pscustomobject][ordered]@{
        classification = $classification
        upstream_status_seen = $seen
        upstream_status_enabled = $enabled
        cached_level_chunks = $cachedLevel
        ordinary_level_chunks = $ordinaryLevel
        cached_sub_chunks = $cachedSub
        ordinary_sub_chunks = $ordinarySub
    }
}

function Assert-CacheBoundaryConsistency {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('Lunar', 'Zeqa')][string]$Server,
        [Parameter(Mandatory = $true)][string]$ClientBlobCacheRoute,
        [Parameter(Mandatory = $true)][AllowNull()]$BoundaryEvidence
    )

    if ($null -eq $BoundaryEvidence -or [string]$BoundaryEvidence.classification -cnotin @(
        'cache_backed', 'negotiation_failure', 'server_ordinary_despite_cache_capability'
    )) {
        throw 'Cache boundary evidence is unavailable or invalid; binding acceptance requires independent packet-route evidence'
    }
    $cachedRoutes = [uint64]$BoundaryEvidence.cached_level_chunks +
        [uint64]$BoundaryEvidence.cached_sub_chunks
    $boundaryCacheBacked = [string]$BoundaryEvidence.classification -ceq 'cache_backed' -and
        $BoundaryEvidence.upstream_status_seen -is [bool] -and
        [bool]$BoundaryEvidence.upstream_status_seen -and
        $BoundaryEvidence.upstream_status_enabled -is [bool] -and
        [bool]$BoundaryEvidence.upstream_status_enabled -and
        $cachedRoutes -gt 0
    if ($Server -ceq 'Lunar' -and
        ($ClientBlobCacheRoute -cne 'cache_backed' -or -not $boundaryCacheBacked)) {
        throw 'Lunar acceptance requires coherent cache-backed publication and independent boundary evidence'
    }
    if ($Server -ceq 'Zeqa') {
        if ($ClientBlobCacheRoute -cnotin @('cache_backed', 'ordinary_payload')) {
            throw 'Zeqa acceptance contains an unknown client blob cache route'
        }
        if (($ClientBlobCacheRoute -ceq 'cache_backed') -ne $boundaryCacheBacked) {
            throw 'Zeqa acceptance client and independent cache boundary routes disagree'
        }
    }
}

function Assert-PublicationRecord {
    param(
        [Parameter(Mandatory = $true)]$Record,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [ValidateSet('debug', 'release')][string]$ExpectedBuildProfile = 'release',
        [string]$ExpectedGraphicsIdentity,
        [string]$ExpectedAssetsIdentity,
        [switch]$AllowUninitializedPublisher,
        [switch]$AllowResettingPublisher
    )

    Assert-AcceptanceExactProperties -Value $Record `
        -Names @('client_blob_cache', 'client_blob_cache_enabled', 'presentation', 'publication') `
        -Label 'CINNABAR_CHUNK_PUBLICATION root'
    if ($Record.client_blob_cache_enabled -isnot [bool]) {
        throw 'CINNABAR_CHUNK_PUBLICATION client_blob_cache_enabled must be a Boolean'
    }
    $cache = $Record.client_blob_cache
    $requiredCacheFields = @(
        'admitted_blobs', 'evictions', 'hashes_classified', 'hits', 'misses', 'pending_bytes',
        'pending_resets', 'pending_transactions', 'reconstructed_level_chunks',
        'reconstructed_sub_chunks', 'rejected_blobs'
    )
    Assert-AcceptanceExactProperties -Value $cache -Names $requiredCacheFields `
        -Label 'CINNABAR_CHUNK_PUBLICATION client_blob_cache'
    foreach ($field in $requiredCacheFields) {
        Assert-AcceptanceUnsignedInteger -Value $cache.$field -Label "client_blob_cache.$field"
    }
    if ([decimal]$cache.hits + [decimal]$cache.misses -ne [decimal]$cache.hashes_classified) {
        throw 'CINNABAR_CHUNK_PUBLICATION client blob cache hit and miss totals disagree with hashes_classified'
    }
    if (-not [bool]$Record.client_blob_cache_enabled) {
        foreach ($field in $requiredCacheFields) {
            if ([uint64]$cache.$field -ne 0) {
                throw 'CINNABAR_CHUNK_PUBLICATION disabled client blob cache contains nonzero counters'
            }
        }
    }
    $presentation = $Record.presentation
    $publication = $Record.publication
    Assert-AcceptanceExactProperties -Value $presentation -Names @(
        'allocation', 'assets_manifest_sha256', 'build_profile', 'effective_present_mode', 'gpu_presented',
        'graphics_identity_sha256', 'present_mode_proven', 'publisher_disk', 'requested_present_mode',
        'resident', 'submitted', 'visible', 'visible_subset_of_resident', 'player_column'
    ) -Label 'CINNABAR_CHUNK_PUBLICATION presentation'
    Assert-AcceptanceExactProperties -Value $publication -Names @(
        'inactive_level_chunks', 'loaded_required_columns', 'local_reset', 'max_queue_wait_us',
        'max_worker_time_us', 'outcomes', 'player_column', 'player_column_loaded',
        'player_column_required', 'publisher_center', 'publisher_epoch', 'publisher_radius_blocks',
        'publisher_radius_chunks', 'request_queue', 'required_cohort_hash', 'required_cohort_stable',
        'required_columns', 'session_generation', 'stages'
    ) -Label 'CINNABAR_CHUNK_PUBLICATION publication'
    $mode = $ExpectedPresentMode.ToLowerInvariant()
    if ($null -eq $presentation -or $null -eq $publication -or
        [string]$presentation.build_profile -cne $ExpectedBuildProfile -or
        [string]$presentation.requested_present_mode -cne $mode -or
        [string]$presentation.effective_present_mode -cne $mode -or
        $presentation.present_mode_proven -isnot [bool] -or
        -not [bool]$presentation.present_mode_proven -or
        [string]$presentation.graphics_identity_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$presentation.assets_manifest_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'CINNABAR_CHUNK_PUBLICATION did not prove expected build, effective present mode, graphics identity, and asset identity'
    }
    if ($presentation.visible_subset_of_resident -isnot [bool]) {
        throw 'CINNABAR_CHUNK_PUBLICATION visible_subset_of_resident must be a Boolean'
    }
    if (-not [string]::IsNullOrEmpty($ExpectedGraphicsIdentity) -and
        [string]$presentation.graphics_identity_sha256 -cne $ExpectedGraphicsIdentity) {
        throw 'CINNABAR_CHUNK_PUBLICATION graphics identity changed during the diagnostic sequence'
    }
    if (-not [string]::IsNullOrEmpty($ExpectedAssetsIdentity) -and
        [string]$presentation.assets_manifest_sha256 -cne $ExpectedAssetsIdentity) {
        throw 'CINNABAR_CHUNK_PUBLICATION assets identity changed during the diagnostic sequence'
    }
    Assert-AcceptanceUnsignedInteger -Value $publication.session_generation -Label 'publication.session_generation' -Positive
    Assert-AcceptanceUnsignedInteger -Value $publication.publisher_epoch -Label 'publication.publisher_epoch'
    Assert-AcceptanceUnsignedInteger -Value $publication.required_columns -Label 'publication.required_columns'
    Assert-AcceptanceUnsignedInteger -Value $publication.loaded_required_columns -Label 'publication.loaded_required_columns'
    Assert-AcceptanceUnsignedInteger -Value $publication.inactive_level_chunks -Label 'publication.inactive_level_chunks'
    foreach ($field in @('player_column_required', 'player_column_loaded')) {
        if ($publication.$field -isnot [bool]) { throw "publication.$field must be a Boolean" }
    }
    if ($publication.required_cohort_stable -isnot [bool]) {
        throw 'publication.required_cohort_stable must be a Boolean'
    }
    $publisherUninitialized = $null -eq $publication.publisher_radius_blocks -and
        $null -eq $publication.publisher_radius_chunks
    if ($publisherUninitialized) {
        if (-not $AllowUninitializedPublisher) {
            throw 'publication.publisher_radius_blocks must be an exact integral JSON number'
        }
    }
    else {
        Assert-AcceptanceUnsignedInteger -Value $publication.publisher_radius_blocks -Label 'publication.publisher_radius_blocks' -Maximum 1024 -Positive
        Assert-AcceptanceUnsignedInteger -Value $publication.publisher_radius_chunks -Label 'publication.publisher_radius_chunks' -Maximum 64 -Positive
        $derivedRetentionRadius = [uint64][Math]::Ceiling([uint64]$publication.publisher_radius_blocks / 16.0)
        if ([uint64]$publication.publisher_radius_chunks -ne $derivedRetentionRadius) {
            throw 'CINNABAR_CHUNK_PUBLICATION raw block radius disagrees with its derived retention radius'
        }
    }
    if ($null -eq $publication.stages -or $null -eq $publication.outcomes -or
        [string]$publication.required_cohort_hash -notmatch '^[0-9a-f]{16}$' -or
        [uint64]$publication.loaded_required_columns -gt [uint64]$publication.required_columns) {
        throw 'CINNABAR_CHUNK_PUBLICATION lacks a coherent publication identity and bounded cohort counts'
    }
    Assert-AcceptanceExactProperties -Value $publication.player_column -Names @('dimension', 'x', 'z') `
        -Label 'CINNABAR_CHUNK_PUBLICATION player_column'
    foreach ($field in @('dimension', 'x', 'z')) {
        Assert-AcceptanceSignedInteger32 -Value $publication.player_column.$field -Label "publication.player_column.$field"
    }
    if ($null -ne $publication.publisher_center) {
        if (@($publication.publisher_center).Count -ne 3) {
            throw 'publication.publisher_center must contain exactly three coordinates'
        }
        for ($index = 0; $index -lt 3; $index++) {
            Assert-AcceptanceSignedInteger32 -Value $publication.publisher_center[$index] `
                -Label "publication.publisher_center[$index]"
        }
    }
    elseif (-not $publisherUninitialized) {
        throw 'publication.publisher_center is missing for an initialized publisher'
    }
    $localReset = $publication.local_reset
    Assert-AcceptanceExactProperties -Value $localReset -Names @(
        'armed', 'armed_count', 'consumed_count', 'dispatch_classes', 'dispatch_count',
        'dispatch_total', 'dispatch_trace_overflowed'
    ) -Label 'CINNABAR_CHUNK_PUBLICATION publication.local_reset'
    if ($localReset.armed -isnot [bool] -or $localReset.dispatch_trace_overflowed -isnot [bool]) {
        throw 'publication.local_reset Boolean evidence is malformed'
    }
    foreach ($field in @('armed_count', 'consumed_count', 'dispatch_count', 'dispatch_total')) {
        Assert-AcceptanceUnsignedInteger -Value $localReset.$field -Label "publication.local_reset.$field"
    }
    $dispatchClasses = @($localReset.dispatch_classes)
    if ($dispatchClasses.Count -ne [uint64]$localReset.dispatch_count -or
        [uint64]$localReset.dispatch_count -gt 16 -or
        [uint64]$localReset.dispatch_total -lt [uint64]$localReset.dispatch_count -or
        [bool]$localReset.dispatch_trace_overflowed -ne
            ([uint64]$localReset.dispatch_total -gt [uint64]$localReset.dispatch_count) -or
        [uint64]$localReset.consumed_count -gt [uint64]$localReset.armed_count) {
        throw 'publication.local_reset counters or bounded trace are incoherent'
    }
    $requestClasses = @('player_retry', 'player_initial', 'visible_retry', 'visible_initial', 'prefetch_retry', 'prefetch_initial')
    foreach ($class in $dispatchClasses) {
        if ([string]$class -cnotin $requestClasses) { throw 'publication.local_reset contains an unknown dispatch class' }
    }
    $queue = $publication.request_queue
    Assert-AcceptanceExactProperties -Value $queue -Names @(
        'class_depths', 'next_class', 'next_is_starved', 'next_is_transport_retry',
        'ready_blocked_by_reservation', 'reservations'
    ) -Label 'CINNABAR_CHUNK_PUBLICATION publication.request_queue'
    if ($queue.next_is_starved -isnot [bool] -or $queue.next_is_transport_retry -isnot [bool]) {
        throw 'publication.request_queue selection reasons must be Boolean'
    }
    foreach ($field in @('ready_blocked_by_reservation', 'reservations')) {
        Assert-AcceptanceUnsignedInteger -Value $queue.$field -Label "publication.request_queue.$field"
    }
    $depths = @($queue.class_depths)
    if ($depths.Count -ne $requestClasses.Count) { throw 'publication.request_queue class depth count is not exact' }
    for ($index = 0; $index -lt $requestClasses.Count; $index++) {
        Assert-AcceptanceExactProperties -Value $depths[$index] -Names @('class', 'eligible', 'ready') `
            -Label "publication.request_queue.class_depths[$index]"
        if ([string]$depths[$index].class -cne $requestClasses[$index]) {
            throw 'publication.request_queue priority order changed'
        }
        foreach ($field in @('eligible', 'ready')) {
            Assert-AcceptanceUnsignedInteger -Value $depths[$index].$field `
                -Label "publication.request_queue.class_depths[$index].$field"
        }
        if ([uint64]$depths[$index].eligible -gt [uint64]$depths[$index].ready) {
            throw 'publication.request_queue eligible depth exceeds ready depth'
        }
    }
    if ($null -ne $queue.next_class -and [string]$queue.next_class -cnotin $requestClasses) {
        throw 'publication.request_queue next class is unknown'
    }
    [decimal]$readyDepth = 0
    [decimal]$eligibleDepth = 0
    foreach ($depth in $depths) {
        $readyDepth += [decimal]$depth.ready
        $eligibleDepth += [decimal]$depth.eligible
    }
    if ($readyDepth -ne [decimal]$publication.stages.requests_ready -or
        ($readyDepth - $eligibleDepth) -ne [decimal]$queue.ready_blocked_by_reservation) {
        throw 'publication.request_queue depth sums disagree with publication stage gauges'
    }
    if (($eligibleDepth -eq 0 -and $null -ne $queue.next_class) -or
        ($eligibleDepth -gt 0 -and $null -eq $queue.next_class) -or
        ([bool]$queue.next_is_starved -and [bool]$queue.next_is_transport_retry)) {
        throw 'publication.request_queue next selection is incoherent with eligible work'
    }
    if ($null -ne $queue.next_class) {
        $selectedDepth = @($depths | Where-Object { [string]$_.class -ceq [string]$queue.next_class })[0]
        if ([uint64]$selectedDepth.eligible -eq 0) {
            throw 'publication.request_queue selected class has no eligible work'
        }
    }
    $presentedPlayer = $presentation.player_column
    Assert-AcceptanceExactProperties -Value $presentedPlayer -Names @(
        'allocated_subchunks', 'dimension', 'gpu_presented_subchunks', 'resident_subchunks',
        'submitted_subchunks', 'visible_subchunks', 'x', 'z'
    ) -Label 'CINNABAR_CHUNK_PUBLICATION presentation.player_column'
    foreach ($field in @('dimension', 'x', 'z')) {
        Assert-AcceptanceSignedInteger32 -Value $presentedPlayer.$field -Label "presentation.player_column.$field"
        if ([int32]$presentedPlayer.$field -ne [int32]$publication.player_column.$field) {
            throw 'CINNABAR_CHUNK_PUBLICATION publication and presentation player columns disagree'
        }
    }
    Assert-AcceptanceUnsignedInteger -Value $presentedPlayer.allocated_subchunks `
        -Label 'presentation.player_column.allocated_subchunks'
    foreach ($field in @('resident_subchunks', 'visible_subchunks', 'submitted_subchunks', 'gpu_presented_subchunks')) {
        if ($null -ne $presentedPlayer.$field) {
            Assert-AcceptanceUnsignedInteger -Value $presentedPlayer.$field -Label "presentation.player_column.$field"
        }
    }
    if (($null -ne $presentedPlayer.visible_subchunks -and
            [uint64]$presentedPlayer.visible_subchunks -gt [uint64]$presentedPlayer.resident_subchunks) -or
        ($null -ne $presentedPlayer.submitted_subchunks -and
            $null -ne $presentedPlayer.visible_subchunks -and
            [uint64]$presentedPlayer.submitted_subchunks -gt [uint64]$presentedPlayer.visible_subchunks) -or
        ($null -ne $presentedPlayer.gpu_presented_subchunks -and
            $null -ne $presentedPlayer.submitted_subchunks -and
            [uint64]$presentedPlayer.gpu_presented_subchunks -gt [uint64]$presentedPlayer.submitted_subchunks)) {
        throw 'presentation.player_column stage counts violate resident/visible/submitted/GPU ordering'
    }
    Assert-AcceptanceExactProperties -Value $publication.outcomes `
        -Names @('all_air', 'malformed', 'stale', 'success', 'timed_out', 'unavailable') `
        -Label 'CINNABAR_CHUNK_PUBLICATION outcomes'
    foreach ($field in @('all_air', 'malformed', 'stale', 'success', 'timed_out', 'unavailable')) {
        Assert-AcceptanceUnsignedInteger -Value $publication.outcomes.$field -Label "publication.outcomes.$field"
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
    Assert-AcceptanceExactProperties -Value $publication.stages -Names $requiredStageFields `
        -Label 'CINNABAR_CHUNK_PUBLICATION stages'
    foreach ($field in $requiredStageFields) {
        Assert-AcceptanceUnsignedInteger -Value $publication.stages.$field -Label "publication.stages.$field"
    }
    foreach ($prefix in @('decode_jobs', 'light_jobs', 'mesh_jobs')) {
        $dispatched = [uint64]$publication.stages.("${prefix}_dispatched")
        $completed = [uint64]$publication.stages.("${prefix}_completed")
        $inFlight = [uint64]$publication.stages.("${prefix}_in_flight")
        if ($completed -gt $dispatched) {
            throw "CINNABAR_CHUNK_PUBLICATION ${prefix}_completed exceeds ${prefix}_dispatched"
        }
        if ($dispatched -ne [uint64]::MaxValue -and $completed -ne [uint64]::MaxValue -and
            $inFlight -ne ($dispatched - $completed)) {
            throw "CINNABAR_CHUNK_PUBLICATION ${prefix}_in_flight disagrees with unsaturated dispatch counters"
        }
    }
    $constructed = [uint64]$publication.stages.requests_constructed
    $sent = [uint64]$publication.stages.requests_sent
    if ($sent -gt $constructed) {
        throw 'CINNABAR_CHUNK_PUBLICATION requests_sent exceeds requests_constructed'
    }
    $ready = [uint64]$publication.stages.requests_ready
    $transportPending = [uint64]$publication.stages.requests_transport_pending
    if ($constructed -ne [uint64]::MaxValue -and $sent -ne [uint64]::MaxValue -and
        $ready -ne [uint64]::MaxValue -and $transportPending -ne [uint64]::MaxValue -and
        ([decimal]$ready + [decimal]$transportPending) -gt ([decimal]$constructed - [decimal]$sent)) {
        throw 'CINNABAR_CHUNK_PUBLICATION ready and transport-pending gauges exceed unsaturated request counters'
    }
    $changesQueued = [uint64]$publication.stages.mesh_changes_queued
    $changesDequeued = [uint64]$publication.stages.mesh_changes_dequeued
    if ($changesDequeued -gt $changesQueued) {
        throw 'CINNABAR_CHUNK_PUBLICATION mesh_changes_dequeued exceeds mesh_changes_queued'
    }
    if ($changesQueued -ne [uint64]::MaxValue -and $changesDequeued -ne [uint64]::MaxValue -and
        [uint64]$publication.stages.mesh_changes_pending -ne ($changesQueued - $changesDequeued)) {
        throw 'CINNABAR_CHUNK_PUBLICATION mesh_changes_pending disagrees with unsaturated change counters'
    }
    $outcomeTotal = [decimal]0
    foreach ($field in @('success', 'all_air', 'unavailable', 'malformed', 'stale', 'timed_out')) {
        $outcomeTotal += [decimal]$publication.outcomes.$field
    }
    $maximumCommittableOutcomes = [decimal]$publication.outcomes.success +
        [decimal]$publication.outcomes.all_air + [decimal]$publication.outcomes.unavailable
    if ($outcomeTotal -gt [decimal]$publication.stages.responses_admitted -or
        [decimal]$publication.stages.subchunks_committed -gt $maximumCommittableOutcomes) {
        throw 'CINNABAR_CHUNK_PUBLICATION response outcomes exceed admitted or committable responses'
    }
    foreach ($timingName in @('max_queue_wait_us', 'max_worker_time_us')) {
        Assert-AcceptanceExactProperties -Value $publication.$timingName -Names @('decode', 'lighting', 'meshing') `
            -Label "CINNABAR_CHUNK_PUBLICATION $timingName"
        foreach ($field in @('decode', 'lighting', 'meshing')) {
            Assert-AcceptanceFiniteNonnegativeNumber -Value $publication.$timingName.$field `
                -Label "publication.$timingName.$field"
        }
    }
    foreach ($identityName in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
        $identity = $presentation.$identityName
        Assert-AcceptanceExactProperties -Value $identity `
            -Names @('entry_count', 'generation_manifest_hash', 'manifest_domain', 'publisher_epoch', 'required_cohort_count',
                'required_cohort_hash', 'session_generation') `
            -Label "CINNABAR_CHUNK_PUBLICATION presentation.$identityName"
        Assert-AcceptanceUnsignedInteger -Value $identity.entry_count -Label "presentation.$identityName.entry_count"
        Assert-AcceptanceUnsignedInteger -Value $identity.session_generation -Label "presentation.$identityName.session_generation" -Positive
        Assert-AcceptanceUnsignedInteger -Value $identity.publisher_epoch -Label "presentation.$identityName.publisher_epoch"
        Assert-AcceptanceUnsignedInteger -Value $identity.required_cohort_count -Label "presentation.$identityName.required_cohort_count"
        if ([uint64]$identity.session_generation -ne [uint64]$publication.session_generation -or
            [uint64]$identity.publisher_epoch -ne [uint64]$publication.publisher_epoch -or
            [uint64]$identity.required_cohort_count -ne [uint64]$publication.required_columns -or
            [string]$identity.required_cohort_hash -cne [string]$publication.required_cohort_hash -or
            [string]$identity.generation_manifest_hash -notmatch '^[0-9a-f]{16}$') {
            throw "CINNABAR_CHUNK_PUBLICATION contains incoherent $identityName identity"
        }
        $expectedManifestDomain = if ($identityName -in @('publisher_disk', 'allocation')) {
            'key_generation'
        }
        else {
            'key'
        }
        if ([string]$identity.manifest_domain -cne $expectedManifestDomain) {
            throw "CINNABAR_CHUNK_PUBLICATION contains incoherent $identityName manifest domain"
        }
    }
    if ($publisherUninitialized) {
        $emptyManifestHash = 'cbf29ce484222325'
        if ([bool]$publication.required_cohort_stable -or
            [uint64]$publication.required_columns -ne 0 -or
            [uint64]$publication.loaded_required_columns -ne 0) {
            throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains a nonempty cohort'
        }
        if ([string]$publication.required_cohort_hash -cne $emptyManifestHash) {
            throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains a noncanonical empty cohort identity'
        }
        if (-not $AllowResettingPublisher) {
            foreach ($field in $requiredStageFields) {
                if ([uint64]$publication.stages.$field -ne 0) {
                    throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains stage progress'
                }
            }
            foreach ($field in @('success', 'all_air', 'unavailable', 'malformed', 'stale', 'timed_out')) {
                if ([uint64]$publication.outcomes.$field -ne 0) {
                    throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains response outcomes'
                }
            }
            foreach ($timingName in @('max_queue_wait_us', 'max_worker_time_us')) {
                foreach ($field in @('decode', 'lighting', 'meshing')) {
                    if ([decimal]$publication.$timingName.$field -ne 0) {
                        throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains stage timing'
                    }
                }
            }
        }
        foreach ($identityName in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
            if ([uint64]$presentation.$identityName.entry_count -ne 0) {
                throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains presented entries'
            }
        }
        foreach ($identityName in @('publisher_disk', 'allocation')) {
            if ([string]$presentation.$identityName.generation_manifest_hash -cne $emptyManifestHash) {
                throw 'CINNABAR_CHUNK_PUBLICATION uninitialized publisher contains a noncanonical empty manifest identity'
            }
        }
    }
    elseif ([uint64]$publication.publisher_epoch -eq 0) {
        throw 'CINNABAR_CHUNK_PUBLICATION initialized publisher has no publisher epoch'
    }
}

function Get-PublicationFirstStalledStage {
    param(
        [Parameter(Mandatory = $true)]$PublicationRecord,
        [Parameter(Mandatory = $true)][bool]$WorldReadyObserved
    )

    $publication = $PublicationRecord.publication
    $stages = $publication.stages
    $presentation = $PublicationRecord.presentation
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
    if (-not [bool]$publication.required_cohort_stable) { return 'required_cohort_identity' }
    $publisher = $presentation.publisher_disk
    $resident = $presentation.resident
    $allocation = $presentation.allocation
    $visible = $presentation.visible
    $submitted = $presentation.submitted
    $presented = $presentation.gpu_presented
    # Publisher/allocation identities contain exact key+generation manifests.
    # Resident/visible/submitted/presented identities contain render key sets.
    # Their hashes are therefore comparable only within their own domains.
    if ([uint64]$resident.entry_count -ne [uint64]$publisher.entry_count) { return 'main_apply' }
    if ([uint64]$allocation.entry_count -ne [uint64]$publisher.entry_count -or
        [string]$allocation.generation_manifest_hash -cne [string]$publisher.generation_manifest_hash) { return 'gpu_upload' }
    if (-not [bool]$presentation.visible_subset_of_resident -or
        [uint64]$visible.entry_count -gt [uint64]$resident.entry_count) { return 'extraction' }
    if ([uint64]$submitted.entry_count -ne [uint64]$visible.entry_count -or
        [string]$submitted.generation_manifest_hash -cne [string]$visible.generation_manifest_hash) { return 'submission' }
    if ([uint64]$presented.entry_count -ne [uint64]$submitted.entry_count -or
        [string]$presented.generation_manifest_hash -cne [string]$submitted.generation_manifest_hash) { return 'presentation' }
    if ($WorldReadyObserved) { return 'none' }
    return 'presentation'
}
