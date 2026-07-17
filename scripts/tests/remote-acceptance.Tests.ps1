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
        [Nullable[int]]$PublisherRadius = 16,
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
            required_cohort_hash = $hash; required_columns = $RequiredColumns; loaded_required_columns = $LoadedColumns; publisher_radius_chunks = $PublisherRadius
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
    return [ordered]@{
        schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; mode = $Mode; status = 'passed'
        diagnostic_complete = ($Mode -eq 'Diagnostic'); behavior_gate_passed = ($Mode -ne 'Diagnostic')
        world_ready_observed = ($Mode -ne 'Diagnostic'); publication_snapshot_count = 2
        first_stalled_stage = if ($Mode -eq 'Diagnostic') { 'presentation' } else { 'none' }; final_publication = $publication
        metrics_evidence = [ordered]@{ status = if ($Mode -eq 'Diagnostic') { 'unavailable' } else { 'passed' } }
        resources_evidence = [ordered]@{ status = if ($Mode -eq 'Diagnostic') { 'unavailable' } else { 'passed' } }
    }
}

Describe 'Phase 2 remote acceptance runner' {
    BeforeEach {
        . (Join-Path $ProjectRoot 'scripts\acceptance\Load.ps1')
    }

    It 'enforces create-new, duration, radius, auth locality, and canonical Lunar endpoint' {
        $runId = 'pester-remote-' + [guid]::NewGuid().ToString('N')
        $runDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId"
        $candidateRunDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId-candidate"
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
        }
        finally {
            Remove-Item -LiteralPath $runDirectory -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $candidateRunDirectory -Recurse -Force -ErrorAction SilentlyContinue
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
            -PublisherRadius 8
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$false) |
            Should Be 'required_cohort_identity'
    }

    It 'classifies a complete cohort with a no-ready mesh backlog as meshing' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 4000
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$false) |
            Should Be 'meshing'
    }

    It 'does not treat bounded downstream work as a stall after world ready' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 4
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$true) |
            Should Be 'none'
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
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty

            $diagnosticPath = Join-Path $temporary 'diagnostic\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $diagnosticPath) | Out-Null
            New-SyntheticPhase2LunarManifest -Mode Diagnostic | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $diagnosticPath
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic).Path | Should Be $diagnosticPath
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Candidate) | Should BeNullOrEmpty
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Final) | Should BeNullOrEmpty

            $badCandidatePath = Join-Path $temporary 'candidate-bad\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $badCandidatePath) | Out-Null
            $badCandidate = New-SyntheticPhase2LunarManifest -Mode Candidate
            $badCandidate.first_stalled_stage = 'meshing'
            $badCandidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $badCandidatePath
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Candidate) | Should BeNullOrEmpty

            $candidatePath = Join-Path $temporary 'candidate\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $candidatePath) | Out-Null
            New-SyntheticPhase2LunarManifest -Mode Candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $candidatePath
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Candidate).Path | Should Be $candidatePath
            (Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Final) | Should BeNullOrEmpty

            $finalPath = Join-Path $temporary 'final\manifest.json'
            New-Item -ItemType Directory -Path (Split-Path -Parent $finalPath) | Out-Null
            New-SyntheticPhase2LunarManifest -Mode Final | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $finalPath
            $result = Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Final
            $result.Path | Should Be $finalPath
            $result.Mode | Should Be 'Final'
            $result.Sha256 | Should Match '^[0-9A-F]{64}$'
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
