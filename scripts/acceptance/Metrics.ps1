function Assert-AcceptanceMetrics {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$RequireFullViewTeleport,
        $TeleportMarker,
        $ForcedRemeshMarker,
        [string]$ExpectedTargetCohort,
        [string]$SteadyResourceArtifactPath,
        $ExpectedMutationCoordinate,
        [switch]$RequireAssets,
        [string]$ExpectedAssetBlobSha256,
        [switch]$OpaqueBaselineSchema,
        [switch]$RequireTransparentWater,
        [ValidateRange(1, 2147483647)][uint64]$MinimumTransparentWaterDistinctTintCount = 1,
        [ValidateRange(0.000001, [double]::MaxValue)][double]$MaximumP99FrameMilliseconds
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "app did not write acceptance metrics: $Path"
    }
    $metrics = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    if ($OpaqueBaselineSchema -and $RequireFullViewTeleport) {
        throw 'OpaqueBaselineSchema cannot be combined with full-view validation'
    }
    if ($OpaqueBaselineSchema -and $RequireTransparentWater) {
        throw 'OpaqueBaselineSchema cannot be combined with transparent-water validation'
    }
    if ($OpaqueBaselineSchema -and (-not $RequireAssets -or $null -eq $ExpectedMutationCoordinate)) {
        throw 'OpaqueBaselineSchema requires exact asset and mutation evidence'
    }
    $currentRequired = @(
        'session_seconds', 'world_ready', 'requested_radius_chunks', 'received_radius_chunks',
        'publisher_radius_chunks', 'mutation_coordinate', 'visible_mutation_count', 'frame_count',
        'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms', 'max_decode_ms',
        'max_mesh_ms', 'max_remesh_ms', 'teleport_settle_ms', 'forced_full_view_remesh_ms',
        'max_mutation_to_visible_ms', 'decode_error_count',
        'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks',
        'peak_admitted_world_events', 'peak_admitted_heavy_events', 'peak_queued_decode_jobs',
        'peak_in_flight_decode_jobs', 'peak_completed_decode_results', 'peak_pending_retry_requests',
        'peak_outbound_requests', 'peak_pending_mesh_jobs', 'peak_in_flight_mesh_jobs',
        'gpu_upload_bytes'
    )
    $opaqueBaselineRequired = @(
        'session_seconds', 'world_ready', 'requested_radius_chunks', 'received_radius_chunks',
        'publisher_radius_chunks', 'mutation_coordinate', 'visible_mutation_count', 'frame_count',
        'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms', 'max_decode_ms',
        'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms', 'decode_error_count',
        'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks',
        'peak_admitted_world_events', 'peak_admitted_heavy_events', 'peak_queued_decode_jobs',
        'peak_in_flight_decode_jobs', 'peak_completed_decode_results', 'peak_pending_retry_requests',
        'peak_outbound_requests', 'peak_pending_mesh_jobs', 'peak_in_flight_mesh_jobs',
        'gpu_upload_bytes', 'assets'
    )
    $required = if ($OpaqueBaselineSchema) { $opaqueBaselineRequired } else { $currentRequired }
    if ($OpaqueBaselineSchema) {
        $actualFields = @($metrics.PSObject.Properties.Name)
        $missingFields = @($required | Where-Object { -not ($actualFields -ccontains $_) } | Sort-Object)
        $extraFields = @($actualFields | Where-Object { -not ($required -ccontains $_) } | Sort-Object)
        if ($missingFields.Count -ne 0 -or $extraFields.Count -ne 0) {
            $missing = if ($missingFields.Count -eq 0) { '<none>' } else { $missingFields -join ',' }
            $extra = if ($extraFields.Count -eq 0) { '<none>' } else { $extraFields -join ',' }
            throw "opaque baseline metrics schema mismatch: missing=$missing extra=$extra"
        }
    }
    foreach ($field in $required) {
        if ($null -eq $metrics.PSObject.Properties[$field]) {
            throw "acceptance metrics are missing $field"
        }
    }
    if ([double]$metrics.session_seconds -lt $DurationSeconds) {
        throw "session_seconds=$($metrics.session_seconds), expected at least $DurationSeconds"
    }
    if (-not [bool]$metrics.world_ready) {
        throw 'world_ready was false'
    }
    if ([int]$metrics.requested_radius_chunks -ne 16 -or
        [int]$metrics.received_radius_chunks -ne 16 -or
        [int]$metrics.publisher_radius_chunks -ne 16) {
        throw "radius gate failed: requested=$($metrics.requested_radius_chunks) received=$($metrics.received_radius_chunks) publisher=$($metrics.publisher_radius_chunks)"
    }
    if ([uint64]$metrics.frame_count -eq 0) {
        throw 'frame_count was zero'
    }
    if ($OpaqueBaselineSchema) {
        foreach ($field in @(
            'session_seconds', 'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms',
            'max_decode_ms', 'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms'
        )) {
            $value = ConvertTo-EvidenceDouble -Value $metrics.$field -Field "opaque baseline $field"
            if ($value -lt 0.0) {
                throw "opaque baseline $field was negative: $value"
            }
        }
    }
    $p99 = [double]$metrics.p99_frame_ms
    if ([double]::IsNaN($p99) -or [double]::IsInfinity($p99)) {
        throw "p99_frame_ms was not finite: $($metrics.p99_frame_ms)"
    }
    if ($PSBoundParameters.ContainsKey('MaximumP99FrameMilliseconds') -and
        $p99 -gt $MaximumP99FrameMilliseconds) {
        throw "p99_frame_ms exceeded manifested maximum: actual=$p99 maximum=$MaximumP99FrameMilliseconds"
    }
    if ([uint64]$metrics.decode_error_count -ne 0) {
        throw "decode_error_count=$($metrics.decode_error_count), expected zero"
    }
    foreach ($field in @('rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks')) {
        if ([uint64]$metrics.$field -eq 0) {
            throw "$field was zero"
        }
    }
    if ($OpaqueBaselineSchema -and [uint64]$metrics.gpu_upload_bytes -eq 0) {
        throw 'gpu_upload_bytes was zero for opaque baseline'
    }
    if ($RequireTransparentWater) {
        $transparentFields = @(
            'transparent_sort_request_generation', 'transparent_sort_result_generation',
            'transparent_sort_committed_generation', 'transparent_sort_encoded_generation',
            'transparent_sort_presented_generation',
            'transparent_sort_ref_count',
            'transparent_sort_cpu_ms', 'transparent_sort_request_to_commit_ms',
            'transparent_sort_staged_bytes', 'transparent_sort_upload_bytes',
            'transparent_sort_stale_reject_count', 'transparent_sort_ceiling_reject_count',
            'transparent_sort_active_slot_age_frames', 'transparent_water_distinct_tint_count',
            'nontransparent_gpu_upload_bytes'
        )
        foreach ($field in $transparentFields) {
            if ($null -eq $metrics.PSObject.Properties[$field]) {
                throw "acceptance metrics are missing $field"
            }
        }
        foreach ($field in @(
            'transparent_sort_request_generation', 'transparent_sort_result_generation',
            'transparent_sort_committed_generation', 'transparent_sort_encoded_generation',
            'transparent_sort_presented_generation',
            'transparent_sort_ref_count',
            'transparent_sort_staged_bytes', 'transparent_sort_upload_bytes',
            'transparent_sort_active_slot_age_frames', 'transparent_water_distinct_tint_count'
        )) {
            if ([uint64]$metrics.$field -eq 0) {
                throw "transparent water metric $field was zero"
            }
        }
        foreach ($field in @('transparent_sort_cpu_ms', 'transparent_sort_request_to_commit_ms')) {
            $value = [double]$metrics.$field
            if ([double]::IsNaN($value) -or [double]::IsInfinity($value) -or $value -le 0.0) {
                throw "transparent water metric $field was zero or non-finite: $($metrics.$field)"
            }
        }
        if ([uint64]$metrics.transparent_water_distinct_tint_count -lt $MinimumTransparentWaterDistinctTintCount) {
            throw "transparent_water_distinct_tint_count must be at least ${MinimumTransparentWaterDistinctTintCount}: $($metrics.transparent_water_distinct_tint_count)"
        }
        $requestGeneration = [uint64]$metrics.transparent_sort_request_generation
        $resultGeneration = [uint64]$metrics.transparent_sort_result_generation
        $committedGeneration = [uint64]$metrics.transparent_sort_committed_generation
        if ($requestGeneration -lt $resultGeneration -or $resultGeneration -lt $committedGeneration) {
            throw "transparent sort generations were not monotonic: request=$requestGeneration result=$resultGeneration committed=$committedGeneration"
        }
        if ([uint64]$metrics.transparent_sort_presented_generation -ne $committedGeneration) {
            throw "transparent presented generation did not equal committed generation: presented=$($metrics.transparent_sort_presented_generation) committed=$committedGeneration"
        }
        if ([uint64]$metrics.transparent_sort_encoded_generation -ne $committedGeneration) {
            throw "transparent encoded generation did not equal committed generation: encoded=$($metrics.transparent_sort_encoded_generation) committed=$committedGeneration"
        }
        if ([uint64]$metrics.transparent_sort_upload_bytes -gt [uint64]$metrics.transparent_sort_staged_bytes) {
            throw "transparent sort upload exceeded staged bytes: upload=$($metrics.transparent_sort_upload_bytes) staged=$($metrics.transparent_sort_staged_bytes)"
        }
        $expectedGpuUploadBytes = [uint64]$metrics.nontransparent_gpu_upload_bytes + [uint64]$metrics.transparent_sort_upload_bytes
        if ([uint64]$metrics.gpu_upload_bytes -ne $expectedGpuUploadBytes) {
            throw "gpu_upload_bytes did not equal nontransparent plus transparent uploads: gpu=$($metrics.gpu_upload_bytes) nontransparent=$($metrics.nontransparent_gpu_upload_bytes) transparent=$($metrics.transparent_sort_upload_bytes)"
        }
    }
    if ($null -ne $ExpectedMutationCoordinate) {
        $expectedMutation = @($ExpectedMutationCoordinate)
        $actualMutation = @($metrics.mutation_coordinate)
        if ($expectedMutation.Count -ne 3) {
            throw "expected target mutation coordinate did not have three components: $($expectedMutation -join ',')"
        }
        if ([uint64]$metrics.visible_mutation_count -eq 0) {
            throw 'visible_mutation_count was zero for target mutation evidence'
        }
        if ($actualMutation.Count -ne 3 -or
            (($actualMutation | ForEach-Object { [int]$_ }) -join ',') -cne
            (($expectedMutation | ForEach-Object { [int]$_ }) -join ',')) {
            throw "mutation_coordinate did not match manifested target: expected=$($expectedMutation -join ',') actual=$($actualMutation -join ',')"
        }
    }
    elseif (-not $RequireFullViewTeleport -and [uint64]$metrics.visible_mutation_count -eq 0) {
        throw 'visible_mutation_count was zero'
    }
    if ($RequireAssets) {
        if ($ExpectedAssetBlobSha256 -notmatch '^[0-9a-fA-F]{64}$') {
            throw "expected asset blob SHA-256 was invalid: $ExpectedAssetBlobSha256"
        }
        $assetsProperty = $metrics.PSObject.Properties['assets']
        if ($null -eq $assetsProperty -or $null -eq $assetsProperty.Value) {
            throw 'acceptance metrics are missing assets'
        }
        $assetMetrics = $assetsProperty.Value
        $requiredAssetFields = @(
            'source_tag', 'source_sha256', 'blob_sha256', 'texture_layers',
            'texture_bytes_including_mips', 'material_count', 'missing_mapping_count',
            'diagnostic_quad_count'
        )
        if ($OpaqueBaselineSchema) {
            $actualAssetFields = @($assetMetrics.PSObject.Properties.Name)
            $missingAssetFields = @($requiredAssetFields | Where-Object { -not ($actualAssetFields -ccontains $_) } | Sort-Object)
            $extraAssetFields = @($actualAssetFields | Where-Object { -not ($requiredAssetFields -ccontains $_) } | Sort-Object)
            if ($missingAssetFields.Count -ne 0 -or $extraAssetFields.Count -ne 0) {
                $missing = if ($missingAssetFields.Count -eq 0) { '<none>' } else { $missingAssetFields -join ',' }
                $extra = if ($extraAssetFields.Count -eq 0) { '<none>' } else { $extraAssetFields -join ',' }
                throw "opaque baseline asset schema mismatch: missing=$missing extra=$extra"
            }
        }
        foreach ($field in $requiredAssetFields) {
            if ($null -eq $assetMetrics.PSObject.Properties[$field]) {
                throw "acceptance asset metrics are missing $field"
            }
        }
        if ([string]$assetMetrics.source_tag -cne $PinnedAssetSourceTag) {
            throw "asset source_tag did not match pinned source: expected=$PinnedAssetSourceTag actual=$($assetMetrics.source_tag)"
        }
        if ([string]$assetMetrics.source_sha256 -cne $PinnedAssetSourceSha256) {
            throw "asset source_sha256 did not match pinned source: expected=$PinnedAssetSourceSha256 actual=$($assetMetrics.source_sha256)"
        }
        if ([string]$assetMetrics.blob_sha256 -cne $ExpectedAssetBlobSha256.ToLowerInvariant()) {
            throw "asset blob_sha256 did not match supplied blob: expected=$($ExpectedAssetBlobSha256.ToLowerInvariant()) actual=$($assetMetrics.blob_sha256)"
        }
        if ([uint64]$assetMetrics.texture_layers -eq 0 -or
            [uint64]$assetMetrics.texture_bytes_including_mips -eq 0 -or
            [uint64]$assetMetrics.material_count -eq 0) {
            throw "asset metrics were not populated: layers=$($assetMetrics.texture_layers) bytes=$($assetMetrics.texture_bytes_including_mips) materials=$($assetMetrics.material_count)"
        }
        if ([uint64]$assetMetrics.missing_mapping_count -ne 0) {
            throw "asset missing_mapping_count=$($assetMetrics.missing_mapping_count), expected zero"
        }
    }
    if ($RequireFullViewTeleport) {
        if ([string]::IsNullOrWhiteSpace($SteadyResourceArtifactPath) -or
            -not (Test-Path -LiteralPath $SteadyResourceArtifactPath -PathType Leaf)) {
            throw "steady resource artifact was not written before full-view SLA validation: $SteadyResourceArtifactPath"
        }
        if ($null -eq $TeleportMarker) {
            throw 'parsed teleport settle marker was not supplied'
        }
        if ($null -eq $ForcedRemeshMarker) {
            throw 'parsed forced-remesh settle marker was not supplied'
        }
        if ([string]::IsNullOrWhiteSpace($ExpectedTargetCohort)) {
            throw 'expected target cohort was not supplied'
        }
        $teleportProofProperty = $metrics.PSObject.Properties['teleport_proof']
        if ($null -eq $teleportProofProperty -or $null -eq $teleportProofProperty.Value) {
            throw 'acceptance metrics are missing teleport_proof'
        }
        $remeshProofProperty = $metrics.PSObject.Properties['forced_full_view_remesh_proof']
        if ($null -eq $remeshProofProperty -or $null -eq $remeshProofProperty.Value) {
            throw 'acceptance metrics are missing forced_full_view_remesh_proof'
        }
        if ($null -eq $metrics.teleport_settle_ms) {
            throw 'teleport_settle_ms was not recorded'
        }
        $teleport = [double]$metrics.teleport_settle_ms
        if ($null -eq $metrics.forced_full_view_remesh_ms) {
            throw 'forced_full_view_remesh_ms was not recorded'
        }
        $remesh = [double]$metrics.forced_full_view_remesh_ms

        Assert-ExactFullViewProof `
            -Proof $teleportProofProperty.Value `
            -Kind Teleport `
            -Label 'teleport_proof' `
            -ExpectedTargetCohort $ExpectedTargetCohort
        Assert-ExactFullViewProof `
            -Proof $remeshProofProperty.Value `
            -Kind ForcedRemesh `
            -Label 'forced_full_view_remesh_proof' `
            -ExpectedTargetCohort $ExpectedTargetCohort
        Assert-FullViewProofCohortContinuity `
            -TeleportProof $teleportProofProperty.Value `
            -ForcedRemeshProof $remeshProofProperty.Value
        Assert-MarkerMatchesProof `
            -Marker $TeleportMarker `
            -Proof $teleportProofProperty.Value `
            -Kind Teleport `
            -Label 'teleport'
        Assert-MarkerMatchesProof `
            -Marker $ForcedRemeshMarker `
            -Proof $remeshProofProperty.Value `
            -Kind ForcedRemesh `
            -Label 'forced remesh'
        Assert-SteadyResourceArtifact `
            -Path $SteadyResourceArtifactPath `
            -TeleportMarker $TeleportMarker `
            -ForcedRemeshMarker $ForcedRemeshMarker

        if ([double]::IsNaN($teleport) -or
            [double]::IsInfinity($teleport) -or
            [Math]::Abs($teleport - [double]$teleportProofProperty.Value.ms) -gt 0.001) {
            throw "teleport_settle_ms did not match its exact proof: metric=$teleport proof=$($teleportProofProperty.Value.ms)"
        }
        if ([double]::IsNaN($remesh) -or
            [double]::IsInfinity($remesh) -or
            [Math]::Abs($remesh - [double]$remeshProofProperty.Value.ms) -gt 0.001) {
            throw "forced_full_view_remesh_ms did not match its exact proof: metric=$remesh proof=$($remeshProofProperty.Value.ms)"
        }
        if ($teleport -gt 2000.0) {
            throw "teleport_settle_ms failed the 2000ms gate: $($metrics.teleport_settle_ms)"
        }
        if ($remesh -gt 2000.0) {
            throw "forced_full_view_remesh_ms failed the 2000ms gate: $($metrics.forced_full_view_remesh_ms)"
        }
    }
    $mutationLatency = [double]$metrics.max_mutation_to_visible_ms
    if ([double]::IsNaN($mutationLatency) -or
        [double]::IsInfinity($mutationLatency) -or
        $mutationLatency -gt 100.0) {
        throw "max_mutation_to_visible_ms=$($metrics.max_mutation_to_visible_ms), expected finite <= 100"
    }
    return $metrics
}
