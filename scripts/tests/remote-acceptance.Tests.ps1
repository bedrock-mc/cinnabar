$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$ScriptPath = Join-Path $ProjectRoot 'scripts\remote-acceptance.ps1'

function New-SyntheticPhase2Publication {
    param(
        [int]$RequiredColumns,
        [int]$LoadedColumns,
        [uint64]$RequestsConstructed,
        [uint64]$RequestsSent,
        [uint64]$ResponsesAdmitted,
        [uint64]$SubchunksCommitted,
        [object]$PublisherRadiusBlocks = 128,
        [object]$PublisherRadius = 8,
        [uint64]$MeshJobsCompleted = 1,
        [int]$MeshJobsQueued = 0,
        [uint64]$UploadsAcknowledged = 1,
        [int]$UploadsUnacknowledged = 0
    )
    $hash = '1111111111111111'
    $identity = { param($count) [ordered]@{ entry_count = $count; generation_manifest_hash = $hash; required_cohort_hash = $hash; session_generation = 1 } }
    return [ordered]@{
        presentation = [ordered]@{
            build_profile = 'release'; requested_present_mode = 'fifo'; effective_present_mode = 'fifo'; present_mode_proven = $true
            graphics_identity_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            assets_manifest_sha256 = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
            publisher_disk = & $identity $LoadedColumns; resident = & $identity $LoadedColumns; allocation = & $identity $LoadedColumns
            visible = & $identity $LoadedColumns; submitted = & $identity $LoadedColumns; gpu_presented = & $identity $LoadedColumns
        }
        publication = [ordered]@{
            session_generation = 1; player_column = [ordered]@{ dimension = 0; x = 1; z = 2 }
            required_cohort_hash = $hash; required_columns = $RequiredColumns; loaded_required_columns = $LoadedColumns
            publisher_radius_blocks = $PublisherRadiusBlocks; publisher_radius_chunks = $PublisherRadius
            max_queue_wait_us = [ordered]@{ decode = 0; lighting = 0; meshing = 0 }
            max_worker_time_us = [ordered]@{ decode = 0; lighting = 0; meshing = 0 }
            outcomes = [ordered]@{ success = $SubchunksCommitted; all_air = 0; unavailable = 0; malformed = 0; stale = 0; timed_out = 0 }
            stages = [ordered]@{
                requests_constructed = $RequestsConstructed; requests_ready = 0; requests_sent = $RequestsSent
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

function New-SyntheticPhase2LunarManifest {
    param([ValidateSet('Diagnostic', 'Candidate', 'Final')][string]$Mode)
    $publication = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
        -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
    $findings = [Collections.Generic.List[string]]::new()
    if ($Mode -eq 'Diagnostic') { $findings.Add('world_ready_not_observed') }
    return [ordered]@{
        schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; upstream = 'pvp.lunarbedrock.com:19134'; mode = $Mode; status = 'passed'
        join_milliseconds = if ($Mode -eq 'Diagnostic') { $null } else { 1500.0 }
        initial_radius = 16; requested_present_mode = 'Fifo'; full_view_teleport_gate = ($Mode -ne 'Diagnostic')
        diagnostic_complete = ($Mode -eq 'Diagnostic'); behavior_gate_passed = ($Mode -ne 'Diagnostic')
        world_ready_observed = ($Mode -ne 'Diagnostic'); publication_snapshot_count = 2
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
            'PHASE2_PUBLICATION={"presentation":{"build_profile":"release","requested_present_mode":"fifo","effective_present_mode":"fifo","present_mode_proven":true,"graphics_identity_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","assets_manifest_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}' |
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
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false

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

    It 'does not classify a cohort identity gap across malformed response outcomes' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
            -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
        $record.publication.outcomes.malformed = 1
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$false) |
            Should Be 'response_semantics'
    }

    It 'classifies a complete cohort with a no-ready mesh backlog as meshing' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 4000
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$false) |
            Should Be 'meshing'
    }

    It 'requires empty downstream work before terminal none after world ready' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 4
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$true) |
            Should Be 'meshing'
    }

    It 'requires the exact raw block radius schema and derived retention radius' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-radius-schema-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $invalidValues = @($null, $true, '128', 128.5, -1, [decimal]18446744073709551616)
            $case = 0
            foreach ($invalid in $invalidValues) {
                $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                    -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894 `
                    -PublisherRadiusBlocks $invalid -PublisherRadius 8
                $path = Join-Path $temporary ("invalid-$case.log")
                'PHASE2_PUBLICATION=' + ($record | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $path
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path -ExpectedPresentMode Fifo `
                    -WorldReadyObserved:$false } | Should Throw
                $case++
            }

            $missing = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $missing.publication.Remove('publisher_radius_blocks')
            $missingPath = Join-Path $temporary 'missing.log'
            'PHASE2_PUBLICATION=' + ($missing | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $missingPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $missingPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false } | Should Throw

            $wrongDerived = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894 `
                -PublisherRadiusBlocks 120 -PublisherRadius 7
            $wrongPath = Join-Path $temporary 'wrong-derived.log'
            'PHASE2_PUBLICATION=' + ($wrongDerived | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $wrongPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $wrongPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false } | Should Throw

            foreach ($geometry in @(
                @{ blocks = 120; chunks = 8; columns = 177 },
                @{ blocks = 128; chunks = 8; columns = 197 },
                @{ blocks = 256; chunks = 16; columns = 797 }
            )) {
                $valid = New-SyntheticPhase2Publication -RequiredColumns $geometry.columns -LoadedColumns $geometry.columns `
                    -RequestsConstructed $geometry.columns -RequestsSent $geometry.columns `
                    -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
                    -PublisherRadiusBlocks $geometry.blocks -PublisherRadius $geometry.chunks
                $parsed = $valid | ConvertTo-Json -Depth 20 | ConvertFrom-Json
                { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
                    Should Not Throw
            }
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects unknown fields, inexact integral fields, and mixed sequence identities' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-strict-schema-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $unknown = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $unknown.presentation['access_token'] = 'must-not-pass'
            $unknownPath = Join-Path $temporary 'unknown.log'
            'PHASE2_PUBLICATION=' + ($unknown | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $unknownPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $unknownPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false } | Should Throw

            foreach ($invalid in @($null, $true, '1', 1.5, -1, [decimal]18446744073709551616)) {
                $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                    -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
                $record.publication.stages.requests_sent = $invalid
                $path = Join-Path $temporary ("stage-$([guid]::NewGuid().ToString('N')).log")
                'PHASE2_PUBLICATION=' + ($record | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $path
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path -ExpectedPresentMode Fifo `
                    -WorldReadyObserved:$false } | Should Throw
            }

            $first = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 1 `
                -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 22 -SubchunksCommitted 22
            $mixed = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 2 `
                -RequestsConstructed 2 -RequestsSent 2 -ResponsesAdmitted 44 -SubchunksCommitted 44
            $mixed.publication.required_cohort_hash = '2222222222222222'
            foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
                $mixed.presentation.$name.required_cohort_hash = '2222222222222222'
            }
            $mixedPath = Join-Path $temporary 'mixed.log'
            @(
                'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($mixed | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $mixedPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $mixedPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects terminal none with adversarial mesh and upload backlog' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 400000 -UploadsAcknowledged 12 -UploadsUnacknowledged 500000
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$true) |
            Should Be 'meshing'
    }

    It 'rejects incoherent stage gauges and response outcomes before terminal classification' {
        $mutations = @(
            { param($record) $record.publication.stages.decode_jobs_dispatched = 1; $record.publication.stages.decode_jobs_completed = 2 },
            { param($record) $record.publication.stages.light_jobs_dispatched = 1; $record.publication.stages.light_jobs_completed = 2 },
            { param($record) $record.publication.stages.mesh_jobs_dispatched = 1; $record.publication.stages.mesh_jobs_completed = 2 },
            { param($record) $record.publication.stages.mesh_changes_queued = 1; $record.publication.stages.mesh_changes_dequeued = 2 },
            { param($record) $record.publication.stages.mesh_changes_queued = 100; $record.publication.stages.mesh_changes_dequeued = 0; $record.publication.stages.mesh_changes_pending = 0 },
            { param($record) foreach ($name in @('success','all_air','unavailable','malformed','stale','timed_out')) { $record.publication.outcomes[$name] = 0 } },
            { param($record) $record.publication.stages.responses_admitted = 4095 },
            { param($record) $record.publication.stages.subchunks_committed = 4097 }
        )
        foreach ($mutation in $mutations) {
            $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
                -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
            & $mutation $record
            $parsed = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
                Should Throw
        }

        $adversarial = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
        foreach ($name in @('success','all_air','unavailable','malformed','stale','timed_out')) {
            $adversarial.publication.outcomes[$name] = 0
        }
        $adversarial.publication.stages.mesh_changes_queued = 100
        $adversarial.publication.stages.mesh_changes_dequeued = 0
        $adversarial.publication.stages.mesh_changes_pending = 0
        $parsedAdversarial = $adversarial | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        (Get-Phase2FirstStalledStage -PublicationRecord $parsedAdversarial -WorldReadyObserved:$true) |
            Should Not Be 'none'

        $incompleteCommit = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
        $incompleteCommit.publication.stages.subchunks_committed = 4095
        $parsedIncompleteCommit = $incompleteCommit | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        (Get-Phase2FirstStalledStage -PublicationRecord $parsedIncompleteCommit -WorldReadyObserved:$true) |
            Should Be 'response_semantics'
    }

    It 'rejects missing or incoherent publication evidence as diagnostic completeness' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-bad-diagnostic-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $emptyPath = Join-Path $temporary 'empty.log'
            Set-Content -LiteralPath $emptyPath -Value 'no publication evidence'
            { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
                -ClientLogPath $emptyPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false } | Should Throw

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
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false } | Should Throw

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
                -ClientLogPath $regressionPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'does not allow Candidate or Final to use diagnostic no-ready completion' {
        foreach ($mode in @('Candidate', 'Final')) {
            { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = $mode }) `
                -ClientLogPath 'unused.log' -ExpectedPresentMode Fifo -WorldReadyObserved:$false } | Should Throw
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
