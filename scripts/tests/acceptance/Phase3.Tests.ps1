Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Describe 'Phase 3 production marker evidence validation' {
    BeforeAll {
        $script:RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
        $script:Validator = Join-Path $script:RepoRoot 'scripts\acceptance\Phase3.ps1'
        . (Join-Path $script:RepoRoot 'scripts\acceptance\Phase3Launch.ps1')
        $script:TempRoot = Join-Path ([IO.Path]::GetTempPath()) "rust-mcbe-phase3-$PID-$([guid]::NewGuid().ToString('N'))"
        New-Item -ItemType Directory -Path $script:TempRoot -Force | Out-Null
    }

    AfterAll {
        if (Test-Path -LiteralPath $script:TempRoot) {
            Remove-Item -LiteralPath $script:TempRoot -Recurse -Force
        }
    }

    BeforeEach {
        $script:BuildCommit = '0123456789abcdef0123456789abcdef01234567'
        $script:PregSha256 = '11' * 32
        $script:BregSha256 = '22' * 32
        $script:RunId = '0123456789abcdef0123456789abcdef'
        $script:Endpoint = '127.0.0.1:19132'
        $script:BridgeEndpoint = '127.0.0.1:19133'
        $script:CoreSha256 = '33' * 32
        $script:AppSha256 = '44' * 32
        $script:Identity = [ordered]@{
            schema = 'rust-mcbe-phase3-identity-v1'; build_commit = $script:BuildCommit
            target = 'Bds'; protocol = 1001; session_generation = 7
            preg_sha256 = $script:PregSha256; breg_sha256 = $script:BregSha256
            candidate_physics = $true
            source_dirty = $false; run_id = $script:RunId; endpoint = $script:Endpoint
            bridge_endpoint = $script:BridgeEndpoint
            core_sha256 = $script:CoreSha256; core_process_id = 41; app_process_id = 42
        }
        $script:Frames = @(
            [ordered]@{
                schema = 'rust-mcbe-phase3-frame-v1'; session_generation = 7; fifo_sequence = 40
                physics_tick = 41; pose_generation = 101; dimension = 0; input_mode = 'KeyboardMouse'
                perspective = 'FirstPerson'; movement = @(0.0, 1.0); look_delta = @(0.0, 0.0)
                camera_blocked = $false; camera_fallback = $false; local_avatar_visible = $false
                jump_held = $true; outbound_authorized = $true; outbox_depth = 1; outbox_drops = 0
                free_camera_packet_count = 0
                grounded_before_tick = $true; grounded_after_tick = $false
                jump_started = $true; jump_repeated = $false; jump_released = $false
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-frame-v1'; session_generation = 7; fifo_sequence = 41
                physics_tick = 42; pose_generation = 102; dimension = 0; input_mode = 'GamePad'
                perspective = 'ThirdPersonBack'; movement = @(0.0, 1.0); look_delta = @(0.5, -0.25)
                camera_blocked = $true; camera_fallback = $false; local_avatar_visible = $true
                jump_held = $true; outbound_authorized = $true; outbox_depth = 2; outbox_drops = 0
                free_camera_packet_count = 0
                grounded_before_tick = $true; grounded_after_tick = $false
                jump_started = $true; jump_repeated = $true; jump_released = $false
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-frame-v1'; session_generation = 7; fifo_sequence = 42
                physics_tick = 43; pose_generation = 103; dimension = 1; input_mode = 'Touch'
                perspective = 'ThirdPersonFront'; movement = @(-0.25, 0.75); look_delta = @(-0.5, 0.25)
                camera_blocked = $false; camera_fallback = $true; local_avatar_visible = $true
                jump_held = $false; outbound_authorized = $true; outbox_depth = 0; outbox_drops = 0
                free_camera_packet_count = 0
                grounded_before_tick = $false; grounded_after_tick = $false
                jump_started = $false; jump_repeated = $false; jump_released = $true
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-frame-v1'; session_generation = 7; fifo_sequence = 43
                physics_tick = 44; pose_generation = 104; dimension = 1; input_mode = 'Touch'
                perspective = 'FirstPerson'; movement = @(0.0, 0.5); look_delta = @(0.0, 0.0)
                camera_blocked = $false; camera_fallback = $false; local_avatar_visible = $false
                jump_held = $false; outbound_authorized = $true; outbox_depth = 0; outbox_drops = 0
                free_camera_packet_count = 0
                grounded_before_tick = $true; grounded_after_tick = $true
                jump_started = $false; jump_repeated = $false; jump_released = $false
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-frame-v1'; session_generation = 7; fifo_sequence = 44
                physics_tick = 45; pose_generation = 105; dimension = 1; input_mode = 'KeyboardMouse'
                perspective = 'FirstPerson'; movement = @(0.5, 0.5); look_delta = @(0.0, 0.0)
                camera_blocked = $false; camera_fallback = $false; local_avatar_visible = $false
                jump_held = $false; outbound_authorized = $true; outbox_depth = 0; outbox_drops = 0
                free_camera_packet_count = 0
                grounded_before_tick = $true; grounded_after_tick = $true
                jump_started = $false; jump_repeated = $false; jump_released = $false
            }
        )
        $script:Events = @(
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'correction'; event_sequence = 0
                session_generation = 7; fifo_sequence = 40; physics_tick = 41; dimension = 0
                correction_outcome = 'replayed'; corrected_tick = 40; replayed_ticks = 1
                correction_magnitude = 3.5
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'correction'; event_sequence = 1
                session_generation = 7; fifo_sequence = 41; physics_tick = 42; dimension = 0
                correction_outcome = 'snapped'; corrected_tick = 42; replayed_ticks = 0
                correction_magnitude = 1.25
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'dimension'; session_generation = 7
                event_sequence = 2; fifo_sequence = 42; physics_tick = 43; dimension = 1
            }
        )
        $script:Violations = @()
        $script:Terminals = @(
            [ordered]@{
                schema = 'rust-mcbe-phase3-terminal-v1'; session_generation = 7
                source = 'Physics'; physics_packet_count = 5; free_camera_packet_count = 0
                pending_outbox_depth = 0; outbox_reconciliation = 'Drained'
            }
        )
        $script:ScenarioManifest = [ordered]@{
            schema = 'rust-mcbe-phase3-scenario-v1'; scenario = 'CandidatePhysics'
            required_input_modes = @('KeyboardMouse', 'GamePad', 'Touch')
            required_perspective_sequence = @(
                'FirstPerson', 'ThirdPersonBack', 'ThirdPersonFront', 'FirstPerson'
            )
            require_replay = $true; require_snap = $true; require_held_jump_rejump = $true
            require_release_before_landing = $true; require_camera_blocked = $true
            require_camera_fallback = $true; require_avatar_visibility_states = $true
            required_controlled_matrix = [ordered]@{
                sprint = $true; sneak_ledge = $true; slabs_stairs = $true; ladder = $true
                liquids = @('Water', 'Lava')
                special_surfaces = @('Cobweb', 'Slime', 'Bed', 'SoulSand', 'Honey', 'BubbleColumn')
                knockback = $true; teleport = $true; dimension_change = $true
                focus_loss = $true; controller_disconnect = $true
                frame_caps = @(30, 60, 144); targeting_ray_invariant = $true
                flat_walk_min_magnitude = 0.25; diagonal_walk_min_axis_magnitude = 0.25
                single_jump_non_repeated_min_count = 1
                camera_wall_outcome = 'WallBlocked'
                camera_corner_outcome = 'CornerBlocked'
                camera_ceiling_outcome = 'CeilingBlocked'
            }
        }
        $script:RunMetadata = [ordered]@{
            schema = 'rust-mcbe-phase3-run-v1'; run_id = $script:RunId; target = 'Bds'
            endpoint = $script:Endpoint; build_commit = $script:BuildCommit; source_dirty = $false
            bridge_endpoint = $script:BridgeEndpoint
            core_sha256 = $script:CoreSha256; app_sha256 = $script:AppSha256
            core_process_id = 41; app_process_id = 42; app_exit_code = 0; core_exit_code = $null
            core_terminated_by_launcher = $true; timed_out = $false; duration_seconds = 60
            scenario = 'CandidatePhysics'
        }
        $script:Metrics = [ordered]@{
            session_seconds = 60.0; frame_count = 3600; p50_frame_ms = 16.0; p95_frame_ms = 18.0
            p99_frame_ms = 20.0; max_frame_ms = 24.0; rendered_sub_chunks = 64
            resident_sub_chunks = 80; visible_sub_chunks = 64; decode_error_count = 0
            gpu_upload_bytes = 4096
        }
    }

    function Write-MarkerLog {
        param([string]$Name)
        $path = Join-Path $script:TempRoot $Name
        $lines = [Collections.Generic.List[string]]::new()
        $lines.Add('ordinary client log line')
        $lines.Add('RUST_MCBE_PHASE3_IDENTITY=' + ($script:Identity | ConvertTo-Json -Depth 6 -Compress))
        foreach ($frame in $script:Frames) {
            $lines.Add('RUST_MCBE_PHASE3_FRAME=' + ($frame | ConvertTo-Json -Depth 6 -Compress))
        }
        foreach ($event in $script:Events) {
            $lines.Add('RUST_MCBE_PHASE3_EVENT=' + ($event | ConvertTo-Json -Depth 6 -Compress))
        }
        foreach ($violation in $script:Violations) {
            $lines.Add('RUST_MCBE_PHASE3_VIOLATION=' + ($violation | ConvertTo-Json -Depth 6 -Compress))
        }
        foreach ($terminal in $script:Terminals) {
            $lines.Add('RUST_MCBE_PHASE3_TERMINAL=' + ($terminal | ConvertTo-Json -Depth 6 -Compress))
        }
        Set-Content -LiteralPath $path -Value $lines -Encoding utf8
        return $path
    }

    function Invoke-Validator {
        param([string]$Path)
        $runMetadataPath = $Path + '.run.json'
        $metricsPath = $Path + '.metrics.json'
        $outputPath = $Path + '.final.json'
        $scenarioManifestPath = $Path + '.scenario.json'
        $script:RunMetadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $runMetadataPath -Encoding utf8
        $script:Metrics | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $metricsPath -Encoding utf8
        $script:ScenarioManifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $scenarioManifestPath -Encoding utf8
        $savedErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $script:Validator `
                -LogPath $Path -ExpectedTarget Bds -ExpectedBuildCommit $script:BuildCommit `
                -ExpectedPregSha256 $script:PregSha256 -ExpectedBregSha256 $script:BregSha256 `
                -ExpectedRunId $script:RunId -ExpectedEndpoint $script:Endpoint `
                -ExpectedBridgeEndpoint $script:BridgeEndpoint `
                -ExpectedCoreSha256 $script:CoreSha256 -ExpectedCoreProcessId 41 `
                -ExpectedAppProcessId 42 -RunMetadataPath $runMetadataPath `
                -MetricsPath $metricsPath -OutputPath $outputPath `
                -ScenarioManifestPath $scenarioManifestPath `
                2>&1 | Out-String
            $exitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $savedErrorActionPreference
        }
        return [pscustomobject]@{ ExitCode = $exitCode; Output = $output; Aggregate = $outputPath }
    }

    It 'accepts one bounded production-derived consecutive tick sequence' {
        $result = Invoke-Validator (Write-MarkerLog 'valid.log')
        $result.ExitCode | Should Be 0
        $result.Output | Should Match 'PHASE3_EVIDENCE_VALID target=Bds .* frames=5 events=3'
        $aggregate = Get-Content -Raw -LiteralPath $result.Aggregate | ConvertFrom-Json
        $aggregate.movement.held_jump_longest_run | Should Be 2
        $aggregate.camera_avatar.perspective_sequence -join ',' | Should Be 'FirstPerson,ThirdPersonBack,ThirdPersonFront,FirstPerson'
        $aggregate.evidence.terminal_pending_outbox_depth | Should Be 0
        $aggregate.evidence.terminal_outbox_reconciliation | Should Be 'Drained'
        $aggregate.required_controlled_matrix.frame_caps -join ',' | Should Be '30,60,144'
        $aggregate.required_controlled_matrix.targeting_ray_invariant | Should Be $true
        $aggregate.required_controlled_matrix.camera_wall_outcome | Should Be 'WallBlocked'
        $aggregate.required_controlled_matrix.camera_corner_outcome | Should Be 'CornerBlocked'
        $aggregate.required_controlled_matrix.camera_ceiling_outcome | Should Be 'CeilingBlocked'
        $aggregate.movement.flat_walk_witness_count | Should Be 1
        $aggregate.movement.diagonal_walk_witness_count | Should Be 1
        $aggregate.movement.single_jump_non_repeated_count | Should Be 1
    }

    It 'accepts an independent network-silent FreeCamera terminal without movement frames' {
        $script:Identity.candidate_physics = $false
        $script:Frames = @()
        $script:Events = @()
        $script:Terminals[0].source = 'FreeCamera'
        $script:Terminals[0].physics_packet_count = 0
        $script:Terminals[0].outbox_reconciliation = 'NotAuthoritative'
        $script:ScenarioManifest.scenario = 'FreeCameraSilence'
        $script:ScenarioManifest.required_input_modes = @()
        $script:ScenarioManifest.required_perspective_sequence = @()
        $script:ScenarioManifest.require_replay = $false
        $script:ScenarioManifest.require_snap = $false
        $script:ScenarioManifest.require_held_jump_rejump = $false
        $script:ScenarioManifest.require_release_before_landing = $false
        $script:ScenarioManifest.require_camera_blocked = $false
        $script:ScenarioManifest.require_camera_fallback = $false
        $script:ScenarioManifest.require_avatar_visibility_states = $false
        $script:ScenarioManifest.required_controlled_matrix = [ordered]@{
            sprint = $false; sneak_ledge = $false; slabs_stairs = $false; ladder = $false
            liquids = @(); special_surfaces = @(); knockback = $false; teleport = $false
            dimension_change = $false; focus_loss = $false; controller_disconnect = $false
            frame_caps = @(); targeting_ray_invariant = $false
            flat_walk_min_magnitude = 0.0; diagonal_walk_min_axis_magnitude = 0.0
            single_jump_non_repeated_min_count = 0
            camera_wall_outcome = 'NotRequired'
            camera_corner_outcome = 'NotRequired'
            camera_ceiling_outcome = 'NotRequired'
        }
        $script:RunMetadata.scenario = 'FreeCameraSilence'
        $result = Invoke-Validator (Write-MarkerLog 'free-camera-silence-valid.log')
        $result.ExitCode | Should Be 0
        $result.Output | Should Match 'scenario=FreeCameraSilence .* frames=0 events=0'
        $aggregate = Get-Content -Raw -LiteralPath $result.Aggregate | ConvertFrom-Json
        $aggregate.movement.tick_count | Should Be 0
        $aggregate.evidence.terminal_free_camera_packet_count | Should Be 0
    }

    It 'builds exact live target plans with candidate-only physics and no free camera' {
        $targets = [ordered]@{
            Lunar = 'pvp.lunarbedrock.com:19134'
            Zeqa = 'zeqa.net:19132'
            Lbsg = 'play.lbsg.net:19132'
            Bds = '127.0.0.1:19132'
        }
        foreach ($target in $targets.Keys) {
            $endpoint = Get-Phase3TargetEndpoint -Target $target
            $endpoint | Should Be $targets[$target]
            $authCache = if ($target -ceq 'Bds') { $null } else { '.local/auth/token.json' }
            $duration = if ($target -ceq 'Bds') { 60 } else { 300 }
            $plan = New-Phase3LaunchPlan -Target $target -Endpoint $endpoint `
                -RunId $script:RunId -SocketDirectory 'socket' -MetricsPath 'metrics.json' `
                -DurationSeconds $duration -Scenario CandidatePhysics -AuthCache $authCache
            $plan.CoreArguments -join ' ' | Should Match ([regex]::Escape("-upstream $endpoint"))
            ($plan.AppArguments -ccontains '--phase3-candidate-physics') | Should Be $true
            ($plan.AppArguments -ccontains '--phase3-evidence-target') | Should Be $true
            ($plan.AppArguments -ccontains '--auto-fly') | Should Be $false
            ($plan.CoreArguments -ccontains '-auth-cache') | Should Be ($target -cne 'Bds')
        }
    }

    It 'forbids missing authentication on Lunar Zeqa and LBSG plans' {
        foreach ($target in @('Lunar', 'Zeqa', 'Lbsg')) {
            { New-Phase3LaunchPlan -Target $target -Endpoint (Get-Phase3TargetEndpoint $target) `
                    -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                    -DurationSeconds 300 -Scenario CandidatePhysics } | Should Throw
        }
    }

    It 'forbids sub-five-minute Lunar Zeqa and LBSG plans' {
        foreach ($target in @('Lunar', 'Zeqa', 'Lbsg')) {
            { New-Phase3LaunchPlan -Target $target -Endpoint (Get-Phase3TargetEndpoint $target) `
                    -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                    -DurationSeconds 299 -Scenario CandidatePhysics -AuthCache token.json } | Should Throw
        }
    }

    It 'preserves the offline BDS candidate plan' {
        $bds = New-Phase3LaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 60 -Scenario CandidatePhysics
        ($bds.CoreArguments -ccontains '-auth-cache') | Should Be $false
    }

    It 'builds a distinct network-silent FreeCamera scenario without candidate physics frames' {
        $plan = New-Phase3LaunchPlan -Target Lunar -Endpoint (Get-Phase3TargetEndpoint Lunar) `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 300 -Scenario FreeCameraSilence -AuthCache token.json
        ($plan.AppArguments -ccontains '--auto-fly') | Should Be $true
        ($plan.AppArguments -ccontains '--phase3-candidate-physics') | Should Be $false
        ($plan.CoreArguments -ccontains '-auth-cache') | Should Be $true
    }

    It 'uses the mandated stable Windows debug client path and non-release Cargo build' {
        $launcher = Get-Content -Raw -LiteralPath (Join-Path $script:RepoRoot 'scripts\acceptance\Phase3Launcher.ps1')
        $launcher | Should Match 'target\\debug\\bedrock-client'
        $launcher | Should Not Match "'build', '--release'"
        $launcher | Should Match "'build', '--locked', '-p', 'bedrock-client'"
        $launcher | Should Match 'Resolve-Phase2ContainedPath'
        $launcher | Should Match '-AuthCache \$authCacheFull'
        $launcher | Should Match '-ScenarioManifestPath \$scenarioManifestPath'
    }

    It 'creates a missing Phase 3 run directory' {
        $missing = Join-Path $script:TempRoot 'run-directory-missing'
        (Initialize-Phase3RunDirectory -Path $missing) | Should Be ([IO.Path]::GetFullPath($missing))
        (Test-Path -LiteralPath $missing -PathType Container) | Should Be $true
    }

    It 'accepts an existing empty Phase 3 run directory' {
        $empty = Join-Path $script:TempRoot 'run-directory-empty'
        New-Item -ItemType Directory -Path $empty | Out-Null
        (Initialize-Phase3RunDirectory -Path $empty) | Should Be ([IO.Path]::GetFullPath($empty))
    }

    It 'rejects a nonempty Phase 3 run directory as the only changed condition' {
        $nonempty = Join-Path $script:TempRoot 'run-directory-nonempty'
        New-Item -ItemType Directory -Path $nonempty | Out-Null
        Set-Content -LiteralPath (Join-Path $nonempty 'stale.txt') -Value stale
        { Initialize-Phase3RunDirectory -Path $nonempty } | Should Throw
    }

    It 'attributes a fresh bridge endpoint publication to the current core' {
        $socket = Join-Path $script:TempRoot 'fresh-endpoint-socket'
        New-Item -ItemType Directory -Path $socket | Out-Null
        $guard = New-Phase3EndpointPublicationGuard -SocketDirectory $socket
        Set-Content -LiteralPath $guard.EndpointPath -Value '127.0.0.1:19133'
        $handle = [pscustomobject]@{
            Process = [pscustomobject]@{ Id = 41; HasExited = $false; ExitCode = 0 }
        }
        $witness = Wait-Phase3BridgeEndpoint -Guard $guard -CoreHandle $handle -TimeoutSeconds 1
        $witness.Endpoint | Should Be '127.0.0.1:19133'
        $witness.CoreProcessId | Should Be 41
    }

    It 'rejects a stale bridge endpoint before launching the core' {
        $staleSocket = Join-Path $script:TempRoot 'stale-endpoint-socket'
        New-Item -ItemType Directory -Path $staleSocket | Out-Null
        Set-Content -LiteralPath (Join-Path $staleSocket 'game.addr') -Value '127.0.0.1:19134'
        { New-Phase3EndpointPublicationGuard -SocketDirectory $staleSocket } | Should Throw
    }

    It 'accepts a tick reanchor only at an exactly correlated dimension transition' {
        $script:Frames[2].physics_tick = 0
        $script:Frames[3].physics_tick = 1
        $script:Frames[4].physics_tick = 2
        $script:Events[2].physics_tick = 0
        $result = Invoke-Validator (Write-MarkerLog 'dimension-reanchor.log')
        $result.ExitCode | Should Be 0
    }

    It 'accepts equal pose generations for multi-tick catch-up evidence' {
        $script:Frames[1].pose_generation = $script:Frames[0].pose_generation
        $result = Invoke-Validator (Write-MarkerLog 'catch-up-pose.log')
        $result.ExitCode | Should Be 0
    }

    It 'aggregates bounded replay and snap correction outcomes and magnitudes' {
        $script:Events = @(
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'correction'; event_sequence = 0
                session_generation = 7; fifo_sequence = 40; physics_tick = 41; dimension = 0
                correction_outcome = 'replayed'; corrected_tick = 40; replayed_ticks = 1
                correction_magnitude = 3.5
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'correction'; event_sequence = 1
                session_generation = 7; fifo_sequence = 41; physics_tick = 42; dimension = 0
                correction_outcome = 'snapped'; corrected_tick = 42; replayed_ticks = 0
                correction_magnitude = 1.25
            },
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'dimension'; event_sequence = 2
                session_generation = 7; fifo_sequence = 42; physics_tick = 43; dimension = 1
            }
        )
        $result = Invoke-Validator (Write-MarkerLog 'corrections.log')
        $result.ExitCode | Should Be 0
        $aggregate = Get-Content -Raw -LiteralPath $result.Aggregate | ConvertFrom-Json
        $aggregate.movement.correction_count | Should Be 2
        $aggregate.movement.replay_count | Should Be 1
        $aggregate.movement.snap_count | Should Be 1
        $aggregate.movement.max_correction_magnitude | Should Be 3.5
    }

    It 'rejects a missing frame schema field as the only changed condition' {
        $script:Frames[0].Remove('jump_held')
        $result = Invoke-Validator (Write-MarkerLog 'schema-missing.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects an unknown frame schema field as the only changed condition' {
        $script:Frames[1].unknown = 1
        $result = Invoke-Validator (Write-MarkerLog 'schema-unknown.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a stale build commit as the only changed condition' {
        $script:Identity.build_commit = 'f' * 40
        (Invoke-Validator (Write-MarkerLog 'identity-build.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a wrong target as the only changed condition' {
        $script:Identity.target = 'Zeqa'
        (Invoke-Validator (Write-MarkerLog 'identity-target.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a wrong session generation as the only changed condition' {
        $script:Identity.session_generation = 8
        (Invoke-Validator (Write-MarkerLog 'identity-session.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a stale PREG hash as the only changed condition' {
        $script:Identity.preg_sha256 = '33' * 32
        (Invoke-Validator (Write-MarkerLog 'identity-preg.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a stale BREG hash as the only changed condition' {
        $script:Identity.breg_sha256 = '44' * 32
        (Invoke-Validator (Write-MarkerLog 'identity-breg.log')).ExitCode | Should Not Be 0
    }

    It 'rejects dirty run metadata as the only changed condition' {
        $script:RunMetadata.source_dirty = $true
        (Invoke-Validator (Write-MarkerLog 'run-dirty.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a mismatched run ID as the only changed condition' {
        $script:RunMetadata.run_id = 'f' * 32
        (Invoke-Validator (Write-MarkerLog 'run-id.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a mismatched core process ID as the only changed condition' {
        $script:RunMetadata.core_process_id = 99
        (Invoke-Validator (Write-MarkerLog 'run-core-process.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a mismatched app process ID as the only changed condition' {
        $script:RunMetadata.app_process_id = 99
        (Invoke-Validator (Write-MarkerLog 'run-app-process.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a mismatched core hash as the only changed condition' {
        $script:RunMetadata.core_sha256 = '55' * 32
        (Invoke-Validator (Write-MarkerLog 'run-core-hash.log')).ExitCode | Should Not Be 0
    }

    It 'rejects timeout metadata as the only changed condition' {
        $script:RunMetadata.timed_out = $true
        (Invoke-Validator (Write-MarkerLog 'run-timeout.log')).ExitCode | Should Not Be 0
    }

    It 'rejects recorded free-camera packets from production movement evidence' {
        $script:Frames[1].free_camera_packet_count = 1
        $result = Invoke-Validator (Write-MarkerLog 'free-camera.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects one production violation marker as the only changed condition' {
        $script:Violations = @([ordered]@{
            schema = 'rust-mcbe-phase3-violation-v1'; reason = 'invalid_frame'
        })
        $result = Invoke-Validator (Write-MarkerLog 'violation.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects one outbox drop as the only changed condition' {
        $script:Frames[1].outbox_drops = 1
        $result = Invoke-Validator (Write-MarkerLog 'one-drop.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects one contradictory camera state as the only changed condition' {
        $script:Frames[1].camera_fallback = $true
        $result = Invoke-Validator (Write-MarkerLog 'camera-contradiction.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects one invalid correction magnitude as the only changed condition' {
        $script:Events[0].correction_magnitude = -0.01
        $result = Invoke-Validator (Write-MarkerLog 'invalid-correction-magnitude.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a missing required input mode witness as the only changed condition' {
        $script:Frames[1].input_mode = 'KeyboardMouse'
        $result = Invoke-Validator (Write-MarkerLog 'missing-input-mode.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a mismatched required perspective sequence as the only changed condition' {
        $script:ScenarioManifest.required_perspective_sequence = @(
            'FirstPerson', 'ThirdPersonFront', 'ThirdPersonBack'
        )
        $result = Invoke-Validator (Write-MarkerLog 'wrong-perspective-sequence.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a missing sprint controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.sprint = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-sprint.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing sneak ledge controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.sneak_ledge = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-sneak-ledge.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing slabs stairs controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.slabs_stairs = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-slabs-stairs.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing ladder controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.ladder = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-ladder.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an incomplete liquids controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.liquids = @('Water')
        (Invoke-Validator (Write-MarkerLog 'matrix-liquids.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an incomplete special-surfaces controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.special_surfaces = @(
            'Cobweb', 'Slime', 'Bed', 'SoulSand', 'Honey'
        )
        (Invoke-Validator (Write-MarkerLog 'matrix-special-surfaces.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing knockback controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.knockback = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-knockback.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing teleport controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.teleport = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-teleport.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing dimension-change controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.dimension_change = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-dimension-change.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing focus-loss controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.focus_loss = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-focus-loss.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing controller-disconnect controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.controller_disconnect = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-controller-disconnect.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an incomplete frame-cap controlled-matrix requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.frame_caps = @(30, 60)
        (Invoke-Validator (Write-MarkerLog 'matrix-frame-caps.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing targeting-ray invariance requirement as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.targeting_ray_invariant = $false
        (Invoke-Validator (Write-MarkerLog 'matrix-targeting-ray.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing flat-walk production witness as the only changed condition' {
        $script:Frames[3].movement = @(0.0, 0.0)
        (Invoke-Validator (Write-MarkerLog 'missing-flat-walk.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing diagonal-walk production witness as the only changed condition' {
        $script:Frames[4].movement = @(0.0, 0.0)
        (Invoke-Validator (Write-MarkerLog 'missing-diagonal-walk.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an all-zero movement sequence as the only changed condition' {
        foreach ($frame in $script:Frames) { $frame.movement = @(0.0, 0.0) }
        (Invoke-Validator (Write-MarkerLog 'all-zero-movement.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a weakened flat-walk minimum as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.flat_walk_min_magnitude = 0.0
        (Invoke-Validator (Write-MarkerLog 'matrix-flat-walk-min.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a weakened diagonal-walk minimum as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.diagonal_walk_min_axis_magnitude = 0.0
        (Invoke-Validator (Write-MarkerLog 'matrix-diagonal-walk-min.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a weakened single-jump minimum as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.single_jump_non_repeated_min_count = 0
        (Invoke-Validator (Write-MarkerLog 'matrix-single-jump-min.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a weakened wall-collision outcome as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.camera_wall_outcome = 'NotRequired'
        (Invoke-Validator (Write-MarkerLog 'matrix-camera-wall.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a weakened corner-collision outcome as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.camera_corner_outcome = 'NotRequired'
        (Invoke-Validator (Write-MarkerLog 'matrix-camera-corner.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a weakened ceiling-collision outcome as the only changed condition' {
        $script:ScenarioManifest.required_controlled_matrix.camera_ceiling_outcome = 'NotRequired'
        (Invoke-Validator (Write-MarkerLog 'matrix-camera-ceiling.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing non-repeated single-jump witness as the only changed condition' {
        $script:Frames[0].jump_started = $false
        (Invoke-Validator (Write-MarkerLog 'missing-single-jump.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a missing held landing re-jump witness as the only changed condition' {
        $script:Frames[1].jump_repeated = $false
        $result = Invoke-Validator (Write-MarkerLog 'missing-held-rejump.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a missing release-before-landing witness as the only changed condition' {
        $script:Frames[2].jump_released = $false
        $result = Invoke-Validator (Write-MarkerLog 'missing-release-before-landing.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a missing replay witness as the only changed condition' {
        $script:Events = @($script:Events | Where-Object { $_.event_sequence -ne 0 })
        $result = Invoke-Validator (Write-MarkerLog 'missing-replay.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a missing snap witness as the only changed condition' {
        $script:Events = @($script:Events | Where-Object { $_.event_sequence -ne 1 })
        $result = Invoke-Validator (Write-MarkerLog 'missing-snap.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a zero candidate terminal packet count as the only changed condition' {
        $script:Terminals[0].physics_packet_count = 0
        $result = Invoke-Validator (Write-MarkerLog 'terminal-count.log')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a nonempty terminal outbox as the only changed condition' {
        $script:Terminals[0].pending_outbox_depth = 1
        (Invoke-Validator (Write-MarkerLog 'terminal-pending-outbox.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a final Full restoration as the only changed condition' {
        $script:Terminals[0].outbox_reconciliation = 'FullRestored'
        (Invoke-Validator (Write-MarkerLog 'terminal-full-restored.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a nonzero app process exit as the only changed condition' {
        $script:RunMetadata.app_exit_code = 9
        (Invoke-Validator (Write-MarkerLog 'process-app-exit.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a nonzero core process exit as the only changed condition' {
        $script:RunMetadata.core_exit_code = 7
        $script:RunMetadata.core_terminated_by_launcher = $false
        (Invoke-Validator (Write-MarkerLog 'process-core-exit.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a string-coerced integer field as the only changed condition' {
        $script:Frames[0].session_generation = '7'
        (Invoke-Validator (Write-MarkerLog 'type-integer.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a string-coerced number field as the only changed condition' {
        $script:Frames[1].movement = @('0.0', 1.0)
        (Invoke-Validator (Write-MarkerLog 'type-number.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a string-coerced boolean field as the only changed condition' {
        $script:Frames[2].outbound_authorized = 'true'
        (Invoke-Validator (Write-MarkerLog 'type-boolean.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a duplicate physics tick as the only changed condition' {
        $script:Frames[1].physics_tick = 41
        (Invoke-Validator (Write-MarkerLog 'tick-duplicate.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a retrograde physics tick as the only changed condition' {
        $script:Frames[1].physics_tick = 40
        (Invoke-Validator (Write-MarkerLog 'tick-retrograde.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a gapped physics tick as the only changed condition' {
        $script:Frames[1].physics_tick = 45
        (Invoke-Validator (Write-MarkerLog 'tick-gap.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a retrograde FIFO sequence as the only changed condition' {
        $script:Frames[1].fifo_sequence = 39
        (Invoke-Validator (Write-MarkerLog 'fifo-retrograde.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a retrograde pose generation as the only changed condition' {
        $script:Frames[1].pose_generation = 100
        (Invoke-Validator (Write-MarkerLog 'pose-retrograde.log')).ExitCode | Should Not Be 0
    }

    It 'rejects a gapped post-dimension physics tick as the only changed condition' {
        $script:Frames[2].physics_tick = 45
        $script:Frames[3].physics_tick = 47
        (Invoke-Validator (Write-MarkerLog 'post-dimension-tick-gap.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event session mismatch as the only changed condition' {
        $script:Events[0].session_generation = 8
        (Invoke-Validator (Write-MarkerLog 'event-session.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event FIFO mismatch as the only changed condition' {
        $script:Events[0].fifo_sequence = 99
        (Invoke-Validator (Write-MarkerLog 'event-fifo.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event physics-tick mismatch as the only changed condition' {
        $script:Events[0].physics_tick = 99
        (Invoke-Validator (Write-MarkerLog 'event-tick.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event dimension mismatch as the only changed condition' {
        $script:Events[0].dimension = 1
        (Invoke-Validator (Write-MarkerLog 'event-dimension.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an out-of-range movement vector as the only changed condition' {
        $script:Frames[0].movement = @(2.0, 0.0)
        (Invoke-Validator (Write-MarkerLog 'bound-movement.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an over-capacity outbox depth as the only changed condition' {
        $script:Frames[1].outbox_depth = 33
        (Invoke-Validator (Write-MarkerLog 'bound-outbox.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an unsupported input mode as the only changed condition' {
        $script:Frames[2].input_mode = 'RememberedTouch'
        (Invoke-Validator (Write-MarkerLog 'enum-input-mode.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an over-capacity event record array as the only changed condition' {
        $script:Events = @(0..256 | ForEach-Object {
            [ordered]@{
                schema = 'rust-mcbe-phase3-event-v1'; kind = 'correction'; session_generation = 7
                event_sequence = $_; fifo_sequence = 40; physics_tick = 41; dimension = 0
                correction_outcome = 'snapped'; corrected_tick = 41; replayed_ticks = 0
                correction_magnitude = 1.0
            }
        })
        (Invoke-Validator (Write-MarkerLog 'bound-events.log')).ExitCode | Should Not Be 0
    }

    It 'rejects hand-authored JSON without registered production marker prefixes' {
        $path = Join-Path $script:TempRoot 'plain.json'
        $script:Frames[0] | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path -Encoding utf8
        $result = Invoke-Validator $path
        $result.ExitCode | Should Not Be 0
    }
}
