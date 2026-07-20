Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Describe 'FastTransferWitness focused LBSG acceptance' {
    BeforeAll {
        $script:RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
        . (Join-Path $script:RepoRoot 'scripts\acceptance\Phase3Launch.ps1')
        . (Join-Path $script:RepoRoot 'scripts\acceptance\FastTransferWitnessValidation.ps1')
        $script:RealPhase2ResetHelper = (Get-Command Get-Phase2LocalResetSequenceEvidence).ScriptBlock
        $script:TempRoot = Join-Path ([IO.Path]::GetTempPath()) "fast-transfer-witness-$PID-$([guid]::NewGuid().ToString('N'))"
        New-Item -ItemType Directory -Path $script:TempRoot | Out-Null
    }

    AfterAll {
        Remove-Item -LiteralPath $script:TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    }

    BeforeEach {
        $script:BuildCommit = '0123456789abcdef0123456789abcdef01234567'
        $script:PregSha256 = '11' * 32
        $script:BregSha256 = '22' * 32
        $script:CoreSha256 = '33' * 32
        $script:AppSha256 = '44' * 32
        $script:AssetsSha256 = '55' * 32
        $script:RunId = '0123456789abcdef0123456789abcdef'
        $script:BridgeEndpoint = '127.0.0.1:19133'
        $script:Identity = [ordered]@{
            schema = 'rust-mcbe-phase3-identity-v1'; build_commit = $script:BuildCommit
            target = 'Lbsg'; protocol = 1001; session_generation = 7
            preg_sha256 = $script:PregSha256; breg_sha256 = $script:BregSha256
            candidate_physics = $true; source_dirty = $false; run_id = $script:RunId
            endpoint = 'play.lbsg.net:19132'; bridge_endpoint = $script:BridgeEndpoint
            core_sha256 = $script:CoreSha256; core_process_id = 41; app_process_id = 42
        }
        $script:Terminal = [ordered]@{
            schema = 'rust-mcbe-phase3-terminal-v1'; session_generation = 7; source = 'Physics'
            physics_packet_count = 9; free_camera_packet_count = 0; pending_outbox_depth = 0
            outbox_reconciliation = 'Drained'
        }
        $script:Metadata = [ordered]@{
            schema = 'rust-mcbe-phase3-run-v1'; run_id = $script:RunId; target = 'Lbsg'
            endpoint = 'play.lbsg.net:19132'; bridge_endpoint = $script:BridgeEndpoint
            build_commit = $script:BuildCommit; source_dirty = $false
            core_sha256 = $script:CoreSha256; app_sha256 = $script:AppSha256
            assets_sha256 = $script:AssetsSha256
            core_process_id = 41; app_process_id = 42; app_exit_code = 0; core_exit_code = $null
            core_terminated_by_launcher = $true; timed_out = $false; duration_seconds = 600
            scenario = 'FastTransferWitness'; screenshot_slots = @(
                [ordered]@{ filename = 'fast-transfer-before.png'; sha256 = $null },
                [ordered]@{ filename = 'fast-transfer-after.png'; sha256 = $null }
            )
        }
        $script:Scenario = [ordered]@{
            schema = 'rust-mcbe-fast-transfer-witness-scenario-v1'; scenario = 'FastTransferWitness'
            target = 'Lbsg'; required_command = '/transfer sm3'
            assets_sha256 = $script:AssetsSha256
            maximum_command_to_reset_arm_milliseconds = 30000
            minimum_post_reset_network_position_delta = 0.5; minimum_duration_seconds = 600
            screenshot_slots = @(
                [ordered]@{ filename = 'fast-transfer-before.png'; sha256 = $null },
                [ordered]@{ filename = 'fast-transfer-after.png'; sha256 = $null }
            )
        }
        $script:Metrics = [ordered]@{
            world_ready = $true; assets = [ordered]@{ blob_sha256 = $script:AssetsSha256 }
        }
        $script:Positions = [double[][]]@(
            [double[]]@(0.0, 72.62, 0.0),
            [double[]]@(64.0, 80.62, 64.0),
            [double[]]@(64.6, 80.62, 64.0)
        )
        Mock Get-Phase2LocalResetSequenceEvidence {
            [pscustomobject]@{
                SnapshotCount = 4; FirstStalledStage = 'none'
                FinalPublication = $script:FocusedTerminalPublication
            }
        }
    }

    function New-FocusedPublication {
        param(
            [uint64]$Epoch, [int]$X, [int]$Z, [uint64]$Armed, [uint64]$Consumed,
            [bool]$IsArmed, [uint64]$Required, [uint64]$Loaded, [bool]$Stable
        )
        return [ordered]@{
            presentation = [ordered]@{
                assets_manifest_sha256 = $script:AssetsSha256
                graphics_identity_sha256 = 'aa' * 32; requested_present_mode = 'fifo'
                effective_present_mode = 'fifo'
                player_column = [ordered]@{
                    dimension = 0; x = $X; z = $Z
                    gpu_presented_subchunks = $(if ($Stable) { 1 } else { 0 })
                }
            }
            publication = [ordered]@{
                session_generation = 7; publisher_epoch = $Epoch; publisher_center = @($X, 64, $Z)
                player_column = [ordered]@{ dimension = 0; x = $X; z = $Z }
                required_columns = $Required; loaded_required_columns = $Loaded
                required_cohort_stable = $Stable; player_column_required = ($Required -gt 0)
                player_column_loaded = ($Loaded -gt 0)
                inactive_level_chunks = 0
                outcomes = [ordered]@{ stale = 0; timed_out = 0 }
                request_queue = [ordered]@{ class_depths = @(); reservations = 0 }
                local_reset = [ordered]@{
                    armed = $IsArmed; armed_count = $Armed; consumed_count = $Consumed
                    dispatch_classes = $(if ($Stable -and $Consumed -gt 0) { @('player_initial') } else { @() })
                    dispatch_count = $(if ($Stable -and $Consumed -gt 0) { 1 } else { 0 })
                    dispatch_total = $(if ($Stable -and $Consumed -gt 0) { 1 } else { 0 })
                    dispatch_trace_overflowed = $false
                }
            }
        }
    }

    function New-CompletePhase2Publication {
        param(
            [uint64]$Epoch, [int]$X, [int]$Z, [uint64]$Armed, [uint64]$Consumed,
            [bool]$IsArmed, [uint64]$Required, [uint64]$Loaded, [bool]$Stable,
            [bool]$Final = $false
        )
        $hash = '1111111111111111'
        $dispatchClasses = [Collections.Generic.List[string]]::new()
        if ($Final) { $dispatchClasses.Add('player_initial') }
        $identity = { param($count, $domain) [ordered]@{
            entry_count = $count; generation_manifest_hash = $hash; manifest_domain = $domain
            publisher_epoch = $Epoch; required_cohort_count = $Required
            required_cohort_hash = $hash; session_generation = 7
        } }
        return [ordered]@{
            client_blob_cache_enabled = $false
            client_blob_cache = [ordered]@{
                hashes_classified = 0; hits = 0; misses = 0; admitted_blobs = 0; rejected_blobs = 0
                evictions = 0; pending_transactions = 0; pending_bytes = 0; pending_resets = 0
                reconstructed_level_chunks = 0; reconstructed_sub_chunks = 0
            }
            presentation = [ordered]@{
                build_profile = 'debug'; requested_present_mode = 'fifo'; effective_present_mode = 'fifo'
                present_mode_proven = $true; visible_subset_of_resident = $true
                graphics_identity_sha256 = 'aa' * 32; assets_manifest_sha256 = $script:AssetsSha256
                publisher_disk = & $identity $Loaded 'key_generation'; resident = & $identity $Loaded 'key'
                allocation = & $identity $Loaded 'key_generation'; visible = & $identity $Loaded 'key'
                submitted = & $identity $Loaded 'key'; gpu_presented = & $identity $Loaded 'key'
                player_column = [ordered]@{
                    dimension = 0; x = $X; z = $Z
                    resident_subchunks = $(if ($Stable) { 1 } else { 0 })
                    allocated_subchunks = $(if ($Stable) { 1 } else { 0 })
                    visible_subchunks = $(if ($Stable) { 1 } else { 0 })
                    submitted_subchunks = $(if ($Stable) { 1 } else { 0 })
                    gpu_presented_subchunks = $(if ($Stable) { 1 } else { 0 })
                }
            }
            publication = [ordered]@{
                session_generation = 7; player_column = [ordered]@{ dimension = 0; x = $X; z = $Z }
                publisher_center = @($X, 64, $Z); publisher_epoch = $Epoch
                publisher_radius_blocks = 128; publisher_radius_chunks = 8
                required_cohort_hash = $hash; required_columns = $Required
                loaded_required_columns = $Loaded; required_cohort_stable = $Stable
                player_column_required = ($Required -gt 0); player_column_loaded = ($Loaded -gt 0)
                inactive_level_chunks = 0
                local_reset = [ordered]@{
                    armed = $IsArmed; armed_count = $Armed; consumed_count = $Consumed
                    dispatch_classes = $dispatchClasses
                    dispatch_count = $(if ($Final) { 1 } else { 0 })
                    dispatch_total = $(if ($Final) { 1 } else { 0 })
                    dispatch_trace_overflowed = $false
                }
                request_queue = [ordered]@{
                    class_depths = @(
                        [ordered]@{ class = 'player_retry'; ready = 0; eligible = 0 },
                        [ordered]@{ class = 'player_initial'; ready = 0; eligible = 0 },
                        [ordered]@{ class = 'visible_retry'; ready = 0; eligible = 0 },
                        [ordered]@{ class = 'visible_initial'; ready = 0; eligible = 0 },
                        [ordered]@{ class = 'prefetch_retry'; ready = 0; eligible = 0 },
                        [ordered]@{ class = 'prefetch_initial'; ready = 0; eligible = 0 }
                    )
                    reservations = 0; ready_blocked_by_reservation = 0; next_class = $null
                    next_is_transport_retry = $false; next_is_starved = $false
                }
                max_queue_wait_us = [ordered]@{ decode = 0; lighting = 0; meshing = 0 }
                max_worker_time_us = [ordered]@{ decode = 0; lighting = 0; meshing = 0 }
                outcomes = [ordered]@{ success = 1; all_air = 0; unavailable = 0; malformed = 0; stale = 0; timed_out = 0 }
                stages = [ordered]@{
                    requests_constructed = 1; requests_ready = 0; requests_transport_pending = 0
                    requests_sent = 1; responses_admitted = 1; subchunks_awaiting_response = 0
                    subchunks_committed = 1; decode_jobs_queued = 0; decode_jobs_dispatched = 1
                    decode_jobs_in_flight = 0; decode_jobs_completed = 1; light_jobs_queued = 0
                    light_jobs_dispatched = 1; light_jobs_in_flight = 0; light_jobs_completed = 1
                    mesh_changes_queued = 0; mesh_changes_pending = 0; mesh_changes_dequeued = 0
                    mesh_jobs_queued = 0; mesh_jobs_dispatched = 1; mesh_jobs_in_flight = 0
                    mesh_jobs_completed = 1; mesh_uploads_unacknowledged = 0
                    mesh_uploads_acknowledged = 1
                }
            }
        }
    }

    function Convert-WitnessToCompletePhase2 {
        param($Artifacts)
        $records = @(
            (New-CompletePhase2Publication 1 1 2 0 0 $false 197 197 $true),
            (New-CompletePhase2Publication 1 1 2 1 0 $true 0 0 $false),
            (New-CompletePhase2Publication 2 4 4 1 1 $false 197 0 $false),
            (New-CompletePhase2Publication 2 4 4 1 1 $false 197 197 $true $true)
        )
        $lines = @(Get-Content -LiteralPath $Artifacts.LogPath)
        $publicationIndexes = @(0..($lines.Count - 1) | Where-Object {
            $lines[$_].StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
        })
        for ($index = 0; $index -lt $publicationIndexes.Count; $index++) {
            $lines[$publicationIndexes[$index]] = 'PHASE2_PUBLICATION=' +
                ($records[$index] | ConvertTo-Json -Depth 20 -Compress)
            $lines[$publicationIndexes[$index] + 1] = New-TestPhase2TimingLine `
                $lines[$publicationIndexes[$index]] (999000 + ($index * 1000))
        }
        Set-Content -LiteralPath $Artifacts.LogPath -Value $lines -Encoding utf8
        return $records
    }

    function New-TestPhase2TimingLine {
        param([string]$PublicationLine, [uint64]$ObservedUnixMs)
        return 'RUST_MCBE_PHASE2_TIMING=' + ([ordered]@{
            schema = 'rust-mcbe-phase2-timing-v1'; observed_unix_ms = $ObservedUnixMs
            publication_sha256 = Get-FastTransferTextSha256 $PublicationLine
        } | ConvertTo-Json -Compress)
    }

    function New-WitnessFrame {
        param([uint64]$Tick, [double[]]$Position, [double[]]$Movement = @(0.0, 1.0))
        return [ordered]@{
            schema = 'rust-mcbe-phase3-frame-v2'; session_generation = 7; fifo_sequence = $Tick
            physics_tick = $Tick; pose_generation = $Tick; dimension = 0
            network_position = $Position; input_mode = 'KeyboardMouse'; perspective = 'FirstPerson'
            camera_blocked = $false; camera_fallback = $false; local_avatar_visible = $false
            movement = $Movement; look_delta = @(0.0, 0.0); jump_held = $false
            outbound_authorized = $true; outbox_depth = 0; outbox_drops = 0
            free_camera_packet_count = 0; grounded_before_tick = $true; grounded_after_tick = $true
            jump_started = $false; jump_repeated = $false; jump_released = $false
        }
    }

    function Write-WitnessArtifacts {
        param([string]$Name)
        $directory = Join-Path $script:TempRoot $Name
        New-Item -ItemType Directory -Path $directory | Out-Null
        $logPath = Join-Path $directory 'app.stdout.log'
        $pre = New-FocusedPublication 1 1 2 0 0 $false 197 197 $true
        $arm = New-FocusedPublication 1 1 2 1 0 $true 0 0 $false
        $consume = New-FocusedPublication 2 4 4 1 1 $false 197 0 $false
        $script:FocusedTerminalPublication = New-FocusedPublication 2 4 4 1 1 $false 197 197 $true
        $preLine = 'PHASE2_PUBLICATION=' + ($pre | ConvertTo-Json -Depth 8 -Compress)
        $armLine = 'PHASE2_PUBLICATION=' + ($arm | ConvertTo-Json -Depth 8 -Compress)
        $consumeLine = 'PHASE2_PUBLICATION=' + ($consume | ConvertTo-Json -Depth 8 -Compress)
        $recoveryLine = 'PHASE2_PUBLICATION=' +
            ($script:FocusedTerminalPublication | ConvertTo-Json -Depth 8 -Compress)
        $lines = @(
            ('RUST_MCBE_PHASE3_IDENTITY=' + ($script:Identity | ConvertTo-Json -Depth 8 -Compress)),
            ('RUST_MCBE_PHASE3_FRAME=' + ((New-WitnessFrame 40 $script:Positions[0]) | ConvertTo-Json -Depth 8 -Compress)),
            $preLine,
            (New-TestPhase2TimingLine $preLine 999000),
            'RUST_MCBE_FAST_TRANSFER_ACTION={"action_ordinal":0,"command":"/transfer sm3","kind":"command_sent","schema":"rust-mcbe-fast-transfer-action-v1","sent_unix_ms":1000000,"session_generation":7}',
            $armLine,
            (New-TestPhase2TimingLine $armLine 1001000),
            $consumeLine,
            (New-TestPhase2TimingLine $consumeLine 1002000),
            $recoveryLine,
            (New-TestPhase2TimingLine $recoveryLine 1003000),
            ('RUST_MCBE_PHASE3_FRAME=' + ((New-WitnessFrame 41 $script:Positions[1]) | ConvertTo-Json -Depth 8 -Compress)),
            ('RUST_MCBE_PHASE3_FRAME=' + ((New-WitnessFrame 42 $script:Positions[2]) | ConvertTo-Json -Depth 8 -Compress)),
            ('RUST_MCBE_PHASE3_FRAME=' + ((New-WitnessFrame 43 $script:Positions[2] @(0.0, 0.0)) | ConvertTo-Json -Depth 8 -Compress)),
            ('RUST_MCBE_PHASE3_TERMINAL=' + ($script:Terminal | ConvertTo-Json -Depth 8 -Compress))
        )
        Set-Content -LiteralPath $logPath -Value $lines -Encoding utf8
        $metadataPath = Join-Path $directory 'run-metadata.json'
        $metricsPath = Join-Path $directory 'app-metrics.json'
        $scenarioPath = Join-Path $directory 'scenario-manifest.json'
        $outputPath = Join-Path $directory 'phase3-final.json'
        $script:Metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $metadataPath -Encoding utf8
        $script:Metrics | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $metricsPath -Encoding utf8
        $script:Scenario | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $scenarioPath -Encoding utf8
        return @{
            LogPath = $logPath; RunMetadataPath = $metadataPath; MetricsPath = $metricsPath
            ScenarioManifestPath = $scenarioPath; OutputPath = $outputPath
        }
    }

    function Invoke-WitnessValidation {
        param($Artifacts)
        return Assert-FastTransferWitnessEvidence @Artifacts `
            -ExpectedBuildCommit $script:BuildCommit -ExpectedPregSha256 $script:PregSha256 `
            -ExpectedBregSha256 $script:BregSha256 -ExpectedCoreSha256 $script:CoreSha256 `
            -ExpectedAppSha256 $script:AppSha256 -ExpectedAssetsSha256 $script:AssetsSha256 `
            -ExpectedRunId $script:RunId `
            -ExpectedBridgeEndpoint $script:BridgeEndpoint -ExpectedCoreProcessId 41 `
            -ExpectedAppProcessId 42 -ExpectedPresentMode Fifo
    }

    It 'accepts one exact post-reset movement and clean terminal witness' {
        $artifacts = Write-WitnessArtifacts 'valid'
        $result = Invoke-WitnessValidation $artifacts
        $result.status | Should Be 'passed'
        $result.post_reset_network_position_delta | Should BeGreaterThan 0.5
        $result.terminal_physics_packet_count | Should Be 9
        Assert-MockCalled Get-Phase2LocalResetSequenceEvidence 1 -ParameterFilter {
            $ExpectedPresentMode -ceq 'Fifo' -and $ExpectedBuildProfile -ceq 'debug' -and
            $WorldReadyObserved -and $Server -ceq 'Lbsg'
        }
        (Test-Path -LiteralPath $artifacts.OutputPath -PathType Leaf) | Should Be $true
    }

    It 'rejects movement that occurs only before the reset consume boundary' {
        $script:Positions[0] = @(100.0, 72.62, 100.0)
        $script:Positions[2] = @(64.4, 80.62, 64.0)
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'pre-reset-only') } | Should Throw
    }

    It 'rejects a post-reset network-position delta below 0.5' {
        $script:Positions[2] = @(64.49, 80.62, 64.0)
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'short-delta') } | Should Throw
    }

    It 'rejects FreeCamera sends, outbox drops, and an unclean terminal' {
        $artifacts = Write-WitnessArtifacts 'free-camera'
        $lines = Get-Content -LiteralPath $artifacts.LogPath
        $frameLineIndex = 0..($lines.Count - 1) | Where-Object {
            $_ -gt 0 -and $lines[$_].StartsWith('RUST_MCBE_PHASE3_FRAME=', [StringComparison]::Ordinal)
        } | Select-Object -First 1
        $frame = $lines[$frameLineIndex].Substring('RUST_MCBE_PHASE3_FRAME='.Length) | ConvertFrom-Json
        $frame.free_camera_packet_count = 1
        $lines[$frameLineIndex] = 'RUST_MCBE_PHASE3_FRAME=' + ($frame | ConvertTo-Json -Depth 8 -Compress)
        Set-Content -LiteralPath $artifacts.LogPath -Value $lines -Encoding utf8
        { Invoke-WitnessValidation $artifacts } | Should Throw

        $script:Terminal.physics_packet_count = 0
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'no-physics') } | Should Throw

        $script:Terminal.physics_packet_count = 9
        $script:Terminal.pending_outbox_depth = 1
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'pending-terminal') } | Should Throw
    }

    It 'rejects non-world-ready, dirty, timed-out, or wrong-endpoint evidence' {
        $script:Metrics.world_ready = $false
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'not-ready') } | Should Throw
        $script:Metrics.world_ready = $true
        $script:Metadata.source_dirty = $true
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'dirty') } | Should Throw
        $script:Metadata.source_dirty = $false
        $script:Metadata.timed_out = $true
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'timed-out') } | Should Throw
        $script:Metadata.timed_out = $false
        $script:Metadata.endpoint = 'zeqa.net:19132'
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'wrong-endpoint') } | Should Throw
    }

    It 'rejects missing duplicated misordered or cross-session command attribution' {
        $missing = Write-WitnessArtifacts 'missing-action'
        @(Get-Content $missing.LogPath | Where-Object {
            -not $_.StartsWith('RUST_MCBE_FAST_TRANSFER_ACTION=', [StringComparison]::Ordinal)
        }) | Set-Content $missing.LogPath -Encoding utf8
        { Invoke-WitnessValidation $missing } | Should Throw

        $duplicate = Write-WitnessArtifacts 'duplicate-action'
        $duplicateLines = @(Get-Content $duplicate.LogPath)
        $actionLine = @($duplicateLines | Where-Object {
            $_.StartsWith('RUST_MCBE_FAST_TRANSFER_ACTION=', [StringComparison]::Ordinal)
        })[0]
        @($duplicateLines + $actionLine) | Set-Content $duplicate.LogPath -Encoding utf8
        { Invoke-WitnessValidation $duplicate } | Should Throw

        $wrongSession = Write-WitnessArtifacts 'wrong-action-session'
        (Get-Content $wrongSession.LogPath) -replace '"session_generation":7}', '"session_generation":8}' |
            Set-Content $wrongSession.LogPath -Encoding utf8
        { Invoke-WitnessValidation $wrongSession } | Should Throw

        $misordered = Write-WitnessArtifacts 'misordered-action'
        $misorderedLines = [Collections.Generic.List[string]]::new()
        foreach ($line in @(Get-Content $misordered.LogPath)) { $misorderedLines.Add($line) }
        $actionIndex = 0..($misorderedLines.Count - 1) | Where-Object {
            $misorderedLines[$_].StartsWith('RUST_MCBE_FAST_TRANSFER_ACTION=', [StringComparison]::Ordinal)
        } | Select-Object -First 1
        $armIndex = @(0..($misorderedLines.Count - 1) | Where-Object {
            $misorderedLines[$_].StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
        })[1]
        $actionValue = $misorderedLines[$actionIndex]
        $misorderedLines.RemoveAt($actionIndex)
        $misorderedLines.Insert($armIndex, $actionValue)
        $misorderedLines | Set-Content $misordered.LogPath -Encoding utf8
        { Invoke-WitnessValidation $misordered } | Should Throw
    }

    It 'rejects unbound delayed or non-first reset-arm timing' {
        $unbound = Write-WitnessArtifacts 'unbound-phase2-time'
        $unboundLines = @(Get-Content $unbound.LogPath)
        $timingIndex = 0..($unboundLines.Count - 1) | Where-Object {
            $unboundLines[$_].StartsWith('RUST_MCBE_PHASE2_TIMING=', [StringComparison]::Ordinal)
        } | Select-Object -First 1
        $timing = $unboundLines[$timingIndex].Substring('RUST_MCBE_PHASE2_TIMING='.Length) | ConvertFrom-Json
        $timing.publication_sha256 = '00' * 32
        $unboundLines[$timingIndex] = 'RUST_MCBE_PHASE2_TIMING=' + ($timing | ConvertTo-Json -Compress)
        $unboundLines | Set-Content $unbound.LogPath -Encoding utf8
        { Invoke-WitnessValidation $unbound } | Should Throw

        $delayed = Write-WitnessArtifacts 'delayed-arm'
        $delayedLines = @(Get-Content $delayed.LogPath)
        $publicationIndexes = @(0..($delayedLines.Count - 1) | Where-Object {
            $delayedLines[$_].StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
        })
        $delayedLines[$publicationIndexes[1] + 1] = New-TestPhase2TimingLine `
            $delayedLines[$publicationIndexes[1]] 1030001
        $delayedLines | Set-Content $delayed.LogPath -Encoding utf8
        { Invoke-WitnessValidation $delayed } | Should Throw

        $wrongFirst = Write-WitnessArtifacts 'wrong-first-reset'
        $wrongFirstLines = [Collections.Generic.List[string]]::new()
        foreach ($line in @(Get-Content $wrongFirst.LogPath)) { $wrongFirstLines.Add($line) }
        $actionIndex = 0..($wrongFirstLines.Count - 1) | Where-Object {
            $wrongFirstLines[$_].StartsWith('RUST_MCBE_FAST_TRANSFER_ACTION=', [StringComparison]::Ordinal)
        } | Select-Object -First 1
        $unexpected = New-FocusedPublication 1 1 2 2 0 $true 0 0 $false
        $unexpectedLine = 'PHASE2_PUBLICATION=' + ($unexpected | ConvertTo-Json -Depth 8 -Compress)
        $wrongFirstLines.Insert($actionIndex + 1, $unexpectedLine)
        $wrongFirstLines.Insert($actionIndex + 2, (New-TestPhase2TimingLine $unexpectedLine 1000500))
        $wrongFirstLines | Set-Content $wrongFirst.LogPath -Encoding utf8
        { Invoke-WitnessValidation $wrongFirst } | Should Throw
    }

    It 'rejects vertical-only zero-input correction-overlapped and asset-mismatched movement proof' {
        $script:Positions[2] = @(64.0, 82.0, 64.0)
        { Invoke-WitnessValidation (Write-WitnessArtifacts 'vertical-only') } | Should Throw
        $script:Positions[2] = @(64.6, 80.62, 64.0)

        $zeroInput = Write-WitnessArtifacts 'zero-input'
        $zeroLines = @(Get-Content $zeroInput.LogPath)
        for ($index = 0; $index -lt $zeroLines.Count; $index++) {
            if ($zeroLines[$index].StartsWith('RUST_MCBE_PHASE3_FRAME=', [StringComparison]::Ordinal)) {
                $frame = $zeroLines[$index].Substring('RUST_MCBE_PHASE3_FRAME='.Length) | ConvertFrom-Json
                $frame.movement = @(0.0, 0.0)
                $zeroLines[$index] = 'RUST_MCBE_PHASE3_FRAME=' + ($frame | ConvertTo-Json -Depth 8 -Compress)
            }
        }
        $zeroLines | Set-Content $zeroInput.LogPath -Encoding utf8
        { Invoke-WitnessValidation $zeroInput } | Should Throw

        $correction = Write-WitnessArtifacts 'correction-overlap'
        $correctionLines = [Collections.Generic.List[string]]::new()
        foreach ($line in @(Get-Content $correction.LogPath)) { $correctionLines.Add($line) }
        $terminalIndex = 0..($correctionLines.Count - 1) | Where-Object {
            $correctionLines[$_].StartsWith('RUST_MCBE_PHASE3_TERMINAL=', [StringComparison]::Ordinal)
        } | Select-Object -First 1
        $correctionLines.Insert($terminalIndex,
            'RUST_MCBE_PHASE3_EVENT={"kind":"correction","schema":"rust-mcbe-phase3-event-v1","session_generation":7}')
        $correctionLines | Set-Content $correction.LogPath -Encoding utf8
        { Invoke-WitnessValidation $correction } | Should Throw

        $unsettled = Write-WitnessArtifacts 'unsettled-final-frame'
        $unsettledLines = @(Get-Content $unsettled.LogPath)
        $lastFrameIndex = @(0..($unsettledLines.Count - 1) | Where-Object {
            $unsettledLines[$_].StartsWith('RUST_MCBE_PHASE3_FRAME=', [StringComparison]::Ordinal)
        } | Select-Object -Last 1)[0]
        $lastFrame = $unsettledLines[$lastFrameIndex].Substring('RUST_MCBE_PHASE3_FRAME='.Length) |
            ConvertFrom-Json
        $lastFrame.movement = @(0.0, 1.0)
        $unsettledLines[$lastFrameIndex] = 'RUST_MCBE_PHASE3_FRAME=' +
            ($lastFrame | ConvertTo-Json -Depth 8 -Compress)
        $unsettledLines | Set-Content $unsettled.LogPath -Encoding utf8
        { Invoke-WitnessValidation $unsettled } | Should Throw

        $assetMismatch = Write-WitnessArtifacts 'asset-mismatch'
        $metrics = Get-Content -Raw $assetMismatch.MetricsPath | ConvertFrom-Json
        $metrics.assets.blob_sha256 = '66' * 32
        $metrics | ConvertTo-Json -Depth 8 | Set-Content $assetMismatch.MetricsPath -Encoding utf8
        { Invoke-WitnessValidation $assetMismatch } | Should Throw
    }

    It 'rejects a terminal pending-correction violation without a final correction event' {
        $pending = Write-WitnessArtifacts 'terminal-pending-correction'
        $lines = [Collections.Generic.List[string]]::new()
        foreach ($line in @(Get-Content $pending.LogPath)) { $lines.Add($line) }
        $terminalIndex = 0..($lines.Count - 1) | Where-Object {
            $lines[$_].StartsWith('RUST_MCBE_PHASE3_TERMINAL=', [StringComparison]::Ordinal)
        } | Select-Object -First 1
        $lines.Insert(
            $terminalIndex,
            'RUST_MCBE_PHASE3_VIOLATION={"reason":"terminal_pending_correction","schema":"rust-mcbe-phase3-violation-v1"}'
        )
        $lines | Set-Content $pending.LogPath -Encoding utf8

        { Invoke-WitnessValidation $pending } | Should Throw
    }

    It 'builds only the fixed authenticated LBSG candidate plan with a ten-minute minimum' {
        $plan = New-Phase3LaunchPlan -Target Lbsg -Endpoint 'play.lbsg.net:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 600 -Scenario FastTransferWitness -AuthCache token.json `
            -Assets vanilla.mcbea
        ($plan.AppArguments -ccontains '--phase3-candidate-physics') | Should Be $true
        ($plan.CoreArguments -ccontains '-auth-cache') | Should Be $true
        { New-Phase3LaunchPlan -Target Lbsg -Endpoint 'play.lbsg.net:19132' `
                -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                -DurationSeconds 599 -Scenario FastTransferWitness -AuthCache token.json `
                -Assets vanilla.mcbea } | Should Throw
        { New-Phase3LaunchPlan -Target Zeqa -Endpoint 'zeqa.net:19132' `
                -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                -DurationSeconds 600 -Scenario FastTransferWitness -AuthCache token.json `
                -Assets vanilla.mcbea } | Should Throw
    }

    It 'runs the combined Phase 2 reset helper end to end and rejects a broken dispatch witness' {
        $artifacts = Write-WitnessArtifacts 'combined-real-success'
        $records = Convert-WitnessToCompletePhase2 $artifacts
        Set-Item -Path Function:\Get-Phase2LocalResetSequenceEvidence -Value $script:RealPhase2ResetHelper
        (Invoke-WitnessValidation $artifacts).status | Should Be 'passed'

        $broken = Write-WitnessArtifacts 'combined-real-failure'
        $brokenRecords = Convert-WitnessToCompletePhase2 $broken
        $brokenRecords[3].publication.local_reset.dispatch_total = 0
        $brokenRecords[3].publication.local_reset.dispatch_count = 0
        $brokenRecords[3].publication.local_reset.dispatch_classes = @()
        $lines = @(Get-Content -LiteralPath $broken.LogPath)
        $publicationIndexes = @(0..($lines.Count - 1) | Where-Object {
            $lines[$_].StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
        })
        $lines[$publicationIndexes[3]] = 'PHASE2_PUBLICATION=' +
            ($brokenRecords[3] | ConvertTo-Json -Depth 20 -Compress)
        $lines[$publicationIndexes[3] + 1] = New-TestPhase2TimingLine `
            $lines[$publicationIndexes[3]] 1002000
        Set-Content -LiteralPath $broken.LogPath -Value $lines -Encoding utf8
        { Invoke-WitnessValidation $broken } | Should Throw
    }

    It 'wires the public entrypoint to stable launcher paths and preserves failed artifacts' {
        $entrypoint = Get-Content -Raw -LiteralPath (Join-Path $script:RepoRoot 'scripts\acceptance\FastTransferWitness.ps1')
        $launcher = Get-Content -Raw -LiteralPath (Join-Path $script:RepoRoot 'scripts\acceptance\Phase3Launcher.ps1')
        $entrypoint | Should Match "Target = 'Lbsg'"
        $entrypoint | Should Match "Scenario = 'FastTransferWitness'"
        $entrypoint | Should Match 'DurationSeconds = 900'
        $entrypoint | Should Match 'microsoft-token.json'
        $entrypoint | Should Match 'vanilla-v1001.mcbea'
        $launcher | Should Match 'target\\debug\\bedrock-client'
        $launcher | Should Match 'FastTransferWitnessValidate.ps1'
        $launcher | Should Match 'validation-error.txt'
        $launcher | Should Match 'rust-mcbe-phase3-launcher-error-v1'
        $launcher.IndexOf('Stop-BoundedProcess -Handle $appHandle -Kind app') | Should BeLessThan `
            $launcher.IndexOf('Stop-BoundedProcess -Handle $coreHandle -Kind core')
        $launcher | Should Not Match 'Remove-Item.*runDirectory'
    }

    It 'rejects outside and reparse-contained paths and a changed clean Git HEAD' {
        $pathRoot = Join-Path $script:TempRoot 'path-root'
        New-Item -ItemType Directory -Path (Join-Path $pathRoot '.local\acceptance') -Force | Out-Null
        $inside = Join-Path $pathRoot '.local\asset.bin'
        Set-Content -LiteralPath $inside -Value 'asset'
        (Resolve-Phase3ContainedPath -ProjectRoot $pathRoot -Path $inside -Scope Local -RequireLeaf) |
            Should Be ([IO.Path]::GetFullPath($inside))
        { Resolve-Phase3ContainedPath -ProjectRoot $pathRoot `
                -Path (Join-Path $script:TempRoot 'outside.bin') -Scope Local } | Should Throw
        if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            $outsideDirectory = Join-Path $script:TempRoot 'outside-dir'
            New-Item -ItemType Directory -Path $outsideDirectory | Out-Null
            Set-Content -LiteralPath (Join-Path $outsideDirectory 'asset.bin') -Value 'asset'
            $junction = Join-Path $pathRoot '.local\linked'
            New-Item -ItemType Junction -Path $junction -Target $outsideDirectory | Out-Null
            { Resolve-Phase3ContainedPath -ProjectRoot $pathRoot `
                    -Path (Join-Path $junction 'asset.bin') -Scope Local -RequireLeaf } | Should Throw
        }

        $gitRoot = Join-Path $script:TempRoot 'git-root'
        New-Item -ItemType Directory -Path $gitRoot | Out-Null
        & git -C $gitRoot init --quiet
        Set-Content -LiteralPath (Join-Path $gitRoot 'tracked.txt') -Value 'one'
        & git -C $gitRoot add tracked.txt
        & git -C $gitRoot -c user.name=Test -c user.email=test@example.invalid commit --quiet -m one
        $firstCommit = (& git -C $gitRoot rev-parse HEAD).Trim()
        Set-Content -LiteralPath (Join-Path $gitRoot 'tracked.txt') -Value 'two'
        & git -C $gitRoot add tracked.txt
        & git -C $gitRoot -c user.name=Test -c user.email=test@example.invalid commit --quiet -m two
        { Assert-Phase3ExactCleanHead -ProjectRoot $gitRoot -ExpectedCommit $firstCommit } | Should Throw
    }
}
