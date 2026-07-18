$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$ScriptPath = Join-Path $ProjectRoot 'scripts\remote-acceptance.ps1'

function New-SyntheticPhase2Publication {
    param(
        [int]$RequiredColumns,
        [int]$LoadedColumns,
        [uint64]$RequestsConstructed,
        [uint64]$RequestsSent,
        [uint64]$RequestsTransportPending = 0,
        [uint64]$ResponsesAdmitted,
        [uint64]$SubchunksCommitted,
        [object]$PublisherRadiusBlocks = 128,
        [object]$PublisherRadius = 8,
        [uint64]$PublisherEpoch = 1,
        [bool]$RequiredCohortStable = $true,
        [uint64]$MeshJobsCompleted = 1,
        [int]$MeshJobsQueued = 0,
        [uint64]$UploadsAcknowledged = 1,
        [int]$UploadsUnacknowledged = 0
    )
    $hash = '1111111111111111'
    $identity = { param($count) [ordered]@{
        entry_count = $count; generation_manifest_hash = $hash; publisher_epoch = $PublisherEpoch
        required_cohort_count = $RequiredColumns; required_cohort_hash = $hash; session_generation = 1
    } }
    return [ordered]@{
        client_blob_cache_enabled = $true
        client_blob_cache = [ordered]@{
            hashes_classified = 0; hits = 0; misses = 0; admitted_blobs = 0; rejected_blobs = 0
            evictions = 0; pending_transactions = 0; pending_bytes = 0; pending_resets = 0
            reconstructed_level_chunks = 0; reconstructed_sub_chunks = 0
        }
        presentation = [ordered]@{
            build_profile = 'release'; requested_present_mode = 'fifo'; effective_present_mode = 'fifo'; present_mode_proven = $true
            graphics_identity_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            assets_manifest_sha256 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
            publisher_disk = & $identity $LoadedColumns; resident = & $identity $LoadedColumns; allocation = & $identity $LoadedColumns
            visible = & $identity $LoadedColumns; submitted = & $identity $LoadedColumns; gpu_presented = & $identity $LoadedColumns
        }
        publication = [ordered]@{
            session_generation = 1; player_column = [ordered]@{ dimension = 0; x = 1; z = 2 }
            publisher_epoch = $PublisherEpoch; required_cohort_hash = $hash; required_columns = $RequiredColumns
            loaded_required_columns = $LoadedColumns; required_cohort_stable = $RequiredCohortStable
            publisher_radius_blocks = $PublisherRadiusBlocks; publisher_radius_chunks = $PublisherRadius
            max_queue_wait_us = [ordered]@{ decode = 0; lighting = 0; meshing = 0 }
            max_worker_time_us = [ordered]@{ decode = 0; lighting = 0; meshing = 0 }
            outcomes = [ordered]@{ success = $SubchunksCommitted; all_air = 0; unavailable = 0; malformed = 0; stale = 0; timed_out = 0 }
            stages = [ordered]@{
                requests_constructed = $RequestsConstructed; requests_ready = 0
                requests_transport_pending = $RequestsTransportPending; requests_sent = $RequestsSent
                responses_admitted = $ResponsesAdmitted; subchunks_awaiting_response = 0; subchunks_committed = $SubchunksCommitted
                decode_jobs_queued = 0; decode_jobs_dispatched = $SubchunksCommitted; decode_jobs_in_flight = 0; decode_jobs_completed = $SubchunksCommitted
                light_jobs_queued = 0; light_jobs_dispatched = $SubchunksCommitted; light_jobs_in_flight = 0; light_jobs_completed = $SubchunksCommitted
                mesh_changes_queued = 0; mesh_changes_pending = 0; mesh_changes_dequeued = 0
                mesh_jobs_queued = $MeshJobsQueued; mesh_jobs_dispatched = $MeshJobsCompleted; mesh_jobs_in_flight = 0; mesh_jobs_completed = $MeshJobsCompleted
                mesh_uploads_unacknowledged = $UploadsUnacknowledged; mesh_uploads_acknowledged = $UploadsAcknowledged
            }
        }
    }
}

function New-SyntheticPhase2PublisherInitialization {
    param(
        [string]$RenderGenerationManifestHash = '00000000000000e1'
    )

    $record = New-SyntheticPhase2Publication -RequiredColumns 0 -LoadedColumns 0 `
        -RequestsConstructed 0 -RequestsSent 0 -ResponsesAdmitted 0 -SubchunksCommitted 0 `
        -PublisherRadiusBlocks $null -PublisherRadius $null -MeshJobsCompleted 0 `
        -UploadsAcknowledged 0
    $emptyManifestHash = 'cbf29ce484222325'
    $record.publication.required_cohort_hash = $emptyManifestHash
    $record.publication.publisher_epoch = 0
    $record.publication.required_cohort_stable = $false
    foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
        $record.presentation.$name.required_cohort_hash = $emptyManifestHash
        $record.presentation.$name.publisher_epoch = 0
        $record.presentation.$name.required_cohort_count = 0
        $record.presentation.$name.entry_count = 0
    }
    foreach ($name in @('publisher_disk', 'allocation')) {
        $record.presentation.$name.generation_manifest_hash = $emptyManifestHash
    }
    foreach ($name in @('resident', 'visible', 'submitted', 'gpu_presented')) {
        $record.presentation.$name.generation_manifest_hash = $RenderGenerationManifestHash
    }
    return $record
}

function New-SyntheticPhase2LunarManifest {
    param([ValidateSet('Diagnostic', 'Candidate', 'Final')][string]$Mode)
    $publication = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
        -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
    $publication.client_blob_cache.hashes_classified = 1
    $publication.client_blob_cache.hits = 1
    $findings = [Collections.Generic.List[string]]::new()
    if ($Mode -eq 'Diagnostic') { $findings.Add('world_ready_not_observed') }
    return [ordered]@{
        schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; upstream = 'pvp.lunarbedrock.com:19134'; mode = $Mode; status = 'passed'
        join_milliseconds = if ($Mode -eq 'Diagnostic') { $null } else { 1500.0 }
        initial_radius = 16; requested_present_mode = 'Fifo'; full_view_teleport_gate = ($Mode -ne 'Diagnostic')
        diagnostic_complete = ($Mode -eq 'Diagnostic'); behavior_gate_passed = ($Mode -ne 'Diagnostic')
        world_ready_observed = ($Mode -ne 'Diagnostic'); publication_snapshot_count = 2
        client_blob_cache_route = 'cache_backed'
        cache_boundary_evidence = [ordered]@{
            classification = 'cache_backed'; upstream_status_seen = $true; upstream_status_enabled = $true
            cached_level_chunks = 1; ordinary_level_chunks = 0; cached_sub_chunks = 1; ordinary_sub_chunks = 0
        }
        first_stalled_stage = if ($Mode -eq 'Diagnostic') { 'presentation' } else { 'none' }; final_publication = $publication
        findings = $findings
        metrics_evidence = [ordered]@{ status = if ($Mode -eq 'Diagnostic') { 'unavailable' } else { 'passed' }; reason = if ($Mode -eq 'Diagnostic') { 'world_ready_not_observed' } else { $null } }
        resources_evidence = [ordered]@{ status = if ($Mode -eq 'Diagnostic') { 'unavailable' } else { 'passed' }; reason = if ($Mode -eq 'Diagnostic') { 'world_ready_not_observed' } else { $null } }
        duration_seconds = 150
        require_effective_present_mode_proof = $true; require_release_build = $true
        auth_cache_scope = '.local'; client_arguments = @('--synthetic')
        performance = [ordered]@{
            warmup_seconds = 30; steady_seconds = 120; p95_frame_ms_max = 16.6666666667
            p99_frame_ms_max = 16.6666666667; max_frame_ms_max = 50.0; lifecycle_ms_max = 2000.0
            resource_sample_count = 120; max_combined_rss_bytes = 681574400
            mean_cpu_percent_max = 15.0; p95_cpu_percent_max = 15.0
        }
        client_shutdown_grace_seconds = 5
        lunar_prerequisite_mode = $null; lunar_prerequisite_manifest_sha256 = $null
    }
}

function Find-SyntheticPhase2LunarPrerequisite {
    param(
        [string]$RemoteRoot,
        [ValidateSet('Diagnostic', 'Candidate', 'Final')][string]$Mode,
        [switch]$RequireFullView
    )
    return Find-Phase2CompletedLunarPrerequisite -RemoteRoot $RemoteRoot -Mode $Mode `
        -ExpectedPresentMode Fifo -ExpectedInitialRadius 16 -RequireFullView:$RequireFullView
}

Describe 'Phase 2 remote acceptance runner' {
    BeforeEach {
        . (Join-Path $ProjectRoot 'scripts\acceptance\Load.ps1')
    }

    It 'enforces create-new, duration, radius, auth locality, and canonical Lunar endpoint' {
        $runId = 'pester-remote-' + [guid]::NewGuid().ToString('N')
        $runDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId"
        $candidateRunDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId-candidate"
        $immediateRunDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId-immediate"
        try {
            & $ScriptPath -Server Lunar -Mode Diagnostic -RunId $runId -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly
            $manifest = Get-Content -Raw -LiteralPath (Join-Path $runDirectory 'manifest.json') | ConvertFrom-Json
            $manifest.upstream | Should Be 'pvp.lunarbedrock.com:19134'
            $manifest.initial_radius | Should Be 16
            @($manifest.client_arguments) -join ' ' | Should Not Match 'initial.radius'
            $manifest.performance.warmup_seconds | Should Be 30
            $manifest.performance.resource_sample_count | Should Be 120

            & $ScriptPath -Server Lunar -Mode Candidate -RunId ($runId + '-candidate') -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly
            $candidateManifest = Get-Content -Raw -LiteralPath (Join-Path $candidateRunDirectory 'manifest.json') | ConvertFrom-Json
            $candidateManifest.diagnostic_complete | Should Be $false

            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId $runId -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-short') -DurationSeconds 149 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-radius') -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 15 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-auth') -DurationSeconds 150 `
                -AuthCache '..\token.json' -InitialRadius 16 -PresentMode Fifo `
                -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-immediate') -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 -PresentMode Immediate `
                -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $runDirectory -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $candidateRunDirectory -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $immediateRunDirectory -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects incomplete release, presentation, frame, lifecycle, and resource evidence' {
        . (Join-Path $ProjectRoot 'scripts\acceptance\Load.ps1')
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-evidence-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $metricsPath = Join-Path $temporary 'metrics.json'
            $resourcesPath = Join-Path $temporary 'resources.json'
            $logPath = Join-Path $temporary 'client.log'
            @{ p95_frame_ms = 16.0; p99_frame_ms = 16.5; max_frame_ms = 49.0; teleport_settle_ms = $null; forced_full_view_remesh_ms = $null } |
                ConvertTo-Json | Set-Content -LiteralPath $metricsPath
            $samples = @(1..120 | ForEach-Object { @{ elapsed_seconds = $_; combined_rss_bytes = 104857600; cpu_percent = 5.0 } })
            @{ schema = 'rust-mcbe-phase2-resources-v1'; warmup_seconds = 30; duration_seconds = 120; processor_count = 8; samples = $samples; summary = @{ sample_count = 120; max_combined_rss_bytes = 104857600; mean_cpu_percent = 5.0; p95_cpu_percent = 5.0 } } |
                ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $resourcesPath
            $publication = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
                -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
            'PHASE2_PUBLICATION=' + ($publication | ConvertTo-Json -Depth 20 -Compress) |
                Set-Content -LiteralPath $logPath
            { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Not Throw
            (Get-Content -Raw -LiteralPath $metricsPath).Replace('16.5', '17.0') | Set-Content -LiteralPath $metricsPath
            { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Throw

            foreach ($malformed in @($null, 'NaN', $true, -1.0)) {
                @{ p95_frame_ms = $malformed; p99_frame_ms = 16.5; max_frame_ms = 49.0 } |
                    ConvertTo-Json | Set-Content -LiteralPath $metricsPath
                { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                    -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Throw
            }

            @{ p95_frame_ms = 16.0; p99_frame_ms = 16.5; max_frame_ms = 49.0 } |
                ConvertTo-Json | Set-Content -LiteralPath $metricsPath
            (Get-Content -Raw -LiteralPath $logPath).Replace('"present_mode_proven":true', '"present_mode_proven":false') |
                Set-Content -LiteralPath $logPath
            { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }


    It 'completes a no-ready Diagnostic from coherent attributable publication evidence' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-no-ready-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $logPath = Join-Path $temporary 'client.log'
            $first = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 0 `
                -RequestsConstructed 0 -RequestsSent 0 -ResponsesAdmitted 0 -SubchunksCommitted 0
            $last = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894 `
                -PublisherRadius 8 -MeshJobsCompleted 998 -MeshJobsQueued 4033 `
                -UploadsAcknowledged 978 -UploadsUnacknowledged 5046
            @(
                'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($last | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $logPath
            $manifest = [pscustomobject][ordered]@{ mode = 'Diagnostic'; initial_radius = 16 }

            Complete-Phase2DiagnosticEvidence -Manifest $manifest -ClientLogPath $logPath `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa

            $manifest.status | Should Be 'passed'
            $manifest.diagnostic_complete | Should Be $true
            $manifest.behavior_gate_passed | Should Be $false
            $manifest.world_ready_observed | Should Be $false
            $manifest.join_milliseconds | Should BeNullOrEmpty
            $manifest.publication_snapshot_count | Should Be 2
            $manifest.first_stalled_stage | Should Be 'required_cohort_identity'
            $manifest.final_publication.publication.loaded_required_columns | Should Be 177
            (@($manifest.findings) -join ',') | Should Match 'world_ready_not_observed'
            $manifest.metrics_evidence.status | Should Be 'unavailable'
            $manifest.resources_evidence.status | Should Be 'unavailable'
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'accepts multiple coherent leading publisher initialization snapshots with evolving empty render generations' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-publisher-initialization-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $logPath = Join-Path $temporary 'client.log'
            $initializing = New-SyntheticPhase2PublisherInitialization
            $stillInitializing = New-SyntheticPhase2PublisherInitialization `
                -RenderGenerationManifestHash '00000000000000e3'
            $initialized = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            @(
                'PHASE2_PUBLICATION=' + ($initializing | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($stillInitializing | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($initialized | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $logPath

            $evidence = Get-Phase2PublicationSequenceEvidence -ClientLogPath $logPath `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa

            $evidence.SnapshotCount | Should Be 3
            $evidence.FinalPublication.publication.publisher_radius_blocks | Should Be 128
            $evidence.FinalPublication.publication.publisher_radius_chunks | Should Be 8
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects unsafe publisher initialization state and attribution changes' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-publisher-initialization-rejection-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $initialized = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $writeAndReject = {
                param([string]$Name, [object[]]$Records)
                $path = Join-Path $temporary ($Name + '.log')
                @($Records | ForEach-Object {
                    'PHASE2_PUBLICATION=' + ($_ | ConvertTo-Json -Depth 20 -Compress)
                }) | Set-Content -LiteralPath $path
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                    -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } |
                    Should Throw
            }

            $blocksNull = New-SyntheticPhase2PublisherInitialization
            $blocksNull.publication.publisher_radius_chunks = 8
            & $writeAndReject 'blocks-null-only' @($blocksNull, $initialized)

            $chunksNull = New-SyntheticPhase2PublisherInitialization
            $chunksNull.publication.publisher_radius_blocks = 128
            & $writeAndReject 'chunks-null-only' @($chunksNull, $initialized)

            $nonemptyCohort = New-SyntheticPhase2PublisherInitialization
            $nonemptyCohort.publication.required_columns = 1
            & $writeAndReject 'nonempty-cohort' @($nonemptyCohort, $initialized)

            $stageProgress = New-SyntheticPhase2PublisherInitialization
            $stageProgress.publication.stages.requests_constructed = 1
            $stageProgress.publication.stages.requests_ready = 1
            & $writeAndReject 'stage-progress' @($stageProgress, $initialized)

            $outcomeProgress = New-SyntheticPhase2PublisherInitialization
            $outcomeProgress.publication.stages.responses_admitted = 1
            $outcomeProgress.publication.outcomes.malformed = 1
            & $writeAndReject 'outcome-progress' @($outcomeProgress, $initialized)

            $presentedEntry = New-SyntheticPhase2PublisherInitialization
            $presentedEntry.presentation.visible.entry_count = 1
            & $writeAndReject 'presented-entry' @($presentedEntry, $initialized)

            $timedWork = New-SyntheticPhase2PublisherInitialization
            $timedWork.publication.max_queue_wait_us.decode = 1
            & $writeAndReject 'queue-timing' @($timedWork, $initialized)

            $workerTiming = New-SyntheticPhase2PublisherInitialization
            $workerTiming.publication.max_worker_time_us.meshing = 1
            & $writeAndReject 'worker-timing' @($workerTiming, $initialized)

            $noncanonicalCohort = New-SyntheticPhase2PublisherInitialization
            $noncanonicalCohort.publication.required_cohort_hash = 'deadbeefdeadbeef'
            foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
                $noncanonicalCohort.presentation.$name.required_cohort_hash = 'deadbeefdeadbeef'
            }
            & $writeAndReject 'noncanonical-cohort' @($noncanonicalCohort, $initialized)

            $noncanonicalManifest = New-SyntheticPhase2PublisherInitialization
            $noncanonicalManifest.presentation.publisher_disk.generation_manifest_hash = 'feedfacefeedface'
            & $writeAndReject 'noncanonical-empty-manifest' @($noncanonicalManifest, $initialized)

            $replacementSession = New-SyntheticPhase2PublisherInitialization
            $replacementSession.publication.session_generation = 2
            foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
                $replacementSession.presentation.$name.session_generation = 2
            }
            & $writeAndReject 'session-replacement' @(
                (New-SyntheticPhase2PublisherInitialization), $replacementSession, $initialized
            )

            $cacheDisabled = New-SyntheticPhase2PublisherInitialization
            $cacheDisabled.client_blob_cache_enabled = $false
            & $writeAndReject 'cache-enablement-change' @($cacheDisabled, $initialized)

            foreach ($presentationChange in @('build', 'requested_mode', 'effective_mode', 'proof', 'graphics', 'assets')) {
                $changed = New-SyntheticPhase2PublisherInitialization
                switch ($presentationChange) {
                    'build' { $changed.presentation.build_profile = 'debug' }
                    'requested_mode' { $changed.presentation.requested_present_mode = 'immediate' }
                    'effective_mode' { $changed.presentation.effective_present_mode = 'immediate' }
                    'proof' { $changed.presentation.present_mode_proven = $false }
                    'graphics' { $changed.presentation.graphics_identity_sha256 = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc' }
                    'assets' { $changed.presentation.assets_manifest_sha256 = 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd' }
                }
                & $writeAndReject ("presentation-$presentationChange") @(
                    (New-SyntheticPhase2PublisherInitialization), $changed, $initialized
                )
            }

            & $writeAndReject 'null-after-initialized' @(
                $initialized, (New-SyntheticPhase2PublisherInitialization)
            )
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'binds evolving publisher-epoch membership to every presentation identity' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-publisher-epoch-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $path = Join-Path $temporary 'client.log'
            $first = New-SyntheticPhase2Publication -RequiredColumns 1 -LoadedColumns 1 `
                -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 1 -SubchunksCommitted 1
            $expanded = New-SyntheticPhase2Publication -RequiredColumns 2 -LoadedColumns 2 `
                -RequestsConstructed 2 -RequestsSent 2 -ResponsesAdmitted 2 -SubchunksCommitted 2
            $expanded.publication.required_cohort_hash = '2222222222222222'
            foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
                $expanded.presentation.$name.required_cohort_count = 2
                $expanded.presentation.$name.required_cohort_hash = '2222222222222222'
            }
            @($first, $expanded) | ForEach-Object {
                'PHASE2_PUBLICATION=' + ($_ | ConvertTo-Json -Depth 20 -Compress)
            } | Set-Content -LiteralPath $path

            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } |
                Should Not Throw

            $expanded.presentation.gpu_presented.publisher_epoch = 2
            'PHASE2_PUBLICATION=' + ($expanded | ConvertTo-Json -Depth 20 -Compress) |
                Set-Content -LiteralPath $path
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } |
                Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'does not report terminal readiness before the required cohort is stable' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 1 -LoadedColumns 1 `
            -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 1 -SubchunksCommitted 1 `
            -RequiredCohortStable:$false
        (Get-Phase2FirstStalledStage -PublicationRecord $record -WorldReadyObserved:$true) |
            Should Be 'required_cohort_identity'
    }

    It 'segments cohort monotonicity across publisher and dimension epoch resets' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-epoch-reset-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $path = Join-Path $temporary 'client.log'
            $first = New-SyntheticPhase2Publication -RequiredColumns 2 -LoadedColumns 2 `
                -RequestsConstructed 3 -RequestsSent 2 -ResponsesAdmitted 2 -SubchunksCommitted 2 `
                -PublisherEpoch 1
            $first.publication.stages.requests_ready = 1
            $moved = New-SyntheticPhase2Publication -RequiredColumns 1 -LoadedColumns 1 `
                -RequestsConstructed 3 -RequestsSent 2 -ResponsesAdmitted 2 -SubchunksCommitted 2 `
                -PublisherEpoch 2

            $dimensionReset = $moved | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            $dimensionReset.publication.player_column.dimension = 1
            $dimensionReset.publication.publisher_radius_blocks = $null
            $dimensionReset.publication.publisher_radius_chunks = $null
            $dimensionReset.publication.required_columns = 0
            $dimensionReset.publication.loaded_required_columns = 0
            $dimensionReset.publication.required_cohort_hash = 'cbf29ce484222325'
            $dimensionReset.publication.required_cohort_stable = $false
            foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
                $dimensionReset.presentation.$name.entry_count = 0
                $dimensionReset.presentation.$name.required_cohort_count = 0
                $dimensionReset.presentation.$name.required_cohort_hash = 'cbf29ce484222325'
                $dimensionReset.presentation.$name.generation_manifest_hash = 'cbf29ce484222325'
            }

            $newDimension = New-SyntheticPhase2Publication -RequiredColumns 1 -LoadedColumns 1 `
                -RequestsConstructed 4 -RequestsSent 3 -ResponsesAdmitted 3 -SubchunksCommitted 3 `
                -PublisherEpoch 3
            $newDimension.publication.player_column.dimension = 1
            @(
                foreach ($record in @($first, $moved, $dimensionReset, $newDimension)) {
                    'PHASE2_PUBLICATION=' + ($record | ConvertTo-Json -Depth 20 -Compress)
                }
            ) | Set-Content -LiteralPath $path

            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } |
                Should Not Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'allows the server publisher radius to differ without hiding a cached cohort identity gap' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
            -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894 `
            -PublisherRadiusBlocks 120 -PublisherRadius 8
        $parsed = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
            Should Not Throw
        (Get-Phase2FirstStalledStage -PublicationRecord $parsed -WorldReadyObserved:$false) |
            Should Be 'required_cohort_identity'
        (Get-Phase2FirstStalledStage -PublicationRecord $parsed -WorldReadyObserved:$true) |
            Should Be 'required_cohort_identity'
    }

    It 'accepts exact client blob cache publication fields' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
            -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
        $record['client_blob_cache_enabled'] = $true
        $record['client_blob_cache'] = [ordered]@{
            hashes_classified = 7; hits = 3; misses = 4; admitted_blobs = 4
            rejected_blobs = 0; evictions = 0; pending_transactions = 2; pending_bytes = 1024
            pending_resets = 0; reconstructed_level_chunks = 2; reconstructed_sub_chunks = 1
        }
        $parsed = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json

        { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
            Should Not Throw

        $missing = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $missing.PSObject.Properties.Remove('client_blob_cache_enabled')
        { Assert-Phase2PublicationRecord -Record $missing -ExpectedPresentMode Fifo } | Should Throw

        $unknown = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $unknown.client_blob_cache | Add-Member -NotePropertyName payload -NotePropertyValue 'forbidden'
        { Assert-Phase2PublicationRecord -Record $unknown -ExpectedPresentMode Fifo } | Should Throw

        $mismatch = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $mismatch.client_blob_cache.misses = 5
        { Assert-Phase2PublicationRecord -Record $mismatch -ExpectedPresentMode Fifo } | Should Throw

        $wrongBoolean = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $wrongBoolean.client_blob_cache_enabled = 'true'
        { Assert-Phase2PublicationRecord -Record $wrongBoolean -ExpectedPresentMode Fifo } | Should Throw

        $negative = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $negative.client_blob_cache.pending_bytes = -1
        { Assert-Phase2PublicationRecord -Record $negative -ExpectedPresentMode Fifo } | Should Throw

        $disabled = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $disabled.client_blob_cache_enabled = $false
        { Assert-Phase2PublicationRecord -Record $disabled -ExpectedPresentMode Fifo } | Should Throw
    }

    It 'enforces server-specific terminal client blob cache routes' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-cache-route-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $path = Join-Path $temporary 'route.log'
            $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $write = {
                param($value)
                'PHASE2_PUBLICATION=' + ($value | ConvertTo-Json -Depth 20 -Compress) |
                    Set-Content -LiteralPath $path
            }

            $record.client_blob_cache.hashes_classified = 7
            $record.client_blob_cache.hits = 3
            $record.client_blob_cache.misses = 4
            $record.client_blob_cache.admitted_blobs = 4
            & $write $record
            $lunar = Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Lunar
            $lunar.ClientBlobCacheRoute | Should Be 'cache_backed'

            foreach ($mutation in @('disabled', 'idle', 'rejected', 'pending_transactions', 'pending_bytes')) {
                $invalid = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
                switch ($mutation) {
                    'disabled' {
                        $invalid.client_blob_cache_enabled = $false
                        foreach ($field in @('hashes_classified', 'hits', 'misses', 'admitted_blobs')) {
                            $invalid.client_blob_cache.$field = 0
                        }
                    }
                    'idle' {
                        foreach ($field in @('hashes_classified', 'hits', 'misses', 'admitted_blobs')) {
                            $invalid.client_blob_cache.$field = 0
                        }
                    }
                    'rejected' { $invalid.client_blob_cache.rejected_blobs = 1 }
                    'pending_transactions' { $invalid.client_blob_cache.pending_transactions = 1 }
                    'pending_bytes' { $invalid.client_blob_cache.pending_bytes = 1 }
                }
                & $write $invalid
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                    -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Lunar } | Should Throw
            }

            $ordinary = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            & $write $ordinary
            $zeqaOrdinary = Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa
            $zeqaOrdinary.ClientBlobCacheRoute | Should Be 'ordinary_payload'

            & $write $record
            $zeqaCached = Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa
            $zeqaCached.ClientBlobCacheRoute | Should Be 'cache_backed'

            foreach ($field in @('rejected_blobs', 'pending_transactions', 'pending_bytes')) {
                $invalid = $ordinary | ConvertTo-Json -Depth 20 | ConvertFrom-Json
                $invalid.client_blob_cache.$field = 1
                & $write $invalid
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path `
                    -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw
            }
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    . (Join-Path $PSScriptRoot 'remote-acceptance\CacheCases.ps1')
    It 'rejects missing or incoherent publication evidence as diagnostic completeness' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-bad-diagnostic-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $emptyPath = Join-Path $temporary 'empty.log'
            Set-Content -LiteralPath $emptyPath -Value 'no publication evidence'
            { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
                -ClientLogPath $emptyPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw

            $logPath = Join-Path $temporary 'incoherent.log'
            $first = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 1 `
                -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 1 -SubchunksCommitted 1
            $last = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 2 `
                -RequestsConstructed 2 -RequestsSent 2 -ResponsesAdmitted 2 -SubchunksCommitted 2
            $last.presentation.graphics_identity_sha256 = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
            @(
                'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($last | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $logPath
            { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw

            $regressionPath = Join-Path $temporary 'regression.log'
            $first.presentation.graphics_identity_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            $last.presentation.graphics_identity_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            $first.publication.stages.requests_constructed = 10
            $last.publication.stages.requests_constructed = 9
            @(
                'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($last | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $regressionPath
            { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
                -ClientLogPath $regressionPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'does not allow Candidate or Final to use diagnostic no-ready completion' {
        foreach ($mode in @('Candidate', 'Final')) {
            { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = $mode }) `
                -ClientLogPath 'unused.log' -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw
        }
    }

    It 'routes only Diagnostic marker timeouts to non-binding completion and uses mode-matched Zeqa prerequisites' {
        $source = Get-Content -Raw -LiteralPath $ScriptPath
        $source | Should Match 'Find-Phase2CompletedLunarPrerequisite\s+-RemoteRoot\s+\$remoteRoot\s+-Mode\s+\$Mode'
        $source | Should Match "if \(\`$Mode -cne 'Diagnostic'\) \{ throw \}"
        $source | Should Match '\$clientHandle\.Process\.HasExited\s+-or\s+\$coreHandle\.Process\.HasExited'
        $source | Should Match "if \(\`$Mode -cne 'Diagnostic' -and -not \`$FullViewTeleportGate\)"
        $source | Should Match 'Complete-Phase2DiagnosticEvidence'
        $source | Should Match "\`$manifest\.behavior_gate_passed = \(\`$Mode -cne 'Diagnostic'\)"
        $source | Should Match "\`$manifest\.world_ready_observed = \`$true"
        $source | Should Match "if \(\`$Mode -cne 'Diagnostic' -and\s+\`$publicationSequence\.FirstStalledStage -cne 'none'\)"
    }

    It 'requires a complete mode-matched Lunar manifest for every Zeqa mode' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-lunar-gate-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $skeletalPath = Join-Path $temporary 'skeletal\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $skeletalPath) | Out-Null
            @{ schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; mode = 'Diagnostic'; status = 'passed'; diagnostic_complete = $true } |
                ConvertTo-Json | Set-Content -LiteralPath $skeletalPath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty

            $immediatePath = Join-Path $temporary 'immediate\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $immediatePath) | Out-Null
            $immediate = New-SyntheticPhase2LunarManifest -Mode Diagnostic
            $immediate.requested_present_mode = 'Immediate'
            $immediate.final_publication.presentation.requested_present_mode = 'immediate'
            $immediate.final_publication.presentation.effective_present_mode = 'immediate'
            $immediate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $immediatePath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty
            { Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic `
                -ExpectedPresentMode Immediate -ExpectedInitialRadius 16 -RequireFullView:$false } | Should Throw

            $diagnosticPath = Join-Path $temporary 'diagnostic\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $diagnosticPath) | Out-Null
            New-SyntheticPhase2LunarManifest -Mode Diagnostic | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $diagnosticPath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic).Path | Should Be $diagnosticPath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView) | Should BeNullOrEmpty

            $noFullViewPath = Join-Path $temporary 'candidate-no-full-view\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $noFullViewPath) | Out-Null
            $noFullView = New-SyntheticPhase2LunarManifest -Mode Candidate
            $noFullView.full_view_teleport_gate = $false
            $noFullView | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $noFullViewPath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty

            $badCandidatePath = Join-Path $temporary 'candidate-bad\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $badCandidatePath) | Out-Null
            $badCandidate = New-SyntheticPhase2LunarManifest -Mode Candidate
            $badCandidate.first_stalled_stage = 'meshing'
            $badCandidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $badCandidatePath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty

            $candidatePath = Join-Path $temporary 'candidate\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $candidatePath) | Out-Null
            New-SyntheticPhase2LunarManifest -Mode Candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $candidatePath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView).Path | Should Be $candidatePath
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView) | Should BeNullOrEmpty

            $finalPath = Join-Path $temporary 'final\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $finalPath) | Out-Null
            New-SyntheticPhase2LunarManifest -Mode Final | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $finalPath
            $result = Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView
            $result.Path | Should Be $finalPath
            $result.Mode | Should Be 'Final'
            $result.Sha256 | Should Match '^[0-9A-F]{64}$'
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'requires an exact completed Lunar manifest schema and exact integral numeric fields' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-lunar-schema-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $case = 0
            $invalidManifests = [Collections.Generic.List[object]]::new()

            $unknownRoot = New-SyntheticPhase2LunarManifest -Mode Candidate
            $unknownRoot['access_token'] = 'must-not-pass'
            $invalidManifests.Add($unknownRoot)
            $unknownMetrics = New-SyntheticPhase2LunarManifest -Mode Candidate
            $unknownMetrics.metrics_evidence['access_token'] = 'must-not-pass'
            $invalidManifests.Add($unknownMetrics)
            $missingResource = New-SyntheticPhase2LunarManifest -Mode Candidate
            $missingResource.resources_evidence.Remove('reason')
            $invalidManifests.Add($missingResource)
            $unknownPerformance = New-SyntheticPhase2LunarManifest -Mode Candidate
            $unknownPerformance.performance['extra'] = 1
            $invalidManifests.Add($unknownPerformance)

            $missingRoot = New-SyntheticPhase2LunarManifest -Mode Candidate
            $missingRoot.Remove('duration_seconds')
            $invalidManifests.Add($missingRoot)
            $missingPerformance = New-SyntheticPhase2LunarManifest -Mode Candidate
            $missingPerformance.performance.Remove('steady_seconds')
            $invalidManifests.Add($missingPerformance)

            $rootIntegralFields = [ordered]@{
                initial_radius = 16; publication_snapshot_count = 2
                duration_seconds = 150; client_shutdown_grace_seconds = 5
            }
            foreach ($field in $rootIntegralFields.Keys) {
                $expected = $rootIntegralFields[$field]
                foreach ($invalid in @($null, $true, [string]$expected, ([double]$expected + 0.5), -1, [decimal]18446744073709551616)) {
                    $manifest = New-SyntheticPhase2LunarManifest -Mode Candidate
                    $manifest[$field] = $invalid
                    $invalidManifests.Add($manifest)
                }
            }
            $performanceIntegralFields = [ordered]@{
                warmup_seconds = 30; steady_seconds = 120; resource_sample_count = 120
                max_combined_rss_bytes = 681574400
            }
            foreach ($field in $performanceIntegralFields.Keys) {
                $expected = $performanceIntegralFields[$field]
                foreach ($invalid in @($null, $true, [string]$expected, ([double]$expected + 0.5), -1, [decimal]18446744073709551616)) {
                    $manifest = New-SyntheticPhase2LunarManifest -Mode Candidate
                    $manifest.performance[$field] = $invalid
                    $invalidManifests.Add($manifest)
                }
            }

            foreach ($manifest in $invalidManifests) {
                $root = Join-Path $temporary "case-$case"
                New-Item -ItemType Directory -Path $root | Out-Null
                $manifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $root 'manifest.json')
                (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $root -Mode Candidate -RequireFullView) |
                    Should BeNullOrEmpty
                $case++
            }
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
