Set-StrictMode -Version Latest

. (Join-Path $PSScriptRoot 'Load.ps1')

function Assert-FastTransferExactProperties {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string[]]$Names,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($null -eq $Value -or $Value -is [System.Array]) { throw "$Label must be one JSON object" }
    $actual = @($Value.PSObject.Properties.Name)
    if ($actual.Count -ne $Names.Count) { throw "$Label has missing or unknown fields" }
    foreach ($name in $actual) {
        if ($Names -cnotcontains $name) { throw "$Label has unknown field $name" }
    }
}

function Assert-FastTransferInteger {
    param($Value, [string]$Label, [decimal]$Minimum, [decimal]$Maximum)
    if ($null -eq $Value -or $Value -is [bool] -or $Value -is [string] -or
        $Value -is [single] -or $Value -is [double] -or $Value -is [decimal]) {
        throw "$Label must be an exact integral JSON number"
    }
    try { $number = [decimal]$Value } catch { throw "$Label must be an exact integral JSON number" }
    if ($number -ne [decimal]::Truncate($number) -or $number -lt $Minimum -or $number -gt $Maximum) {
        throw "$Label is outside $Minimum..$Maximum"
    }
}

function Assert-FastTransferNumber {
    param($Value, [string]$Label, [double]$Minimum, [double]$Maximum)
    if ($null -eq $Value -or $Value -is [bool] -or $Value -is [string]) {
        throw "$Label must be an exact JSON number"
    }
    try { $number = [double]$Value } catch { throw "$Label must be an exact JSON number" }
    if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or
        $number -lt $Minimum -or $number -gt $Maximum) {
        throw "$Label is non-finite or outside $Minimum..$Maximum"
    }
}

function Assert-FastTransferVector3 {
    param($Value, [string]$Label)
    if ($Value -isnot [System.Array] -or @($Value).Count -ne 3) {
        throw "$Label must contain exactly three numbers"
    }
    foreach ($index in 0..2) {
        Assert-FastTransferNumber $Value[$index] "$Label[$index]" -100000000.0 100000000.0
    }
}

function Assert-FastTransferVector2 {
    param($Value, [string]$Label)
    if ($Value -isnot [System.Array] -or @($Value).Count -ne 2) {
        throw "$Label must contain exactly two numbers"
    }
    foreach ($index in 0..1) {
        Assert-FastTransferNumber $Value[$index] "$Label[$index]" -1.0 1.0
    }
}

function ConvertFrom-FastTransferJson {
    param([string]$Json, [string]$Label)
    if ([string]::IsNullOrWhiteSpace($Json)) { throw "$Label JSON is empty" }
    try { return $Json | ConvertFrom-Json }
    catch { throw "$Label JSON is malformed" }
}

function Get-FastTransferTextSha256 {
    param([Parameter(Mandatory = $true)][string]$Text)
    $bytes = [Text.Encoding]::UTF8.GetBytes($Text)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { $digest = $hasher.ComputeHash($bytes) }
    finally { $hasher.Dispose() }
    return (($digest | ForEach-Object { $_.ToString('x2') }) -join '')
}

function Get-FastTransferHorizontalDistance {
    param([double[]]$From, [double[]]$To)
    $sum = 0.0
    foreach ($index in @(0, 2)) {
        $axis = $To[$index] - $From[$index]
        $sum += $axis * $axis
    }
    return [Math]::Sqrt($sum)
}

function Assert-FastTransferWitnessEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][string]$RunMetadataPath,
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][string]$ScenarioManifestPath,
        [Parameter(Mandatory = $true)][string]$OutputPath,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedBuildCommit,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedPregSha256,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedBregSha256,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedCoreSha256,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedAppSha256,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedAssetsSha256,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{32}$')][string]$ExpectedRunId,
        [Parameter(Mandatory = $true)][string]$ExpectedBridgeEndpoint,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$ExpectedCoreProcessId,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$ExpectedAppProcessId,
        [ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode = 'Fifo'
    )

    foreach ($path in @($LogPath, $RunMetadataPath, $MetricsPath, $ScenarioManifestPath)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "required witness artifact is missing: $path" }
    }
    $log = Get-Item -LiteralPath $LogPath
    if ($log.Length -gt 67108864) { throw 'FastTransferWitness client log exceeds the 64 MiB evidence bound' }

    $scenario = ConvertFrom-FastTransferJson (Get-Content -Raw -LiteralPath $ScenarioManifestPath) 'scenario manifest'
    Assert-FastTransferExactProperties $scenario @(
        'assets_sha256', 'maximum_command_to_reset_arm_milliseconds', 'minimum_duration_seconds',
        'minimum_post_reset_network_position_delta', 'required_command', 'scenario', 'schema',
        'screenshot_slots', 'target'
    ) 'scenario manifest'
    if ([string]$scenario.schema -cne 'rust-mcbe-fast-transfer-witness-scenario-v1' -or
        [string]$scenario.scenario -cne 'FastTransferWitness' -or
        [string]$scenario.target -cne 'Lbsg' -or
        [string]$scenario.required_command -cne '/transfer sm3' -or
        [string]$scenario.assets_sha256 -cne $ExpectedAssetsSha256) {
        throw 'FastTransferWitness scenario manifest identity is not exact'
    }
    Assert-FastTransferInteger $scenario.minimum_duration_seconds 'scenario.minimum_duration_seconds' 600 600
    Assert-FastTransferInteger $scenario.maximum_command_to_reset_arm_milliseconds `
        'scenario.maximum_command_to_reset_arm_milliseconds' 30000 30000
    Assert-FastTransferNumber $scenario.minimum_post_reset_network_position_delta `
        'scenario.minimum_post_reset_network_position_delta' 0.5 0.5

    $metadata = ConvertFrom-FastTransferJson (Get-Content -Raw -LiteralPath $RunMetadataPath) 'run metadata'
    Assert-FastTransferExactProperties $metadata @(
        'app_exit_code', 'app_process_id', 'app_sha256', 'assets_sha256', 'bridge_endpoint', 'build_commit',
        'core_exit_code', 'core_process_id', 'core_sha256', 'core_terminated_by_launcher',
        'duration_seconds', 'endpoint', 'run_id', 'scenario', 'schema', 'screenshot_slots', 'source_dirty',
        'target', 'timed_out'
    ) 'run metadata'
    if ([string]$metadata.schema -cne 'rust-mcbe-phase3-run-v1' -or
        [string]$metadata.run_id -cne $ExpectedRunId -or
        [string]$metadata.target -cne 'Lbsg' -or
        [string]$metadata.endpoint -cne 'play.lbsg.net:19132' -or
        [string]$metadata.bridge_endpoint -cne $ExpectedBridgeEndpoint -or
        [string]$metadata.build_commit -cne $ExpectedBuildCommit -or
        [string]$metadata.core_sha256 -cne $ExpectedCoreSha256 -or
        [string]$metadata.app_sha256 -cne $ExpectedAppSha256 -or
        [string]$metadata.assets_sha256 -cne $ExpectedAssetsSha256 -or
        [string]$metadata.scenario -cne 'FastTransferWitness') {
        throw 'FastTransferWitness run metadata attribution is not exact'
    }
    foreach ($field in @('source_dirty', 'core_terminated_by_launcher', 'timed_out')) {
        if ($metadata.$field -isnot [bool]) { throw "run metadata.$field must be an exact JSON boolean" }
    }
    Assert-FastTransferInteger $metadata.core_process_id 'run metadata.core_process_id' 1 ([decimal][int]::MaxValue)
    Assert-FastTransferInteger $metadata.app_process_id 'run metadata.app_process_id' 1 ([decimal][int]::MaxValue)
    Assert-FastTransferInteger $metadata.app_exit_code 'run metadata.app_exit_code' 0 0
    Assert-FastTransferInteger $metadata.duration_seconds 'run metadata.duration_seconds' 600 ([decimal][int]::MaxValue)
    if ([int]$metadata.core_process_id -ne $ExpectedCoreProcessId -or
        [int]$metadata.app_process_id -ne $ExpectedAppProcessId -or
        [bool]$metadata.source_dirty -or [bool]$metadata.timed_out) {
        throw 'FastTransferWitness process terminal metadata is not clean'
    }
    if ($null -ne $metadata.core_exit_code) {
        Assert-FastTransferInteger $metadata.core_exit_code 'run metadata.core_exit_code' 0 0
    }
    elseif (-not [bool]$metadata.core_terminated_by_launcher) {
        throw 'FastTransferWitness core has neither a clean exit nor launcher termination'
    }
    foreach ($slots in @($scenario.screenshot_slots, $metadata.screenshot_slots)) {
        if ($slots -isnot [System.Array] -or @($slots).Count -ne 2) {
            throw 'FastTransferWitness screenshot slots must contain exact before and after placeholders'
        }
        $expectedNames = @('fast-transfer-before.png', 'fast-transfer-after.png')
        for ($index = 0; $index -lt 2; $index++) {
            Assert-FastTransferExactProperties $slots[$index] @('filename', 'sha256') "screenshot_slots[$index]"
            if ([string]$slots[$index].filename -cne $expectedNames[$index] -or
                $null -ne $slots[$index].sha256) {
                throw 'FastTransferWitness screenshot slot identity changed or claimed an unverified hash'
            }
        }
    }

    $metrics = ConvertFrom-FastTransferJson (Get-Content -Raw -LiteralPath $MetricsPath) 'metrics'
    if ($null -eq $metrics.PSObject.Properties['world_ready'] -or $metrics.world_ready -isnot [bool] -or
        -not [bool]$metrics.world_ready) {
        throw 'FastTransferWitness metrics did not prove world-ready'
    }
    if ($null -eq $metrics.PSObject.Properties['assets'] -or
        $null -eq $metrics.assets.PSObject.Properties['blob_sha256'] -or
        [string]$metrics.assets.blob_sha256 -cne $ExpectedAssetsSha256) {
        throw 'FastTransferWitness runtime asset identity does not match the launched carrier'
    }

    $lines = @(Get-Content -LiteralPath $LogPath)
    $identityJson = [Collections.Generic.List[string]]::new()
    $terminals = [Collections.Generic.List[object]]::new()
    $frames = [Collections.Generic.List[object]]::new()
    $publications = [Collections.Generic.List[object]]::new()
    $actions = [Collections.Generic.List[object]]::new()
    $corrections = [Collections.Generic.List[object]]::new()
    $lastPublicationEvidence = $null
    for ($lineIndex = 0; $lineIndex -lt $lines.Count; $lineIndex++) {
        $line = [string]$lines[$lineIndex]
        if ($line.StartsWith('RUST_MCBE_PHASE3_VIOLATION=', [StringComparison]::Ordinal)) {
            throw 'FastTransferWitness client log contains a Phase 3 violation marker'
        }
        if ($line.StartsWith('RUST_MCBE_PHASE3_IDENTITY=', [StringComparison]::Ordinal)) {
            $identityJson.Add($line.Substring('RUST_MCBE_PHASE3_IDENTITY='.Length))
        }
        elseif ($line.StartsWith('RUST_MCBE_PHASE3_FRAME=', [StringComparison]::Ordinal)) {
            if ($frames.Count -eq 12000) { throw 'FastTransferWitness frame markers exceed 12000 records' }
            $frame = ConvertFrom-FastTransferJson $line.Substring('RUST_MCBE_PHASE3_FRAME='.Length) `
                "frame[$($frames.Count)]"
            $frames.Add([pscustomobject]@{ Record = $frame; LineIndex = $lineIndex })
        }
        elseif ($line.StartsWith('RUST_MCBE_PHASE3_TERMINAL=', [StringComparison]::Ordinal)) {
            $terminal = ConvertFrom-FastTransferJson `
                $line.Substring('RUST_MCBE_PHASE3_TERMINAL='.Length) "terminal[$($terminals.Count)]"
            $terminals.Add([pscustomobject]@{ Record = $terminal; LineIndex = $lineIndex })
        }
        elseif ($line.StartsWith('RUST_MCBE_FAST_TRANSFER_ACTION=', [StringComparison]::Ordinal)) {
            $action = ConvertFrom-FastTransferJson `
                $line.Substring('RUST_MCBE_FAST_TRANSFER_ACTION='.Length) "action[$($actions.Count)]"
            $actions.Add([pscustomobject]@{ Record = $action; LineIndex = $lineIndex })
        }
        elseif ($line.StartsWith('RUST_MCBE_PHASE3_EVENT=', [StringComparison]::Ordinal)) {
            $event = ConvertFrom-FastTransferJson $line.Substring('RUST_MCBE_PHASE3_EVENT='.Length) `
                "event line $lineIndex"
            if ([string]$event.kind -ceq 'correction') {
                $corrections.Add([pscustomobject]@{ Record = $event; LineIndex = $lineIndex })
            }
        }
        elseif ($line.StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)) {
            $publication = ConvertFrom-FastTransferJson $line.Substring('PHASE2_PUBLICATION='.Length) `
                "publication line $lineIndex"
            $lastPublicationEvidence = [pscustomobject]@{
                Record = $publication; LineIndex = $lineIndex; RawLine = $line; ObservedUnixMs = $null
            }
            $publications.Add($lastPublicationEvidence)
        }
        elseif ($line.StartsWith('RUST_MCBE_PHASE2_TIMING=', [StringComparison]::Ordinal)) {
            if ($null -eq $lastPublicationEvidence -or
                [int]$lastPublicationEvidence.LineIndex -ne ($lineIndex - 1) -or
                $null -ne $lastPublicationEvidence.ObservedUnixMs) {
                throw 'FastTransferWitness Phase 2 timing marker is not adjacent to one publication'
            }
            $timing = ConvertFrom-FastTransferJson $line.Substring('RUST_MCBE_PHASE2_TIMING='.Length) `
                "Phase 2 timing line $lineIndex"
            Assert-FastTransferExactProperties $timing @(
                'observed_unix_ms', 'publication_sha256', 'schema'
            ) "Phase 2 timing line $lineIndex"
            Assert-FastTransferInteger $timing.observed_unix_ms 'timing.observed_unix_ms' 1 ([decimal][uint64]::MaxValue)
            if ([string]$timing.schema -cne 'rust-mcbe-phase2-timing-v1' -or
                [string]$timing.publication_sha256 -cne
                    (Get-FastTransferTextSha256 $lastPublicationEvidence.RawLine)) {
                throw 'FastTransferWitness Phase 2 timing marker does not bind its exact publication'
            }
            $lastPublicationEvidence.ObservedUnixMs = [uint64]$timing.observed_unix_ms
        }
    }
    if ($identityJson.Count -ne 1 -or $terminals.Count -ne 1 -or $frames.Count -eq 0 -or
        $actions.Count -ne 1 -or $publications.Count -lt 4) {
        throw 'FastTransferWitness log lacks one exact identity, action, terminal, frames, or publication sequence'
    }
    if (@($publications | Where-Object { $null -eq $_.ObservedUnixMs }).Count -ne 0) {
        throw 'FastTransferWitness lacks exact timing for one or more Phase 2 publications'
    }

    $identity = ConvertFrom-FastTransferJson $identityJson[0] 'identity'
    Assert-FastTransferExactProperties $identity @(
        'app_process_id', 'breg_sha256', 'bridge_endpoint', 'build_commit', 'candidate_physics',
        'core_process_id', 'core_sha256', 'endpoint', 'preg_sha256', 'protocol', 'run_id',
        'schema', 'session_generation', 'source_dirty', 'target'
    ) 'identity'
    if ([string]$identity.schema -cne 'rust-mcbe-phase3-identity-v1' -or
        [string]$identity.build_commit -cne $ExpectedBuildCommit -or
        [string]$identity.target -cne 'Lbsg' -or [string]$identity.endpoint -cne 'play.lbsg.net:19132' -or
        [string]$identity.bridge_endpoint -cne $ExpectedBridgeEndpoint -or
        [string]$identity.preg_sha256 -cne $ExpectedPregSha256 -or
        [string]$identity.breg_sha256 -cne $ExpectedBregSha256 -or
        [string]$identity.core_sha256 -cne $ExpectedCoreSha256 -or
        [string]$identity.run_id -cne $ExpectedRunId) {
        throw 'FastTransferWitness production identity attribution is not exact'
    }
    Assert-FastTransferInteger $identity.protocol 'identity.protocol' 1001 1001
    Assert-FastTransferInteger $identity.session_generation 'identity.session_generation' 1 ([decimal][uint64]::MaxValue)
    Assert-FastTransferInteger $identity.core_process_id 'identity.core_process_id' 1 ([decimal][int]::MaxValue)
    Assert-FastTransferInteger $identity.app_process_id 'identity.app_process_id' 1 ([decimal][int]::MaxValue)
    if ($identity.candidate_physics -isnot [bool] -or -not [bool]$identity.candidate_physics -or
        $identity.source_dirty -isnot [bool] -or [bool]$identity.source_dirty -or
        [int]$identity.core_process_id -ne $ExpectedCoreProcessId -or
        [int]$identity.app_process_id -ne $ExpectedAppProcessId) {
        throw 'FastTransferWitness identity is dirty, non-candidate, or process-unattributed'
    }

    $actionEvidence = $actions[0]
    $action = $actionEvidence.Record
    Assert-FastTransferExactProperties $action @(
        'action_ordinal', 'command', 'kind', 'schema', 'sent_unix_ms', 'session_generation'
    ) 'action'
    if ([string]$action.schema -cne 'rust-mcbe-fast-transfer-action-v1' -or
        [string]$action.kind -cne 'command_sent' -or [string]$action.command -cne '/transfer sm3') {
        throw 'FastTransferWitness action marker is not the exact sent transfer command'
    }
    Assert-FastTransferInteger $action.session_generation 'action.session_generation' 1 ([decimal][uint64]::MaxValue)
    Assert-FastTransferInteger $action.action_ordinal 'action.action_ordinal' 0 ([decimal][uint64]::MaxValue)
    Assert-FastTransferInteger $action.sent_unix_ms 'action.sent_unix_ms' 1 ([decimal][uint64]::MaxValue)
    if ([uint64]$action.session_generation -ne [uint64]$identity.session_generation) {
        throw 'FastTransferWitness action session does not match Phase 3 identity'
    }

    $stablePublications = @($publications | Where-Object {
        $record = $_.Record
        [uint64]$record.publication.session_generation -eq [uint64]$identity.session_generation -and
        [bool]$record.publication.required_cohort_stable -and
        [bool]$record.publication.player_column_required -and
        [bool]$record.publication.player_column_loaded -and
        $null -ne $record.presentation.player_column.gpu_presented_subchunks -and
        [uint64]$record.presentation.player_column.gpu_presented_subchunks -gt 0
    })
    $preStable = @($stablePublications | Where-Object {
        [int]$_.LineIndex -lt [int]$actionEvidence.LineIndex
    } | Select-Object -Last 1)
    if ($preStable.Count -ne 1) { throw 'FastTransferWitness lacks one stable player-GPU publication before the command' }
    $preStable = $preStable[0]
    $baseArmed = [uint64]$preStable.Record.publication.local_reset.armed_count
    $baseConsumed = [uint64]$preStable.Record.publication.local_reset.consumed_count
    $resetAdvances = @($publications | Where-Object {
        [int]$_.LineIndex -gt [int]$actionEvidence.LineIndex -and
        ([uint64]$_.Record.publication.local_reset.armed_count -gt $baseArmed -or
            [uint64]$_.Record.publication.local_reset.consumed_count -gt $baseConsumed)
    })
    if ($resetAdvances.Count -eq 0) {
        throw 'FastTransferWitness command is not followed by a reset counter advance'
    }
    $arm = @($resetAdvances | Where-Object {
        [uint64]$_.Record.publication.session_generation -eq [uint64]$identity.session_generation -and
        [bool]$_.Record.publication.local_reset.armed -and
        [uint64]$_.Record.publication.local_reset.armed_count -eq ($baseArmed + 1) -and
        [uint64]$_.Record.publication.local_reset.consumed_count -eq $baseConsumed -and
        [uint64]$_.Record.publication.required_columns -eq 0
    } | Select-Object -First 1)
    if ($arm.Count -ne 1 -or [int]$arm[0].LineIndex -ne [int]$resetAdvances[0].LineIndex) {
        throw 'FastTransferWitness first reset after command is not one exact reset arm'
    }
    $arm = $arm[0]
    $commandToArmMilliseconds = [decimal][uint64]$arm.ObservedUnixMs - [decimal][uint64]$action.sent_unix_ms
    if ($commandToArmMilliseconds -lt 0 -or
        $commandToArmMilliseconds -gt [decimal]$scenario.maximum_command_to_reset_arm_milliseconds) {
        throw 'FastTransferWitness first reset arm is outside the 30-second command causality bound'
    }
    $consume = @($publications | Where-Object {
        [int]$_.LineIndex -gt [int]$arm.LineIndex -and
        [uint64]$_.Record.publication.session_generation -eq [uint64]$identity.session_generation -and
        -not [bool]$_.Record.publication.local_reset.armed -and
        [uint64]$_.Record.publication.local_reset.armed_count -eq ($baseArmed + 1) -and
        [uint64]$_.Record.publication.local_reset.consumed_count -eq ($baseConsumed + 1) -and
        [uint64]$_.Record.publication.publisher_epoch -gt [uint64]$arm.Record.publication.publisher_epoch
    } | Select-Object -First 1)
    if ($consume.Count -ne 1) { throw 'FastTransferWitness reset arm is not followed by one exact consume' }
    $consume = $consume[0]
    $terminalPublication = $publications[$publications.Count - 1]
    if ([int]$terminalPublication.LineIndex -le [int]$consume.LineIndex -or
        @($stablePublications | Where-Object { [int]$_.LineIndex -eq [int]$terminalPublication.LineIndex }).Count -ne 1) {
        throw 'FastTransferWitness terminal PHASE2 publication is not stable player-GPU evidence after consume'
    }
    $preColumn = $preStable.Record.publication.player_column
    $finalColumn = $terminalPublication.Record.publication.player_column
    $columnChanged = [int]$preColumn.dimension -ne [int]$finalColumn.dimension -or
        [int]$preColumn.x -ne [int]$finalColumn.x -or [int]$preColumn.z -ne [int]$finalColumn.z
    $centerChanged = (@($preStable.Record.publication.publisher_center) -join ',') -cne
        (@($terminalPublication.Record.publication.publisher_center) -join ',')
    if (-not $columnChanged -or -not $centerChanged) {
        throw 'FastTransferWitness command did not change the stable player column and publisher center'
    }
    if ([string]$preStable.Record.presentation.assets_manifest_sha256 -cne $ExpectedAssetsSha256 -or
        [string]$terminalPublication.Record.presentation.assets_manifest_sha256 -cne $ExpectedAssetsSha256) {
        throw 'FastTransferWitness Phase 2 stable publications do not match the launched asset carrier'
    }

    $frameProperties = @(
        'camera_blocked', 'camera_fallback', 'dimension', 'fifo_sequence', 'free_camera_packet_count',
        'grounded_after_tick', 'grounded_before_tick', 'input_mode', 'jump_held', 'jump_released',
        'jump_repeated', 'jump_started', 'local_avatar_visible', 'look_delta', 'movement',
        'network_position', 'outbound_authorized', 'outbox_depth', 'outbox_drops', 'perspective',
        'physics_tick', 'pose_generation', 'schema', 'session_generation'
    )
    $recoveryFrames = [Collections.Generic.List[object]]::new()
    $previousTick = $null
    foreach ($frameEvidence in $frames) {
        $frame = $frameEvidence.Record
        Assert-FastTransferExactProperties $frame $frameProperties 'frame'
        if ([string]$frame.schema -cne 'rust-mcbe-phase3-frame-v2') { throw 'FastTransferWitness frame schema is unsupported' }
        foreach ($field in @('session_generation', 'fifo_sequence', 'physics_tick', 'pose_generation')) {
            Assert-FastTransferInteger $frame.$field "frame.$field" 0 ([decimal][uint64]::MaxValue)
        }
        Assert-FastTransferInteger $frame.dimension 'frame.dimension' ([decimal][int32]::MinValue) ([decimal][int32]::MaxValue)
        Assert-FastTransferInteger $frame.outbox_depth 'frame.outbox_depth' 0 32
        Assert-FastTransferInteger $frame.outbox_drops 'frame.outbox_drops' 0 0
        Assert-FastTransferInteger $frame.free_camera_packet_count 'frame.free_camera_packet_count' 0 0
        Assert-FastTransferVector3 $frame.network_position 'frame.network_position'
        Assert-FastTransferVector2 $frame.movement 'frame.movement'
        if ($frame.outbound_authorized -isnot [bool] -or -not [bool]$frame.outbound_authorized -or
            [uint64]$frame.session_generation -ne [uint64]$identity.session_generation) {
            throw 'FastTransferWitness frame is not physics-authorized in the attributed session'
        }
        if ($null -ne $previousTick -and [uint64]$frame.physics_tick -ne ([uint64]$previousTick + 1)) {
            throw 'FastTransferWitness physics ticks are not one exact consecutive sequence'
        }
        $previousTick = [uint64]$frame.physics_tick
        $movementMagnitude = [Math]::Sqrt(
            [double]$frame.movement[0] * [double]$frame.movement[0] +
            [double]$frame.movement[1] * [double]$frame.movement[1]
        )
        if ([int]$frameEvidence.LineIndex -gt [int]$terminalPublication.LineIndex -and
            [int]$frame.dimension -eq [int]$terminalPublication.Record.publication.player_column.dimension) {
            $recoveryFrames.Add([pscustomobject]@{
                Record = $frame; LineIndex = [int]$frameEvidence.LineIndex
                MovementMagnitude = $movementMagnitude
                Position = [double[]]@(
                    [double]$frame.network_position[0], [double]$frame.network_position[1],
                    [double]$frame.network_position[2]
                )
            })
        }
    }
    if ($recoveryFrames.Count -lt 3) {
        throw 'FastTransferWitness lacks sent movement and settling frames after stable player-GPU recovery'
    }
    $settleFrame = $recoveryFrames[$recoveryFrames.Count - 1]
    if ([double]$settleFrame.MovementMagnitude -gt 0.0001) {
        throw 'FastTransferWitness final sent recovery frame is not a zero-input settle'
    }
    $movementFrames = @($recoveryFrames | Select-Object -First ($recoveryFrames.Count - 1) | Where-Object {
        [double]$_.MovementMagnitude -gt 0.0001
    })
    if ($movementFrames.Count -lt 2) {
        throw 'FastTransferWitness lacks two sent nonzero-input movement frames before settling'
    }
    $origin = $movementFrames[0].Position
    $maximumDelta = 0.0
    foreach ($movementFrame in $movementFrames) {
        $maximumDelta = [Math]::Max(
            $maximumDelta,
            (Get-FastTransferHorizontalDistance $origin $movementFrame.Position)
        )
    }
    if ($maximumDelta -lt [double]$scenario.minimum_post_reset_network_position_delta) {
        throw 'FastTransferWitness post-reset sent horizontal displacement is below 0.5 blocks'
    }
    $terminalEvidence = $terminals[0]
    $terminal = $terminalEvidence.Record
    Assert-FastTransferExactProperties $terminal @(
        'free_camera_packet_count', 'outbox_reconciliation', 'pending_outbox_depth',
        'physics_packet_count', 'schema', 'session_generation', 'source'
    ) 'terminal'
    foreach ($field in @('session_generation', 'physics_packet_count', 'free_camera_packet_count', 'pending_outbox_depth')) {
        Assert-FastTransferInteger $terminal.$field "terminal.$field" 0 ([decimal][uint64]::MaxValue)
    }
    if ([string]$terminal.schema -cne 'rust-mcbe-phase3-terminal-v1' -or
        [uint64]$terminal.session_generation -ne [uint64]$identity.session_generation -or
        [string]$terminal.source -cne 'Physics' -or [uint64]$terminal.physics_packet_count -eq 0 -or
        [uint64]$terminal.free_camera_packet_count -ne 0 -or [uint64]$terminal.pending_outbox_depth -ne 0 -or
        [string]$terminal.outbox_reconciliation -cne 'Drained') {
        throw 'FastTransferWitness terminal is not clean, drained, physics-authoritative, and FreeCamera-silent'
    }
    if ([uint64]$terminal.physics_packet_count -lt [uint64]$frames.Count) {
        throw 'FastTransferWitness terminal physics count is below its successfully sent frame evidence'
    }
    if ([int]$terminalEvidence.LineIndex -le [int]$settleFrame.LineIndex) {
        throw 'FastTransferWitness terminal evidence does not follow the final sent settle frame'
    }
    foreach ($correction in $corrections) {
        Assert-FastTransferInteger $correction.Record.session_generation `
            'correction.session_generation' 1 ([decimal][uint64]::MaxValue)
    }
    if (@($corrections | Where-Object {
        [uint64]$_.Record.session_generation -eq [uint64]$identity.session_generation -and
        [int]$_.LineIndex -gt [int]$terminalPublication.LineIndex -and
        [int]$_.LineIndex -lt [int]$terminalEvidence.LineIndex
    }).Count -ne 0) {
        throw 'FastTransferWitness recovery-through-terminal interval contains a same-session correction'
    }

    $resetEvidence = Get-Phase2LocalResetSequenceEvidence -ClientLogPath $LogPath `
        -ExpectedPresentMode $ExpectedPresentMode -ExpectedBuildProfile debug `
        -WorldReadyObserved:$true -Server Lbsg
    if ([uint64]$resetEvidence.FinalPublication.publication.session_generation -ne
        [uint64]$identity.session_generation) {
        throw 'FastTransferWitness Phase 2 final publication session does not match Phase 3 identity'
    }

    if ([uint64]$consume.ObservedUnixMs -lt [uint64]$arm.ObservedUnixMs -or
        [uint64]$terminalPublication.ObservedUnixMs -lt [uint64]$consume.ObservedUnixMs) {
        throw 'FastTransferWitness reset consume and stable recovery timing is retrograde'
    }
    $armToConsumeMilliseconds = [uint64]$consume.ObservedUnixMs - [uint64]$arm.ObservedUnixMs
    $consumeToRecoveryMilliseconds = [uint64]$terminalPublication.ObservedUnixMs - [uint64]$consume.ObservedUnixMs
    $prePublication = $preStable.Record.publication
    $finalPublication = $terminalPublication.Record.publication
    $finalPresentation = $terminalPublication.Record.presentation
    $result = [ordered]@{
        schema = 'rust-mcbe-fast-transfer-witness-v2'
        status = 'passed'
        run_id = $ExpectedRunId
        build_commit = $ExpectedBuildCommit
        target = 'Lbsg'
        endpoint = 'play.lbsg.net:19132'
        duration_seconds = [int]$metadata.duration_seconds
        phase2_snapshot_count = [int]$resetEvidence.SnapshotCount
        phase2_first_stalled_stage = [string]$resetEvidence.FirstStalledStage
        post_reset_frame_count = $recoveryFrames.Count
        post_reset_network_position_delta = $maximumDelta
        post_reset_physics_send_count = $recoveryFrames.Count
        assets_sha256 = $ExpectedAssetsSha256
        screenshot_slots = $metadata.screenshot_slots
        action_reset_recovery_timing = [ordered]@{
            command_sent_unix_ms = [uint64]$action.sent_unix_ms
            reset_arm_unix_ms = [uint64]$arm.ObservedUnixMs
            reset_consume_unix_ms = [uint64]$consume.ObservedUnixMs
            stable_recovery_unix_ms = [uint64]$terminalPublication.ObservedUnixMs
            command_to_arm_milliseconds = [uint64]$commandToArmMilliseconds
            arm_to_consume_milliseconds = $armToConsumeMilliseconds
            consume_to_recovery_milliseconds = $consumeToRecoveryMilliseconds
            causality = 'successful_command_packet_send_then_first_reset_arm_within_30000ms'
        }
        terminal_publication = [ordered]@{
            session_generation = [uint64]$finalPublication.session_generation
            publisher_epoch = [uint64]$finalPublication.publisher_epoch
            publisher_center = @($finalPublication.publisher_center)
            player_column = $finalPublication.player_column
            required_columns = [uint64]$finalPublication.required_columns
            loaded_required_columns = [uint64]$finalPublication.loaded_required_columns
            player_column_required = [bool]$finalPublication.player_column_required
            player_column_loaded = [bool]$finalPublication.player_column_loaded
            required_cohort_stable = [bool]$finalPublication.required_cohort_stable
        }
        terminal_deltas = [ordered]@{
            inactive_level_chunks = [uint64]$finalPublication.inactive_level_chunks - [uint64]$prePublication.inactive_level_chunks
            stale_outcomes = [uint64]$finalPublication.outcomes.stale - [uint64]$prePublication.outcomes.stale
            timed_out_outcomes = [uint64]$finalPublication.outcomes.timed_out - [uint64]$prePublication.outcomes.timed_out
        }
        terminal_queue_dispatch = [ordered]@{
            request_queue = $finalPublication.request_queue
            reset_dispatch_classes = @($finalPublication.local_reset.dispatch_classes)
            reset_dispatch_count = [uint64]$finalPublication.local_reset.dispatch_count
            reset_dispatch_total = [uint64]$finalPublication.local_reset.dispatch_total
            reset_dispatch_trace_overflowed = [bool]$finalPublication.local_reset.dispatch_trace_overflowed
        }
        terminal_presentation = [ordered]@{
            graphics_identity_sha256 = [string]$finalPresentation.graphics_identity_sha256
            assets_manifest_sha256 = [string]$finalPresentation.assets_manifest_sha256
            requested_present_mode = [string]$finalPresentation.requested_present_mode
            effective_present_mode = [string]$finalPresentation.effective_present_mode
            player_column = $finalPresentation.player_column
        }
        final_settle = [ordered]@{
            physics_tick = [uint64]$settleFrame.Record.physics_tick
            network_position = @($settleFrame.Record.network_position)
            movement = @($settleFrame.Record.movement)
        }
        terminal_physics_packet_count = [uint64]$terminal.physics_packet_count
        terminal_free_camera_packet_count = [uint64]$terminal.free_camera_packet_count
        terminal_pending_outbox_depth = [uint64]$terminal.pending_outbox_depth
        terminal_outbox_reconciliation = [string]$terminal.outbox_reconciliation
        log_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $LogPath).Hash.ToLowerInvariant()
    }
    $parent = Split-Path -Parent ([IO.Path]::GetFullPath($OutputPath))
    if (-not [string]::IsNullOrEmpty($parent)) { New-Item -ItemType Directory -Path $parent -Force | Out-Null }
    [IO.File]::WriteAllText(
        $OutputPath,
        ($result | ConvertTo-Json -Depth 6) + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    return [pscustomobject]$result
}
