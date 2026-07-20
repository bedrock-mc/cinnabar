function Write-Phase3FinalAggregate {
    param(
        [Parameter(Mandatory = $true)]$Identity,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Frames,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Events,
        [Parameter(Mandatory = $true)]$ScenarioManifest,
        [Parameter(Mandatory = $true)]$Terminal,
        [Parameter(Mandatory = $true)][string]$RunMetadataPath,
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][string]$OutputPath,
        [Parameter(Mandatory = $true)][string]$LogSha256
    )

    $metadata = Get-Content -Raw -LiteralPath $RunMetadataPath | ConvertFrom-Json
    $metadataFields = @(
        'schema', 'run_id', 'target', 'endpoint', 'bridge_endpoint', 'build_commit', 'source_dirty',
        'core_sha256', 'app_sha256', 'assets_sha256', 'core_process_id', 'app_process_id', 'app_exit_code', 'core_exit_code',
        'core_terminated_by_launcher', 'timed_out', 'duration_seconds', 'scenario', 'screenshot_slots'
    )
    Assert-ExactProperties $metadata $metadataFields 'run metadata'
    if ([string]$metadata.schema -cne 'rust-mcbe-phase3-run-v1') {
        throw 'run metadata schema is unsupported'
    }
    foreach ($binding in @(
        @('run_id', [string]$Identity.run_id), @('target', [string]$Identity.target),
        @('endpoint', [string]$Identity.endpoint), @('build_commit', [string]$Identity.build_commit),
        @('bridge_endpoint', [string]$Identity.bridge_endpoint),
        @('core_sha256', [string]$Identity.core_sha256)
    )) {
        $name = [string]$binding[0]
        if ($metadata.$name -isnot [string] -or [string]$metadata.$name -cne [string]$binding[1]) {
            throw "run metadata $name does not match the in-process identity"
        }
    }
    Assert-Boolean $metadata.source_dirty 'run metadata.source_dirty'
    Assert-Boolean $metadata.core_terminated_by_launcher 'run metadata.core_terminated_by_launcher'
    Assert-Boolean $metadata.timed_out 'run metadata.timed_out'
    if ([bool]$metadata.source_dirty) { throw 'run metadata reports dirty source' }
    if ([bool]$metadata.timed_out) { throw 'Phase 3 app timed out' }
    Assert-Integer $metadata.core_process_id 'run metadata.core_process_id' 1 ([decimal][int]::MaxValue)
    Assert-Integer $metadata.app_process_id 'run metadata.app_process_id' 1 ([decimal][int]::MaxValue)
    Assert-Integer $metadata.app_exit_code 'run metadata.app_exit_code' 0 0
    Assert-Integer $metadata.duration_seconds 'run metadata.duration_seconds' 1 ([decimal][int]::MaxValue)
    if ($metadata.scenario -isnot [string] -or
        [string]$metadata.scenario -cne [string]$ScenarioManifest.scenario) {
        throw 'run metadata scenario does not match the scenario manifest'
    }
    if ([string]$metadata.target -cne 'Bds' -and [int]$metadata.duration_seconds -lt 300) {
        throw 'remote Phase 3 evidence must run for at least five minutes'
    }
    if ([int]$metadata.core_process_id -ne [int]$Identity.core_process_id -or
        [int]$metadata.app_process_id -ne [int]$Identity.app_process_id) {
        throw 'run metadata process IDs do not match the in-process identity'
    }
    if ($metadata.app_sha256 -isnot [string] -or [string]$metadata.app_sha256 -cnotmatch '^[0-9a-f]{64}$') {
        throw 'run metadata app_sha256 is invalid'
    }
    if ($null -ne $metadata.assets_sha256 -and
        ($metadata.assets_sha256 -isnot [string] -or [string]$metadata.assets_sha256 -cnotmatch '^[0-9a-f]{64}$')) {
        throw 'run metadata assets_sha256 is invalid'
    }
    if ($metadata.screenshot_slots -isnot [System.Array]) {
        throw 'run metadata screenshot_slots must be one JSON array'
    }
    if ($null -ne $metadata.core_exit_code) {
        Assert-Integer $metadata.core_exit_code 'run metadata.core_exit_code' ([int]::MinValue) ([int]::MaxValue)
        if ([int]$metadata.core_exit_code -ne 0) { throw 'Phase 3 core exited with a nonzero code' }
    }
    elseif (-not [bool]$metadata.core_terminated_by_launcher) {
        throw 'Phase 3 core has neither a recorded exit nor launcher termination'
    }

    $metrics = Get-Content -Raw -LiteralPath $MetricsPath | ConvertFrom-Json
    foreach ($field in @(
        'session_seconds', 'frame_count', 'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms',
        'max_frame_ms', 'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks',
        'decode_error_count', 'gpu_upload_bytes'
    )) {
        if ($null -eq $metrics.PSObject.Properties[$field]) {
            throw "Phase 3 metrics are missing $field"
        }
    }
    Assert-Number $metrics.session_seconds 'metrics.session_seconds' 0.001 ([double]::MaxValue)
    Assert-Integer $metrics.frame_count 'metrics.frame_count' 1 ([decimal][uint64]::MaxValue)
    foreach ($field in @('p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms')) {
        Assert-Number $metrics.$field "metrics.$field" 0.0 ([double]::MaxValue)
    }
    foreach ($field in @(
        'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks', 'gpu_upload_bytes'
    )) {
        Assert-Integer $metrics.$field "metrics.$field" 0 ([decimal][uint64]::MaxValue)
    }
    Assert-Integer $metrics.decode_error_count 'metrics.decode_error_count' 0 0

    $corrections = @($Events | Where-Object { [string]$_.kind -ceq 'correction' })
    $replayed = @($corrections | Where-Object { [string]$_.correction_outcome -ceq 'replayed' })
    $snapped = @($corrections | Where-Object { [string]$_.correction_outcome -ceq 'snapped' })
    $maxCorrectionMagnitude = if ($corrections.Count -eq 0) {
        0.0
    }
    else {
        [double](($corrections | Measure-Object -Property correction_magnitude -Maximum).Maximum)
    }
    $inputModes = [Collections.Generic.List[string]]::new()
    $perspectiveSequence = [Collections.Generic.List[string]]::new()
    $heldJumpLongest = 0
    $heldJumpCurrent = 0
    foreach ($frame in $Frames) {
        if (-not $inputModes.Contains([string]$frame.input_mode)) { $inputModes.Add([string]$frame.input_mode) }
        if ($perspectiveSequence.Count -eq 0 -or
            [string]$perspectiveSequence[$perspectiveSequence.Count - 1] -cne [string]$frame.perspective) {
            $perspectiveSequence.Add([string]$frame.perspective)
        }
        if ([bool]$frame.jump_held) {
            $heldJumpCurrent++
            $heldJumpLongest = [Math]::Max($heldJumpLongest, $heldJumpCurrent)
        }
        else {
            $heldJumpCurrent = 0
        }
    }
    $candidateScenario = [string]$ScenarioManifest.scenario -ceq 'CandidatePhysics'
    Assert-Integer $Terminal.pending_outbox_depth 'terminal.pending_outbox_depth' 0 0
    $expectedTerminalReconciliation = if ($candidateScenario) { 'Drained' } else { 'NotAuthoritative' }
    if ($Terminal.outbox_reconciliation -isnot [string] -or
        [string]$Terminal.outbox_reconciliation -cne $expectedTerminalReconciliation) {
        throw "terminal outbox reconciliation must finish as $expectedTerminalReconciliation"
    }
    $outboxHighWater = if ($Frames.Count -eq 0) { [uint64]0 } else {
        [uint64](($Frames | Measure-Object -Property outbox_depth -Maximum).Maximum)
    }
    $outboxDrops = if ($Frames.Count -eq 0) { [uint64]0 } else {
        [uint64](($Frames | Measure-Object -Property outbox_drops -Maximum).Maximum)
    }
    $freeCameraPacketCount = if ($Frames.Count -eq 0) { [uint64]0 } else {
        [uint64](($Frames | Measure-Object -Property free_camera_packet_count -Maximum).Maximum)
    }
    $flatWalkThreshold = [double]$ScenarioManifest.required_controlled_matrix.flat_walk_min_magnitude
    $diagonalWalkThreshold = [double]$ScenarioManifest.required_controlled_matrix.diagonal_walk_min_axis_magnitude
    $movementEpsilon = 0.000000001
    $flatWalkWitnesses = @($Frames | Where-Object {
        $x = [Math]::Abs([double]$_.movement[0])
        $z = [Math]::Abs([double]$_.movement[1])
        $groundedNonJump = [bool]$_.grounded_before_tick -and [bool]$_.grounded_after_tick -and
            -not [bool]$_.jump_held -and -not [bool]$_.jump_started -and
            -not [bool]$_.jump_repeated -and -not [bool]$_.jump_released
        $oneAxisOnly = ($x -gt $movementEpsilon) -xor ($z -gt $movementEpsilon)
        $groundedNonJump -and $oneAxisOnly -and
            [Math]::Sqrt(($x * $x) + ($z * $z)) -ge $flatWalkThreshold
    })
    $diagonalWalkWitnesses = @($Frames | Where-Object {
        $x = [Math]::Abs([double]$_.movement[0])
        $z = [Math]::Abs([double]$_.movement[1])
        $groundedNonJump = [bool]$_.grounded_before_tick -and [bool]$_.grounded_after_tick -and
            -not [bool]$_.jump_held -and -not [bool]$_.jump_started -and
            -not [bool]$_.jump_repeated -and -not [bool]$_.jump_released
        $groundedNonJump -and $x -gt $movementEpsilon -and $z -gt $movementEpsilon -and
            $x -ge $diagonalWalkThreshold -and $z -ge $diagonalWalkThreshold
    })
    $singleJumpNonRepeatedWitnesses = @($Frames | Where-Object {
        [bool]$_.jump_started -and -not [bool]$_.jump_repeated -and [bool]$_.jump_held -and
            [bool]$_.grounded_before_tick
    })
    if ($candidateScenario) {
        if ($flatWalkThreshold -ne 0.25 -or $diagonalWalkThreshold -ne 0.25 -or
            [int]$ScenarioManifest.required_controlled_matrix.single_jump_non_repeated_min_count -ne 1) {
            throw 'CandidatePhysics controlled movement minima are not the exact required values'
        }
        foreach ($binding in @(
            @('camera_wall_outcome', 'WallBlocked'),
            @('camera_corner_outcome', 'CornerBlocked'),
            @('camera_ceiling_outcome', 'CeilingBlocked')
        )) {
            $name = [string]$binding[0]
            if ([string]$ScenarioManifest.required_controlled_matrix.$name -cne [string]$binding[1]) {
                throw "CandidatePhysics controlled matrix lacks the exact $name outcome"
            }
        }
        if ($flatWalkWitnesses.Count -lt 1) {
            throw 'required grounded flat walk has no nonzero production witness'
        }
        if ($diagonalWalkWitnesses.Count -lt 1) {
            throw 'required grounded diagonal walk has no nonzero production witness'
        }
        if ($singleJumpNonRepeatedWitnesses.Count -lt
            [int]$ScenarioManifest.required_controlled_matrix.single_jump_non_repeated_min_count) {
            throw 'required non-repeated single jump has no production witness'
        }
        foreach ($requiredMode in @($ScenarioManifest.required_input_modes)) {
            if (-not $inputModes.Contains([string]$requiredMode)) {
                throw "required input mode $requiredMode has no production witness"
            }
        }
        $requiredPerspectives = @($ScenarioManifest.required_perspective_sequence)
        if ($perspectiveSequence.Count -ne $requiredPerspectives.Count) {
            throw 'production perspective sequence does not match the scenario manifest'
        }
        for ($index = 0; $index -lt $requiredPerspectives.Count; $index++) {
            if ([string]$perspectiveSequence[$index] -cne [string]$requiredPerspectives[$index]) {
                throw 'production perspective sequence does not match the scenario manifest'
            }
        }
        if ([bool]$ScenarioManifest.require_replay -and $replayed.Count -eq 0) {
            throw 'required replay correction has no production witness'
        }
        if ([bool]$ScenarioManifest.require_snap -and $snapped.Count -eq 0) {
            throw 'required snap correction has no production witness'
        }
        if ([bool]$ScenarioManifest.require_held_jump_rejump -and
            @($Frames | Where-Object {
                [bool]$_.jump_repeated -and [bool]$_.jump_started -and [bool]$_.jump_held -and
                [bool]$_.grounded_before_tick
            }).Count -eq 0) {
            throw 'required held-jump landing and re-jump has no production witness'
        }
        if ([bool]$ScenarioManifest.require_release_before_landing -and
            @($Frames | Where-Object {
                [bool]$_.jump_released -and -not [bool]$_.jump_held -and
                -not [bool]$_.grounded_before_tick -and -not [bool]$_.grounded_after_tick
            }).Count -eq 0) {
            throw 'required release-before-landing behavior has no production witness'
        }
        if ([bool]$ScenarioManifest.require_camera_blocked -and
            @($Frames | Where-Object { [bool]$_.camera_blocked }).Count -eq 0) {
            throw 'required camera obstruction has no production witness'
        }
        if ([bool]$ScenarioManifest.require_camera_fallback -and
            @($Frames | Where-Object { [bool]$_.camera_fallback }).Count -eq 0) {
            throw 'required camera fallback has no production witness'
        }
        if ([bool]$ScenarioManifest.require_avatar_visibility_states -and
            (@($Frames | Where-Object { [bool]$_.local_avatar_visible }).Count -eq 0 -or
                @($Frames | Where-Object { -not [bool]$_.local_avatar_visible }).Count -eq 0)) {
            throw 'required avatar visibility states do not both have production witnesses'
        }
    }
    else {
        if ($flatWalkThreshold -ne 0.0 -or $diagonalWalkThreshold -ne 0.0 -or
            [int]$ScenarioManifest.required_controlled_matrix.single_jump_non_repeated_min_count -ne 0) {
            throw 'FreeCameraSilence controlled movement minima must remain zero'
        }
        foreach ($name in @('camera_wall_outcome', 'camera_corner_outcome', 'camera_ceiling_outcome')) {
            if ([string]$ScenarioManifest.required_controlled_matrix.$name -cne 'NotRequired') {
                throw "FreeCameraSilence controlled matrix cannot require $name"
            }
        }
    }
    $sessionSeconds = [double]$metrics.session_seconds
    $frameCount = [uint64]$metrics.frame_count
    $aggregate = [ordered]@{
        schema = 'rust-mcbe-phase3-final-v1'
        status = 'valid'
        scenario = [string]$ScenarioManifest.scenario
        identity = [ordered]@{
            build_commit = [string]$Identity.build_commit
            target = [string]$Identity.target
            endpoint = [string]$Identity.endpoint
            bridge_endpoint = [string]$Identity.bridge_endpoint
            run_id = [string]$Identity.run_id
            session_generation = [uint64]$Identity.session_generation
            preg_sha256 = [string]$Identity.preg_sha256
            breg_sha256 = [string]$Identity.breg_sha256
            core_sha256 = [string]$Identity.core_sha256
            app_sha256 = [string]$metadata.app_sha256
            source_dirty = $false
        }
        candidate = [ordered]@{
            candidate_physics = [bool]$Identity.candidate_physics
            production_physics_default_enabled = $false
        }
        movement = [ordered]@{
            input_modes = @($inputModes)
            tick_first = if ($Frames.Count -eq 0) { $null } else { [uint64]$Frames[0].physics_tick }
            tick_last = if ($Frames.Count -eq 0) { $null } else { [uint64]$Frames[$Frames.Count - 1].physics_tick }
            tick_count = [uint64]$Frames.Count
            correction_count = [uint64]$corrections.Count
            replay_count = [uint64]$replayed.Count
            snap_count = [uint64]$snapped.Count
            max_correction_magnitude = $maxCorrectionMagnitude
            outbox_high_water = $outboxHighWater
            outbox_drops = $outboxDrops
            free_camera_packet_count = $freeCameraPacketCount
            held_jump_frame_count = [uint64]@($Frames | Where-Object { [bool]$_.jump_held }).Count
            held_jump_longest_run = [uint64]$heldJumpLongest
            held_jump_rejump_count = [uint64]@($Frames | Where-Object { [bool]$_.jump_repeated }).Count
            flat_walk_witness_count = [uint64]$flatWalkWitnesses.Count
            diagonal_walk_witness_count = [uint64]$diagonalWalkWitnesses.Count
            single_jump_non_repeated_count = [uint64]$singleJumpNonRepeatedWitnesses.Count
            release_before_landing_count = [uint64]@($Frames | Where-Object {
                [bool]$_.jump_released -and -not [bool]$_.grounded_before_tick -and
                -not [bool]$_.grounded_after_tick
            }).Count
        }
        camera_avatar = [ordered]@{
            perspective_sequence = @($perspectiveSequence)
            camera_blocked_count = [uint64]@($Frames | Where-Object { [bool]$_.camera_blocked }).Count
            camera_fallback_count = [uint64]@($Frames | Where-Object { [bool]$_.camera_fallback }).Count
            avatar_visible_count = [uint64]@($Frames | Where-Object { [bool]$_.local_avatar_visible }).Count
            avatar_hidden_count = [uint64]@($Frames | Where-Object { -not [bool]$_.local_avatar_visible }).Count
        }
        required_controlled_matrix = [ordered]@{
            sprint = [bool]$ScenarioManifest.required_controlled_matrix.sprint
            sneak_ledge = [bool]$ScenarioManifest.required_controlled_matrix.sneak_ledge
            slabs_stairs = [bool]$ScenarioManifest.required_controlled_matrix.slabs_stairs
            ladder = [bool]$ScenarioManifest.required_controlled_matrix.ladder
            liquids = @($ScenarioManifest.required_controlled_matrix.liquids)
            special_surfaces = @($ScenarioManifest.required_controlled_matrix.special_surfaces)
            knockback = [bool]$ScenarioManifest.required_controlled_matrix.knockback
            teleport = [bool]$ScenarioManifest.required_controlled_matrix.teleport
            dimension_change = [bool]$ScenarioManifest.required_controlled_matrix.dimension_change
            focus_loss = [bool]$ScenarioManifest.required_controlled_matrix.focus_loss
            controller_disconnect = [bool]$ScenarioManifest.required_controlled_matrix.controller_disconnect
            frame_caps = @($ScenarioManifest.required_controlled_matrix.frame_caps)
            targeting_ray_invariant = [bool]$ScenarioManifest.required_controlled_matrix.targeting_ray_invariant
            flat_walk_min_magnitude = [double]$ScenarioManifest.required_controlled_matrix.flat_walk_min_magnitude
            diagonal_walk_min_axis_magnitude = [double]$ScenarioManifest.required_controlled_matrix.diagonal_walk_min_axis_magnitude
            single_jump_non_repeated_min_count = [uint64]$ScenarioManifest.required_controlled_matrix.single_jump_non_repeated_min_count
            camera_wall_outcome = [string]$ScenarioManifest.required_controlled_matrix.camera_wall_outcome
            camera_corner_outcome = [string]$ScenarioManifest.required_controlled_matrix.camera_corner_outcome
            camera_ceiling_outcome = [string]$ScenarioManifest.required_controlled_matrix.camera_ceiling_outcome
        }
        performance = [ordered]@{
            session_seconds = $sessionSeconds
            frame_count = $frameCount
            average_fps = $frameCount / $sessionSeconds
            p50_frame_ms = [double]$metrics.p50_frame_ms
            p95_frame_ms = [double]$metrics.p95_frame_ms
            p99_frame_ms = [double]$metrics.p99_frame_ms
            max_frame_ms = [double]$metrics.max_frame_ms
        }
        resources = [ordered]@{
            rendered_sub_chunks = [uint64]$metrics.rendered_sub_chunks
            resident_sub_chunks = [uint64]$metrics.resident_sub_chunks
            visible_sub_chunks = [uint64]$metrics.visible_sub_chunks
            gpu_upload_bytes = [uint64]$metrics.gpu_upload_bytes
            decode_error_count = [uint64]$metrics.decode_error_count
        }
        process = [ordered]@{
            core_process_id = [uint64]$metadata.core_process_id
            app_process_id = [uint64]$metadata.app_process_id
            app_exit_code = [int]$metadata.app_exit_code
            core_exit_code = $metadata.core_exit_code
            core_terminated_by_launcher = [bool]$metadata.core_terminated_by_launcher
            timed_out = [bool]$metadata.timed_out
        }
        evidence = [ordered]@{
            log_sha256 = $LogSha256
            frame_record_count = [uint64]$Frames.Count
            event_record_count = [uint64]$Events.Count
            terminal_source = [string]$Terminal.source
            terminal_physics_packet_count = [uint64]$Terminal.physics_packet_count
            terminal_free_camera_packet_count = [uint64]$Terminal.free_camera_packet_count
            terminal_pending_outbox_depth = [uint64]$Terminal.pending_outbox_depth
            terminal_outbox_reconciliation = [string]$Terminal.outbox_reconciliation
        }
    }
    $parent = Split-Path -Parent ([IO.Path]::GetFullPath($OutputPath))
    if (-not (Test-Path -LiteralPath $parent -PathType Container)) {
        New-Item -ItemType Directory -Path $parent -Force | Out-Null
    }
    $json = $aggregate | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText([IO.Path]::GetFullPath($OutputPath), $json + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    return $aggregate
}
