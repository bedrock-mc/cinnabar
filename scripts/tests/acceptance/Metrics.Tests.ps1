    $metrics = [ordered]@{
        session_seconds = 900.0; world_ready = $true; requested_radius_chunks = 16
        received_radius_chunks = 16; publisher_radius_chunks = 16
        mutation_coordinate = @(1, 2, 3); visible_mutation_count = 1; frame_count = 1
        p50_frame_ms = 1.0; p95_frame_ms = 2.0; p99_frame_ms = 3.0; max_frame_ms = 4.0
        max_decode_ms = 1.0; max_mesh_ms = 1.0; max_remesh_ms = 1.0
        teleport_settle_ms = $null; forced_full_view_remesh_ms = $null
        max_mutation_to_visible_ms = 50.0; decode_error_count = 0
        rendered_sub_chunks = 1; resident_sub_chunks = 1; visible_sub_chunks = 1
        peak_admitted_world_events = 1; peak_admitted_heavy_events = 1
        peak_queued_decode_jobs = 1; peak_in_flight_decode_jobs = 1
        peak_completed_decode_results = 1; peak_pending_retry_requests = 1
        peak_outbound_requests = 1; peak_pending_mesh_jobs = 1
        peak_in_flight_mesh_jobs = 1; gpu_upload_bytes = 1
        assets = [ordered]@{
            source_tag = 'v1.26.30.32-preview'
            source_sha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
            blob_sha256 = $expectedAssetBlobSha256
            texture_layers = 372
            texture_bytes_including_mips = 1000
            material_count = 405
            missing_mapping_count = 0
            diagnostic_quad_count = 12
        }
        teleport_proof = [ordered]@{
            target = '0:65:65:16'; committed = '0:65:65:16'; ms = 1500.0
            view_generation = 7; transparent_sort_generation = 11; render_ready_ms = 1200.0; publisher_ms = 100.0
            first_level_ms = 200.0; last_level_ms = 600.0; level_events = 1089
            first_sub_ms = 250.0; last_sub_ms = 900.0; sub_events = 1089
            first_frame_sequence = 41; stable_frame_sequence = 42
            first_present_ms = 1300.0; first_gpu_ms = 1350.0
            stable_present_ms = 1400.0; stable_gpu_ms = 1500.0; frame_count = 90
            expected_manifest_count = 4; expected_manifest_hash = '1111222233334444'
            first_presented_manifest_count = 4; first_presented_manifest_hash = '1111222233334444'
            stable_presented_manifest_count = 4; stable_presented_manifest_hash = '1111222233334444'
            expected = 1089; loaded_target = 1089; missing_target = 0
            foreign_loaded = 0; foreign_requested = 0; foreign_resident = 0; source_leftover = 0
            resident_count = 3; resident_hash = 'aaaabbbbccccdddd'
            known_air_count = 1; known_air_hash = 'eeeeffff00001111'
            missing_target_instances = 0; unexpected_target_instances = 0; source_instances = 0
            foreign_instances = 0; stale_generation_instances = 0; orphan_allocations = 0
        }
        forced_full_view_remesh_proof = [ordered]@{
            target = '0:65:65:16'; committed = '0:65:65:16'; ms = 1500.0
            view_generation = 8; transparent_sort_generation = 12; render_ready_ms = 0.0
            first_frame_sequence = 43; stable_frame_sequence = 44
            first_present_ms = 1200.0; first_gpu_ms = 1300.0
            stable_present_ms = 1400.0; stable_gpu_ms = 1500.0; frame_count = 90
            expected_manifest_count = 4; expected_manifest_hash = '5555666677778888'
            first_presented_manifest_count = 4; first_presented_manifest_hash = '5555666677778888'
            stable_presented_manifest_count = 4; stable_presented_manifest_hash = '5555666677778888'
            expected = 1089; loaded_target = 1089; missing_target = 0
            foreign_loaded = 0; foreign_requested = 0; foreign_resident = 0; source_leftover = 0
            resident_count = 3; resident_hash = 'aaaabbbbccccdddd'
            known_air_count = 1; known_air_hash = 'eeeeffff00001111'
            missing_target_instances = 0; unexpected_target_instances = 0; source_instances = 0
            foreign_instances = 0; stale_generation_instances = 0; orphan_allocations = 0
        }
    }
    $null = Assert-MarkerMatchesProof -Marker $teleportMarker -Proof ([pscustomobject]$metrics.teleport_proof) -Kind Teleport -Label 'teleport proof'
    $metrics.teleport_proof.transparent_sort_generation = 13
    Assert-ThrowsLike {
        Assert-MarkerMatchesProof -Marker $teleportMarker -Proof ([pscustomobject]$metrics.teleport_proof) -Kind Teleport -Label 'teleport proof'
    } 'teleport proof marker/metrics mismatch for transparent_sort_generation*' 'full-view proof accepted a different presented transparent generation'
    $metrics.teleport_proof.transparent_sort_generation = 11
    $metricsPath = Join-Path $TempRoot 'validation-metrics.json'
    $steadyResourceArtifactPath = Join-Path $TempRoot 'steady-resources.json'
    $steadyArtifactSamples = @(1..30 | ForEach-Object {
        [pscustomobject]@{
            elapsed_seconds = [double]$_
            combined_rss_bytes = 350MB
            cpu_percent = 10.0
        }
    })
    $steadyArtifactTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    $steadyArtifact = New-SteadyResourceDocument `
        -Samples $steadyArtifactSamples `
        -DurationSeconds 30 `
        -Trigger $steadyArtifactTrigger
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath

    $transparentMetrics = [ordered]@{}
    foreach ($key in $metrics.Keys) {
        $transparentMetrics[$key] = $metrics[$key]
    }
    $transparentMetrics.transparent_sort_request_generation = 4
    $transparentMetrics.transparent_sort_result_generation = 4
    $transparentMetrics.transparent_sort_committed_generation = 4
    $transparentMetrics.transparent_sort_encoded_generation = 4
    $transparentMetrics.transparent_sort_presented_generation = 4
    $transparentMetrics.transparent_sort_ref_count = 10
    $transparentMetrics.transparent_sort_cpu_ms = 0.25
    $transparentMetrics.transparent_sort_request_to_commit_ms = 3.5
    $transparentMetrics.transparent_sort_staged_bytes = 160
    $transparentMetrics.transparent_sort_upload_bytes = 160
    $transparentMetrics.transparent_sort_stale_reject_count = 0
    $transparentMetrics.transparent_sort_ceiling_reject_count = 0
    $transparentMetrics.transparent_sort_active_slot_age_frames = 2
    $transparentMetrics.transparent_water_distinct_tint_count = 1
    $transparentMetrics.nontransparent_gpu_upload_bytes = 1
    $transparentMetrics.gpu_upload_bytes = 161
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    $transparentMetrics.p99_frame_ms = 16.6
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater -MaximumP99FrameMilliseconds (1000.0 / 60.0)
    foreach ($failingP99 in @(16.7, 17.5)) {
        $transparentMetrics.p99_frame_ms = $failingP99
        $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike {
            Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater -MaximumP99FrameMilliseconds (1000.0 / 60.0)
        } 'p99_frame_ms exceeded manifested maximum*' "water acceptance accepted p99_frame_ms=$failingP99 above 60fps"
    }
    $transparentMetrics.p99_frame_ms = 3.0
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    foreach ($field in @(
        'transparent_sort_request_generation', 'transparent_sort_result_generation',
        'transparent_sort_committed_generation', 'transparent_sort_encoded_generation',
        'transparent_sort_presented_generation', 'transparent_sort_ref_count',
        'transparent_sort_cpu_ms', 'transparent_sort_request_to_commit_ms',
        'transparent_sort_staged_bytes', 'transparent_sort_upload_bytes',
        'transparent_sort_active_slot_age_frames', 'transparent_water_distinct_tint_count'
    )) {
        $saved = $transparentMetrics[$field]
        $transparentMetrics[$field] = 0
        $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike {
            Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
        } "transparent water metric $field was zero*" "water acceptance accepted zero $field"
        $transparentMetrics[$field] = $saved
    }
    $transparentMetrics.transparent_sort_result_generation = 5
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent sort generations were not monotonic*' 'water acceptance accepted a result newer than its request'
    $transparentMetrics.transparent_sort_result_generation = 4
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater -MinimumTransparentWaterDistinctTintCount 2
    } 'transparent_water_distinct_tint_count must be at least 2*' 'configurable water acceptance accepted fewer runtime tints than its manifested minimum'
    $transparentMetrics.transparent_water_distinct_tint_count = 0
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent water metric transparent_water_distinct_tint_count was zero*' 'water acceptance accepted no runtime water tint'
    $transparentMetrics.transparent_water_distinct_tint_count = 1
    $transparentMetrics.transparent_sort_presented_generation = 3
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent presented generation did not equal committed generation*' 'water acceptance accepted an unpresented committed sort'
    $transparentMetrics.transparent_sort_presented_generation = 4
    $transparentMetrics.transparent_sort_encoded_generation = 3
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent encoded generation did not equal committed generation*' 'water acceptance accepted a committed sort whose draw was not encoded'
    $transparentMetrics.transparent_sort_encoded_generation = 4
    $transparentMetrics.transparent_sort_upload_bytes = 161
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent sort upload exceeded staged bytes*' 'water acceptance accepted impossible sort upload accounting'
    $transparentMetrics.transparent_sort_upload_bytes = 160
    $transparentMetrics.gpu_upload_bytes = 162
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'gpu_upload_bytes did not equal nontransparent plus transparent uploads*' 'water acceptance accepted inexact GPU byte accounting'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath

    $approvedOpaqueBlobSha256 = 'af98e5ddd5532972bf99b9fc3bdd3819bb06b1d8696198f135a9d96ae27ca7ba'
    $opaqueBaselineMetrics = [ordered]@{
        session_seconds = 60.0095326
        world_ready = $true
        requested_radius_chunks = 16
        received_radius_chunks = 16
        publisher_radius_chunks = 16
        mutation_coordinate = @(27, 73, 91)
        visible_mutation_count = 1
        frame_count = 5732
        p50_frame_ms = 10.1
        p95_frame_ms = 14.3
        p99_frame_ms = 17.0
        max_frame_ms = 96.7656
        max_decode_ms = 1.6392
        max_mesh_ms = 10.3533
        max_remesh_ms = 27701.8793
        max_mutation_to_visible_ms = 48.663
        decode_error_count = 0
        rendered_sub_chunks = 9495
        resident_sub_chunks = 10445
        visible_sub_chunks = 4802
        peak_admitted_world_events = 27
        peak_admitted_heavy_events = 27
        peak_queued_decode_jobs = 3
        peak_in_flight_decode_jobs = 4
        peak_completed_decode_results = 20
        peak_pending_retry_requests = 0
        peak_outbound_requests = 3
        peak_pending_mesh_jobs = 20646
        peak_in_flight_mesh_jobs = 64
        gpu_upload_bytes = 25976256
        assets = [ordered]@{
            source_tag = 'v1.26.30.32-preview'
            source_sha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
            blob_sha256 = $approvedOpaqueBlobSha256
            texture_layers = 388
            texture_bytes_including_mips = 529232
            material_count = 421
            missing_mapping_count = 0
            diagnostic_quad_count = 588885
        }
    }
    $approvedOpaqueKeys = @(
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
    $approvedOpaqueAssetKeys = @(
        'source_tag', 'source_sha256', 'blob_sha256', 'texture_layers',
        'texture_bytes_including_mips', 'material_count', 'missing_mapping_count',
        'diagnostic_quad_count'
    )
    Assert-Equal 31 @($opaqueBaselineMetrics.Keys).Count 'approved opaque fixture did not have exactly 31 top-level keys'
    Assert-Equal (($approvedOpaqueKeys | Sort-Object) -join ',') (@($opaqueBaselineMetrics.Keys | Sort-Object) -join ',') 'approved opaque fixture key set changed'
    Assert-Equal 8 @($opaqueBaselineMetrics.assets.Keys).Count 'approved opaque fixture did not have exactly eight asset keys'
    Assert-Equal (($approvedOpaqueAssetKeys | Sort-Object) -join ',') (@($opaqueBaselineMetrics.assets.Keys | Sort-Object) -join ',') 'approved opaque asset key set changed'
    Assert-True (-not $opaqueBaselineMetrics.Contains('teleport_settle_ms')) 'approved opaque fixture unexpectedly gained teleport_settle_ms'
    Assert-True (-not $opaqueBaselineMetrics.Contains('forced_full_view_remesh_ms')) 'approved opaque fixture unexpectedly gained forced_full_view_remesh_ms'
    Assert-True (-not $opaqueBaselineMetrics.Contains('teleport_proof')) 'approved opaque fixture unexpectedly gained teleport_proof'
    Assert-True (-not $opaqueBaselineMetrics.Contains('forced_full_view_remesh_proof')) 'approved opaque fixture unexpectedly gained forced_full_view_remesh_proof'

    $opaqueBaselineMetricsPath = Join-Path $TempRoot 'opaque-baseline-validation-metrics.json'
    $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $opaqueBaselineMetricsPath
    } 'acceptance metrics are missing teleport_settle_ms' 'approved base schema unexpectedly passed the current metrics path'
    $opaqueBaselineArguments = @{
        Path = $opaqueBaselineMetricsPath
        OpaqueBaselineSchema = $true
        ExpectedMutationCoordinate = @(27, 73, 91)
        RequireAssets = $true
        ExpectedAssetBlobSha256 = $approvedOpaqueBlobSha256
    }
    $originalDurationSeconds = $DurationSeconds
    $DurationSeconds = 60
    try {
        $null = Assert-AcceptanceMetrics @opaqueBaselineArguments

        $missingOpaqueField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $missingOpaqueField.PSObject.Properties.Remove('gpu_upload_bytes')
        $missingOpaqueField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline metrics schema mismatch:*missing=gpu_upload_bytes*' 'opaque baseline schema accepted a missing approved key'

        $extraOpaqueField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $extraOpaqueField | Add-Member -MemberType NoteProperty -Name unexpected_field -Value 1
        $extraOpaqueField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline metrics schema mismatch:*extra=unexpected_field*' 'opaque baseline schema accepted an unknown key'

        $currentSchemaAsOpaque = $metrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $currentSchemaAsOpaque | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline metrics schema mismatch:*extra=*teleport_settle_ms*' 'opaque baseline switch accepted the current metrics schema'

        $missingOpaqueAssetField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $missingOpaqueAssetField.assets.PSObject.Properties.Remove('diagnostic_quad_count')
        $missingOpaqueAssetField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline asset schema mismatch:*missing=diagnostic_quad_count*' 'opaque baseline schema accepted a missing asset key'

        $extraOpaqueAssetField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $extraOpaqueAssetField.assets | Add-Member -MemberType NoteProperty -Name unexpected_asset_field -Value 1
        $extraOpaqueAssetField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline asset schema mismatch:*extra=unexpected_asset_field*' 'opaque baseline schema accepted an unknown asset key'

        $opaqueSafetyCases = @(
            [pscustomobject]@{ Name = 'short session'; Pattern = 'session_seconds=*expected at least 60'; Mutate = { param($m) $m.session_seconds = 59.0 } },
            [pscustomobject]@{ Name = 'world not ready'; Pattern = 'world_ready was false'; Mutate = { param($m) $m.world_ready = $false } },
            [pscustomobject]@{ Name = 'requested radius'; Pattern = 'radius gate failed:*'; Mutate = { param($m) $m.requested_radius_chunks = 15 } },
            [pscustomobject]@{ Name = 'received radius'; Pattern = 'radius gate failed:*'; Mutate = { param($m) $m.received_radius_chunks = 15 } },
            [pscustomobject]@{ Name = 'publisher radius'; Pattern = 'radius gate failed:*'; Mutate = { param($m) $m.publisher_radius_chunks = 15 } },
            [pscustomobject]@{ Name = 'wrong mutation coordinate'; Pattern = 'mutation_coordinate did not match manifested target:*'; Mutate = { param($m) $m.mutation_coordinate = @(27, 73, 92) } },
            [pscustomobject]@{ Name = 'no visible mutation'; Pattern = 'visible_mutation_count was zero for target mutation evidence'; Mutate = { param($m) $m.visible_mutation_count = 0 } },
            [pscustomobject]@{ Name = 'no frames'; Pattern = 'frame_count was zero'; Mutate = { param($m) $m.frame_count = 0 } },
            [pscustomobject]@{ Name = 'no rendered chunks'; Pattern = 'rendered_sub_chunks was zero'; Mutate = { param($m) $m.rendered_sub_chunks = 0 } },
            [pscustomobject]@{ Name = 'no resident chunks'; Pattern = 'resident_sub_chunks was zero'; Mutate = { param($m) $m.resident_sub_chunks = 0 } },
            [pscustomobject]@{ Name = 'no visible chunks'; Pattern = 'visible_sub_chunks was zero'; Mutate = { param($m) $m.visible_sub_chunks = 0 } },
            [pscustomobject]@{ Name = 'no GPU uploads'; Pattern = 'gpu_upload_bytes was zero for opaque baseline'; Mutate = { param($m) $m.gpu_upload_bytes = 0 } },
            [pscustomobject]@{ Name = 'decode errors'; Pattern = 'decode_error_count=1, expected zero'; Mutate = { param($m) $m.decode_error_count = 1 } },
            [pscustomobject]@{ Name = 'missing mapping'; Pattern = 'asset missing_mapping_count=1, expected zero'; Mutate = { param($m) $m.assets.missing_mapping_count = 1 } },
            [pscustomobject]@{ Name = 'wrong source tag'; Pattern = 'asset source_tag did not match pinned source:*'; Mutate = { param($m) $m.assets.source_tag = 'wrong' } },
            [pscustomobject]@{ Name = 'wrong source hash'; Pattern = 'asset source_sha256 did not match pinned source:*'; Mutate = { param($m) $m.assets.source_sha256 = ('0' * 64) } },
            [pscustomobject]@{ Name = 'wrong blob hash'; Pattern = 'asset blob_sha256 did not match supplied blob:*'; Mutate = { param($m) $m.assets.blob_sha256 = ('0' * 64) } },
            [pscustomobject]@{ Name = 'no texture layers'; Pattern = 'asset metrics were not populated:*'; Mutate = { param($m) $m.assets.texture_layers = 0 } },
            [pscustomobject]@{ Name = 'no mip bytes'; Pattern = 'asset metrics were not populated:*'; Mutate = { param($m) $m.assets.texture_bytes_including_mips = 0 } },
            [pscustomobject]@{ Name = 'no materials'; Pattern = 'asset metrics were not populated:*'; Mutate = { param($m) $m.assets.material_count = 0 } }
        )
        foreach ($case in $opaqueSafetyCases) {
            $candidate = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            & $case.Mutate $candidate
            $candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
            Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } $case.Pattern "opaque baseline accepted unsafe $($case.Name)"
        }

        foreach ($field in @(
            'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms',
            'max_decode_ms', 'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms'
        )) {
            $candidate = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            $candidate.$field = 'NaN'
            $candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
            Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } "opaque baseline $field was not finite:*" "opaque baseline accepted nonfinite $field"
        }

        $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        $opaqueFullViewArguments = @{}
        foreach ($key in $opaqueBaselineArguments.Keys) { $opaqueFullViewArguments[$key] = $opaqueBaselineArguments[$key] }
        $opaqueFullViewArguments['RequireFullViewTeleport'] = $true
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueFullViewArguments } 'OpaqueBaselineSchema cannot be combined with full-view validation' 'opaque baseline schema weakened the full-view gate'
        Assert-True `
            ([regex]::IsMatch(
                $source,
                'if \(\$LeafForestBaseline\)[\s\S]*?OpaqueBaselineSchema',
                [Text.RegularExpressions.RegexOptions]::CultureInvariant
            )) `
            'live LeafForestBaseline path did not select the explicit opaque baseline schema'
    }
    finally {
        $DurationSeconds = $originalDurationSeconds
    }

    $metrics.teleport_settle_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 1500.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $fullViewArguments = @{
        Path = $metricsPath
        RequireFullViewTeleport = $true
        TeleportMarker = $teleportMarker
        ForcedRemeshMarker = $forcedMarker
        ExpectedTargetCohort = '0:65:65:16'
        SteadyResourceArtifactPath = $steadyResourceArtifactPath
        ExpectedMutationCoordinate = @(1, 2, 3)
        RequireAssets = $true
        ExpectedAssetBlobSha256 = $expectedAssetBlobSha256
    }
    $null = Assert-AcceptanceMetrics @fullViewArguments

    $metrics.visible_mutation_count = 0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'visible_mutation_count was zero for target mutation*' 'full-view leaf evidence accepted no visible target mutation'
    $metrics.visible_mutation_count = 1
    $metrics.mutation_coordinate = @(9, 9, 9)
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'mutation_coordinate did not match manifested target*' 'full-view leaf evidence accepted the source/wrong mutation coordinate'
    $metrics.mutation_coordinate = @(1, 2, 3)
    $metrics.assets.missing_mapping_count = 1
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'asset missing_mapping_count=1, expected zero*' 'leaf evidence accepted a missing asset mapping'
    $metrics.assets.missing_mapping_count = 0
    $metrics.assets.blob_sha256 = 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'asset blob_sha256 did not match supplied blob*' 'leaf evidence accepted metrics from the wrong asset blob'
    $metrics.assets.blob_sha256 = $expectedAssetBlobSha256
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath

    $staleResourceArtifact = $steadyArtifact | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    $staleResourceArtifact.trigger.target = '0:66:65:16'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($staleResourceArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'steady resource artifact trigger mismatch for target*' 'stale steady-resource trigger provenance passed validation'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $tamperedResourceArtifact = $steadyArtifact | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    $tamperedResourceArtifact.summary.max_combined_rss_bytes = 1
    $tamperedResourceArtifact.summary.mean_cpu_percent = 0.0
    $tamperedResourceArtifact.summary.p95_cpu_percent = 0.0
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($tamperedResourceArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'steady resource artifact summary did not match samples*' 'tampered steady-resource summary passed validation'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )

    $singleFrameTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('frame_count=90', 'frame_count=1') `
        -Kind Teleport
    $singleFrameArguments = $fullViewArguments.Clone()
    $singleFrameArguments.TeleportMarker = $singleFrameTeleportMarker
    $metrics.teleport_proof.frame_count = 1
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @singleFrameArguments } 'teleport_proof.frame_count must cover at least two presented frames*' 'a one-frame presented interval passed validation'
    $metrics.teleport_proof.frame_count = 90

    $changedCohortRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('resident_hash=aaaabbbbccccdddd', 'resident_hash=0000000000000001') `
        -Kind ForcedRemesh
    $changedCohortArguments = $fullViewArguments.Clone()
    $changedCohortArguments.ForcedRemeshMarker = $changedCohortRemeshMarker
    $metrics.forced_full_view_remesh_proof.resident_hash = '0000000000000001'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @changedCohortArguments } 'full-view proof cohort changed between teleport and forced remesh at resident_hash*' 'forced remesh silently accepted a changed resident cohort'
    $metrics.forced_full_view_remesh_proof.resident_hash = 'aaaabbbbccccdddd'

    $changedManifestCountMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('manifest_count=4', 'manifest_count=5') `
        -Kind ForcedRemesh
    $changedManifestCountArguments = $fullViewArguments.Clone()
    $changedManifestCountArguments.ForcedRemeshMarker = $changedManifestCountMarker
    $metrics.forced_full_view_remesh_proof.expected_manifest_count = 5
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_count = 5
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_count = 5
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @changedManifestCountArguments } 'forced remesh expected manifest count changed from teleport*' 'forced remesh silently changed its mesh-bearing key count'
    $metrics.forced_full_view_remesh_proof.expected_manifest_count = 4
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_count = 4
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_count = 4

    $earlyRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('first_frame_sequence=43 stable_frame_sequence=44', 'first_frame_sequence=42 stable_frame_sequence=43') `
        -Kind ForcedRemesh
    $earlyRemeshArguments = $fullViewArguments.Clone()
    $earlyRemeshArguments.ForcedRemeshMarker = $earlyRemeshMarker
    $metrics.forced_full_view_remesh_proof.first_frame_sequence = 42
    $metrics.forced_full_view_remesh_proof.stable_frame_sequence = 43
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @earlyRemeshArguments } 'forced remesh frames were not later than teleport frames*' 'forced remesh reused the teleport stable frame'
    $metrics.forced_full_view_remesh_proof.first_frame_sequence = 43
    $metrics.forced_full_view_remesh_proof.stable_frame_sequence = 44

    $staleGenerationRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('view_generation=8', 'view_generation=7') `
        -Kind ForcedRemesh
    $staleGenerationArguments = $fullViewArguments.Clone()
    $staleGenerationArguments.ForcedRemeshMarker = $staleGenerationRemeshMarker
    $metrics.forced_full_view_remesh_proof.view_generation = 7
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @staleGenerationArguments } 'forced remesh view generation did not advance beyond teleport*' 'forced remesh reused the teleport view generation'
    $metrics.forced_full_view_remesh_proof.view_generation = 8

    $unchangedManifestRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('5555666677778888', '1111222233334444') `
        -Kind ForcedRemesh
    $unchangedManifestArguments = $fullViewArguments.Clone()
    $unchangedManifestArguments.ForcedRemeshMarker = $unchangedManifestRemeshMarker
    $metrics.forced_full_view_remesh_proof.expected_manifest_hash = '1111222233334444'
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_hash = '1111222233334444'
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_hash = '1111222233334444'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @unchangedManifestArguments } 'forced remesh expected manifest hash did not change from teleport*' 'forced remesh did not prove new mesh generations'
    $metrics.forced_full_view_remesh_proof.expected_manifest_hash = '5555666677778888'
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_hash = '5555666677778888'
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_hash = '5555666677778888'

    $metrics.teleport_settle_ms = 2000.1
    $metrics.teleport_proof.ms = 2000.1
    $metrics.teleport_proof.stable_gpu_ms = 2000.1
    $slowTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('ms=1500.0000', 'ms=2000.1000') `
        -Kind Teleport
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $slowTeleportArguments = $fullViewArguments.Clone()
    $slowTeleportArguments.TeleportMarker = $slowTeleportMarker
    Assert-ThrowsLike `
        { Assert-AcceptanceMetrics @slowTeleportArguments } `
        'teleport_settle_ms failed the 2000ms gate*' `
        'over-budget end-to-end teleport with a fast remesh passed validation'
    Assert-True (Test-Path -LiteralPath $steadyResourceArtifactPath -PathType Leaf) 'resource artifact was not retained before the teleport SLA failure surfaced'

    $metrics.teleport_settle_ms = 1500.0
    $metrics.teleport_proof.ms = 1500.0
    $metrics.teleport_proof.stable_gpu_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 2000.1
    $metrics.forced_full_view_remesh_proof.ms = 2000.1
    $metrics.forced_full_view_remesh_proof.stable_gpu_ms = 2000.1
    $slowRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('ms=1500.0000', 'ms=2000.1000') `
        -Kind ForcedRemesh
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $slowRemeshArguments = $fullViewArguments.Clone()
    $slowRemeshArguments.ForcedRemeshMarker = $slowRemeshMarker
    Assert-ThrowsLike `
        { Assert-AcceptanceMetrics @slowRemeshArguments } `
        'forced_full_view_remesh_ms failed the 2000ms gate*' `
        'over-budget forced full-view remesh with a fast teleport passed validation'
    $metrics.forced_full_view_remesh_ms = $null
    $metrics.forced_full_view_remesh_proof.ms = 1500.0
    $metrics.forced_full_view_remesh_proof.stable_gpu_ms = 1500.0

    $metrics.teleport_settle_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 1500.0
    foreach ($field in @('missing_target', 'foreign_loaded', 'foreign_requested', 'foreign_resident', 'source_leftover')) {
        $metrics.teleport_proof[$field] = 1
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike `
            { Assert-AcceptanceMetrics @fullViewArguments } `
            "teleport_proof.$field*expected zero*" `
            "non-exact teleport cohort field $field passed validation"
        $metrics.teleport_proof[$field] = 0
    }
    $metrics.teleport_proof.loaded_target = 1088
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof loaded/expected cohort counts were not exact*' 'missing destination column passed validation'
    $metrics.teleport_proof.loaded_target = 1089

    $wrongCenterArguments = $fullViewArguments.Clone()
    $wrongCenterArguments.ExpectedTargetCohort = '0:66:65:16'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @wrongCenterArguments } 'teleport_proof target cohort mismatch*' 'wrong destination center passed validation'
    $wrongRadiusArguments = $fullViewArguments.Clone()
    $wrongRadiusArguments.ExpectedTargetCohort = '0:65:65:15'
    Assert-ThrowsLike { Assert-AcceptanceMetrics @wrongRadiusArguments } 'teleport_proof target cohort mismatch*' 'wrong destination radius passed validation'

    $metrics.teleport_proof.committed = $null
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof.committed was missing*' 'missing committed cohort passed validation'
    $metrics.teleport_proof.committed = '0:65:65:16'

    $overlappingCallbackMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('first_gpu_ms=1350.0000', 'first_gpu_ms=1450.0000') `
        -Kind Teleport
    $overlappingCallbackArguments = $fullViewArguments.Clone()
    $overlappingCallbackArguments.TeleportMarker = $overlappingCallbackMarker
    $metrics.teleport_proof.first_gpu_ms = 1450.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics @overlappingCallbackArguments
    $metrics.teleport_proof.first_gpu_ms = 1350.0

    $metrics.teleport_proof.stable_gpu_ms = 'NaN'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof.stable_gpu_ms was not finite*' 'nonfinite GPU-completion timestamp passed validation'
    $metrics.teleport_proof.stable_gpu_ms = 1390.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presentation timestamps were not monotonic*' 'nonmonotonic presentation timestamps passed validation'
    $metrics.teleport_proof.stable_gpu_ms = 1500.0

    $metrics.teleport_proof.stable_frame_sequence = 43
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof frame sequences were not adjacent*' 'non-adjacent presented frames passed validation'
    $metrics.teleport_proof.stable_frame_sequence = 42

    $metrics.teleport_proof.first_presented_manifest_count = 3
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presented manifest count did not equal expected*' 'partial presented manifest count passed validation'
    $metrics.teleport_proof.first_presented_manifest_count = 4
    $metrics.teleport_proof.stable_presented_manifest_hash = '9999000011112222'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presented manifest hash did not equal expected*' 'wrong presented manifest hash passed validation'
    $metrics.teleport_proof.stable_presented_manifest_hash = '1111222233334444'

    foreach ($field in @('missing_target_instances', 'unexpected_target_instances', 'source_instances', 'foreign_instances', 'stale_generation_instances', 'orphan_allocations')) {
        $metrics.forced_full_view_remesh_proof[$field] = 1
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike `
            { Assert-AcceptanceMetrics @fullViewArguments } `
            "forced_full_view_remesh_proof.$field*expected zero*" `
            "forced-remesh render counter $field passed validation"
        $metrics.forced_full_view_remesh_proof[$field] = 0
    }

    $mismatchedTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('resident_hash=aaaabbbbccccdddd', 'resident_hash=0000000000000001') `
        -Kind Teleport
    $mismatchedMarkerArguments = $fullViewArguments.Clone()
    $mismatchedMarkerArguments.TeleportMarker = $mismatchedTeleportMarker
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @mismatchedMarkerArguments } 'teleport marker/metrics mismatch for resident_hash*' 'marker/metrics mismatch passed validation'

    $overCapTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('frame_count=90', 'frame_count=92') `
        -Kind Teleport
    $overCapArguments = $fullViewArguments.Clone()
    $overCapArguments.TeleportMarker = $overCapTeleportMarker
    $metrics.teleport_proof.frame_count = 92
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @overCapArguments } 'teleport_proof exceeded its 60fps cap*' 'per-teleport interval frame cap was not enforced'
    $metrics.teleport_proof.frame_count = 90

    $lateDecodeStageMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('last_sub_ms=900.0000', 'last_sub_ms=1250.0000') `
        -Kind Teleport
    $lateDecodeStageArguments = $fullViewArguments.Clone()
    $lateDecodeStageArguments.TeleportMarker = $lateDecodeStageMarker
    $metrics.teleport_proof.last_sub_ms = 1250.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @lateDecodeStageArguments } 'teleport_proof.last_sub_ms must be JSON null or a nonnegative finite value*' 'a target decode stage after render readiness passed validation'
    $metrics.teleport_proof.last_sub_ms = 900.0

    $missingStageMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('publisher_ms=100.0000', 'publisher_ms=null') `
        -Kind Teleport
    $metrics.teleport_proof.publisher_ms = $null
    $missingStageArguments = $fullViewArguments.Clone()
    $missingStageArguments.TeleportMarker = $missingStageMarker
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics @missingStageArguments
    $metrics.teleport_proof.publisher_ms = -1.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @missingStageArguments } 'teleport_proof.publisher_ms must be JSON null or a nonnegative finite value*' 'missing stage was serialized as -1 without failing'
    $metrics.teleport_proof.publisher_ms = 100.0

    $resourceSamples = @(
        [pscustomobject]@{ combined_rss_bytes = 300MB; cpu_percent = 5.0 },
        [pscustomobject]@{ combined_rss_bytes = 400MB; cpu_percent = 10.0 },
        [pscustomobject]@{ combined_rss_bytes = 350MB; cpu_percent = 15.0 }
    )
    $resourceSummary = Get-SteadyResourceSummary -Samples $resourceSamples
    Assert-Equal (400MB) $resourceSummary.max_combined_rss_bytes 'resource summary chose the wrong RSS maximum'
    Assert-Equal 10.0 $resourceSummary.mean_cpu_percent 'resource summary chose the wrong CPU mean'
    Assert-Equal 15.0 $resourceSummary.p95_cpu_percent 'resource summary chose the wrong CPU p95'
    $resourceTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    $resourceDocument = New-SteadyResourceDocument `
        -Samples $resourceSamples `
        -DurationSeconds 30 `
        -Trigger $resourceTrigger
    Assert-Equal 'rust-mcbe-steady-resources-v2' $resourceDocument.schema 'steady-resource schema did not identify trigger provenance'
    Assert-Equal 'FullViewPresented' $resourceDocument.trigger.kind 'steady-resource trigger kind changed'
    Assert-Equal '0:65:65:16' $resourceDocument.trigger.target 'steady-resource trigger lost its exact target'
    Assert-Equal 7 $resourceDocument.trigger.teleport_view_generation 'steady-resource trigger lost teleport generation'
    Assert-Equal 42 $resourceDocument.trigger.teleport_stable_frame_sequence 'steady-resource trigger lost teleport stable frame'
    Assert-Equal 8 $resourceDocument.trigger.forced_remesh_view_generation 'steady-resource trigger lost forced-remesh generation'
    Assert-Equal 44 $resourceDocument.trigger.forced_remesh_stable_frame_sequence 'steady-resource trigger lost forced-remesh stable frame'

    $metrics.publisher_radius_chunks = 4
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'publisher radius below 16 passed validation'
    $metrics.publisher_radius_chunks = 16
    $metrics.frame_count = 0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'zero frame_count passed validation'
    $metrics.frame_count = 1
    $metrics.p99_frame_ms = 'not-finite'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'nonnumeric p99 passed validation'

}
catch {
    $testFailure = $_
}
