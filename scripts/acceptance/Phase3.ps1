[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$LogPath,

    [Parameter(Mandatory = $true)]
    [ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg')]
    [string]$ExpectedTarget,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{40}$')]
    [string]$ExpectedBuildCommit,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{64}$')]
    [string]$ExpectedPregSha256,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{64}$')]
    [string]$ExpectedBregSha256,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{32}$')]
    [string]$ExpectedRunId,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[^\s:]+:[1-9][0-9]{0,4}$')]
    [string]$ExpectedEndpoint,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[^\s:]+:[1-9][0-9]{0,4}$')]
    [string]$ExpectedBridgeEndpoint,

    [Parameter(Mandatory = $true)]
    [ValidatePattern('^[0-9a-f]{64}$')]
    [string]$ExpectedCoreSha256,

    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$ExpectedCoreProcessId,

    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$ExpectedAppProcessId,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$RunMetadataPath,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$MetricsPath,

    [Parameter(Mandatory = $true)]
    [string]$OutputPath,

    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$ScenarioManifestPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$framePrefix = 'RUST_MCBE_PHASE3_FRAME='
$eventPrefix = 'RUST_MCBE_PHASE3_EVENT='
$identityPrefix = 'RUST_MCBE_PHASE3_IDENTITY='
$terminalPrefix = 'RUST_MCBE_PHASE3_TERMINAL='
$violationPrefix = 'RUST_MCBE_PHASE3_VIOLATION='
$identityProperties = @(
    'schema', 'build_commit', 'target', 'protocol', 'session_generation', 'preg_sha256',
    'breg_sha256', 'candidate_physics', 'source_dirty', 'run_id', 'endpoint',
    'bridge_endpoint', 'core_sha256', 'core_process_id', 'app_process_id'
)
$frameProperties = @(
    'schema', 'session_generation', 'fifo_sequence', 'physics_tick', 'pose_generation',
    'dimension', 'network_position', 'input_mode', 'perspective', 'camera_blocked', 'camera_fallback',
    'local_avatar_visible', 'movement', 'look_delta', 'jump_held', 'outbound_authorized',
    'outbox_depth', 'outbox_drops', 'free_camera_packet_count', 'grounded_before_tick',
    'grounded_after_tick', 'jump_started', 'jump_repeated', 'jump_released'
)
$eventProperties = @(
    'schema', 'kind', 'event_sequence', 'session_generation', 'fifo_sequence', 'physics_tick',
    'dimension'
)
$correctionEventProperties = @(
    'schema', 'kind', 'event_sequence', 'session_generation', 'fifo_sequence', 'physics_tick',
    'dimension', 'correction_outcome', 'corrected_tick', 'replayed_ticks',
    'correction_magnitude'
)
$terminalProperties = @(
    'schema', 'session_generation', 'source', 'physics_packet_count', 'free_camera_packet_count',
    'pending_outbox_depth', 'outbox_reconciliation'
)
$scenarioProperties = @(
    'schema', 'scenario', 'required_input_modes', 'required_perspective_sequence',
    'require_replay', 'require_snap', 'require_held_jump_rejump',
    'require_release_before_landing', 'require_camera_blocked', 'require_camera_fallback',
    'require_avatar_visibility_states', 'required_controlled_matrix'
)
$controlledMatrixProperties = @(
    'sprint', 'sneak_ledge', 'slabs_stairs', 'ladder', 'liquids', 'special_surfaces',
    'knockback', 'teleport', 'dimension_change', 'focus_loss', 'controller_disconnect',
    'frame_caps', 'targeting_ray_invariant', 'flat_walk_min_magnitude',
    'diagonal_walk_min_axis_magnitude', 'single_jump_non_repeated_min_count',
    'camera_wall_outcome', 'camera_corner_outcome', 'camera_ceiling_outcome'
)

function Assert-ExactProperties {
    param($Value, [string[]]$Expected, [string]$Label)
    if ($null -eq $Value -or $Value -is [System.Array]) {
        throw "$Label must be one JSON object"
    }
    $actual = @($Value.PSObject.Properties.Name)
    if ($actual.Count -ne $Expected.Count) {
        throw "$Label has missing or unknown fields"
    }
    foreach ($name in $actual) {
        if ($Expected -cnotcontains $name) {
            throw "$Label has unknown field $name"
        }
    }
}

function Assert-ExactJsonKeys {
    param([string]$Json, [string[]]$Expected, [string]$Label)
    foreach ($name in $Expected) {
        $pattern = '"' + [regex]::Escape($name) + '"\s*:'
        if ([regex]::Matches($Json, $pattern).Count -ne 1) {
            throw "$Label has a missing or duplicate JSON key $name"
        }
    }
}

function ConvertFrom-ExactMarkerJson {
    param([string]$Json, [string[]]$Expected, [string]$Label)
    if ([string]::IsNullOrWhiteSpace($Json) -or $Json.Length -gt 4096) {
        throw "$Label JSON is empty or oversized"
    }
    Assert-ExactJsonKeys $Json $Expected $Label
    try {
        $value = $Json | ConvertFrom-Json
    }
    catch {
        throw "$Label JSON is malformed"
    }
    Assert-ExactProperties $value $Expected $Label
    return $value
}

function Assert-Integer {
    param($Value, [string]$Label, [decimal]$Minimum, [decimal]$Maximum)
    $integral = $Value -is [byte] -or $Value -is [uint16] -or $Value -is [uint32] -or
        $Value -is [uint64] -or $Value -is [sbyte] -or $Value -is [int16] -or
        $Value -is [int32] -or $Value -is [int64]
    if (-not $integral -or $Value -is [bool]) {
        throw "$Label must be an exact JSON integer"
    }
    $number = [decimal]$Value
    if ($number -lt $Minimum -or $number -gt $Maximum) {
        throw "$Label is outside $Minimum..$Maximum"
    }
}

function Assert-Number {
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

function Assert-Boolean {
    param($Value, [string]$Label)
    if ($Value -isnot [bool]) { throw "$Label must be an exact JSON boolean" }
}

function Assert-Vector2 {
    param($Value, [string]$Label, [double]$Minimum, [double]$Maximum)
    if ($Value -isnot [System.Array] -or @($Value).Count -ne 2) {
        throw "$Label must contain exactly two numbers"
    }
    for ($index = 0; $index -lt 2; $index++) {
        Assert-Number $Value[$index] "$Label[$index]" $Minimum $Maximum
    }
}

function Assert-Vector3 {
    param($Value, [string]$Label, [double]$Minimum, [double]$Maximum)
    if ($Value -isnot [System.Array] -or @($Value).Count -ne 3) {
        throw "$Label must contain exactly three numbers"
    }
    for ($index = 0; $index -lt 3; $index++) {
        Assert-Number $Value[$index] "$Label[$index]" $Minimum $Maximum
    }
}

function Assert-StringArray {
    param($Value, [string]$Label, [string[]]$Allowed, [int]$Maximum)
    if ($Value -isnot [System.Array] -or @($Value).Count -gt $Maximum) {
        throw "$Label must be a bounded JSON array"
    }
    $seen = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($item in @($Value)) {
        if ($item -isnot [string] -or $Allowed -cnotcontains [string]$item) {
            throw "$Label contains an unsupported value"
        }
        if (-not $seen.Add([string]$item)) { throw "$Label contains a duplicate value" }
    }
}

function Assert-OrderedStringArray {
    param($Value, [string]$Label, [string[]]$Allowed, [int]$Maximum)
    if ($Value -isnot [System.Array] -or @($Value).Count -gt $Maximum) {
        throw "$Label must be a bounded JSON array"
    }
    foreach ($item in @($Value)) {
        if ($item -isnot [string] -or $Allowed -cnotcontains [string]$item) {
            throw "$Label contains an unsupported value"
        }
    }
}

function Assert-IntegerArray {
    param($Value, [string]$Label, [int[]]$Allowed, [int]$Maximum)
    if ($Value -isnot [System.Array] -or @($Value).Count -gt $Maximum) {
        throw "$Label must be a bounded JSON array"
    }
    $seen = [Collections.Generic.HashSet[int]]::new()
    foreach ($item in @($Value)) {
        Assert-Integer $item "$Label item" 1 ([decimal][int]::MaxValue)
        if ($Allowed -cnotcontains [int]$item) { throw "$Label contains an unsupported value" }
        if (-not $seen.Add([int]$item)) { throw "$Label contains a duplicate value" }
    }
}

function Assert-ExactSequence {
    param($Actual, [object[]]$Expected, [string]$Label)
    $items = @($Actual)
    if ($items.Count -ne $Expected.Count) { throw "$Label does not contain the exact required sequence" }
    for ($index = 0; $index -lt $Expected.Count; $index++) {
        if ([string]$items[$index] -cne [string]$Expected[$index]) {
            throw "$Label does not contain the exact required sequence"
        }
    }
}

$scenarioJson = Get-Content -Raw -LiteralPath $ScenarioManifestPath
if ([string]::IsNullOrWhiteSpace($scenarioJson) -or $scenarioJson.Length -gt 4096) {
    throw 'Phase 3 scenario manifest is empty or oversized'
}
Assert-ExactJsonKeys $scenarioJson $scenarioProperties 'scenario manifest'
Assert-ExactJsonKeys $scenarioJson $controlledMatrixProperties 'scenario manifest.required_controlled_matrix'
try { $scenarioManifest = $scenarioJson | ConvertFrom-Json }
catch { throw 'Phase 3 scenario manifest JSON is malformed' }
Assert-ExactProperties $scenarioManifest $scenarioProperties 'scenario manifest'
Assert-ExactProperties $scenarioManifest.required_controlled_matrix $controlledMatrixProperties `
    'scenario manifest.required_controlled_matrix'
if ([string]$scenarioManifest.schema -cne 'rust-mcbe-phase3-scenario-v1') {
    throw 'scenario manifest schema is unsupported'
}
if ($scenarioManifest.scenario -isnot [string] -or
    [string]$scenarioManifest.scenario -cnotin @('CandidatePhysics', 'FreeCameraSilence')) {
    throw 'scenario manifest scenario is unsupported'
}
Assert-StringArray $scenarioManifest.required_input_modes 'scenario manifest.required_input_modes' `
    @('KeyboardMouse', 'GamePad', 'Touch') 3
Assert-OrderedStringArray $scenarioManifest.required_perspective_sequence `
    'scenario manifest.required_perspective_sequence' `
    @('FirstPerson', 'ThirdPersonBack', 'ThirdPersonFront') 4
foreach ($name in @(
    'require_replay', 'require_snap', 'require_held_jump_rejump',
    'require_release_before_landing', 'require_camera_blocked', 'require_camera_fallback',
    'require_avatar_visibility_states'
)) {
    Assert-Boolean $scenarioManifest.$name "scenario manifest.$name"
}
$controlledMatrix = $scenarioManifest.required_controlled_matrix
foreach ($name in @(
    'sprint', 'sneak_ledge', 'slabs_stairs', 'ladder', 'knockback', 'teleport',
    'dimension_change', 'focus_loss', 'controller_disconnect', 'targeting_ray_invariant'
)) {
    Assert-Boolean $controlledMatrix.$name "scenario manifest.required_controlled_matrix.$name"
}
Assert-StringArray $controlledMatrix.liquids 'scenario manifest.required_controlled_matrix.liquids' `
    @('Water', 'Lava') 2
Assert-StringArray $controlledMatrix.special_surfaces `
    'scenario manifest.required_controlled_matrix.special_surfaces' `
    @('Cobweb', 'Slime', 'Bed', 'SoulSand', 'Honey', 'BubbleColumn') 6
Assert-IntegerArray $controlledMatrix.frame_caps `
    'scenario manifest.required_controlled_matrix.frame_caps' @(30, 60, 144) 3
Assert-Number $controlledMatrix.flat_walk_min_magnitude `
    'scenario manifest.required_controlled_matrix.flat_walk_min_magnitude' 0.0 1.0
Assert-Number $controlledMatrix.diagonal_walk_min_axis_magnitude `
    'scenario manifest.required_controlled_matrix.diagonal_walk_min_axis_magnitude' 0.0 1.0
Assert-Integer $controlledMatrix.single_jump_non_repeated_min_count `
    'scenario manifest.required_controlled_matrix.single_jump_non_repeated_min_count' 0 1
foreach ($name in @('camera_wall_outcome', 'camera_corner_outcome', 'camera_ceiling_outcome')) {
    if ($controlledMatrix.$name -isnot [string] -or
        [string]$controlledMatrix.$name -cnotin @(
            'NotRequired', 'WallBlocked', 'CornerBlocked', 'CeilingBlocked'
        )) {
        throw "scenario manifest.required_controlled_matrix.$name is unsupported"
    }
}
$candidateScenario = [string]$scenarioManifest.scenario -ceq 'CandidatePhysics'
if ($candidateScenario) {
    $requiredModes = @($scenarioManifest.required_input_modes)
    if ($requiredModes.Count -ne 3 -or
        @(@('KeyboardMouse', 'GamePad', 'Touch') | Where-Object {
            $requiredModes -cnotcontains $_
        }).Count -ne 0) {
        throw 'CandidatePhysics scenario must require keyboard mouse gamepad and touch witnesses'
    }
    $requiredPerspectives = @($scenarioManifest.required_perspective_sequence)
    if ($requiredPerspectives.Count -ne 4 -or
        [string]$requiredPerspectives[0] -cne 'FirstPerson' -or
        [string]$requiredPerspectives[1] -cne 'ThirdPersonBack' -or
        [string]$requiredPerspectives[2] -cne 'ThirdPersonFront' -or
        [string]$requiredPerspectives[3] -cne 'FirstPerson') {
        throw 'CandidatePhysics scenario must require the exact First Back Front First wrap'
    }
    foreach ($name in @(
        'require_replay', 'require_snap', 'require_held_jump_rejump',
        'require_release_before_landing', 'require_camera_blocked', 'require_camera_fallback',
        'require_avatar_visibility_states'
    )) {
        if (-not [bool]$scenarioManifest.$name) {
            throw "CandidatePhysics scenario cannot disable $name"
        }
    }
    foreach ($name in @(
        'sprint', 'sneak_ledge', 'slabs_stairs', 'ladder', 'knockback', 'teleport',
        'dimension_change', 'focus_loss', 'controller_disconnect', 'targeting_ray_invariant'
    )) {
        if (-not [bool]$controlledMatrix.$name) {
            throw "CandidatePhysics scenario cannot disable required_controlled_matrix.$name"
        }
    }
    Assert-ExactSequence @($controlledMatrix.liquids) @('Water', 'Lava') `
        'scenario manifest.required_controlled_matrix.liquids'
    Assert-ExactSequence @($controlledMatrix.special_surfaces) `
        @('Cobweb', 'Slime', 'Bed', 'SoulSand', 'Honey', 'BubbleColumn') `
        'scenario manifest.required_controlled_matrix.special_surfaces'
    Assert-ExactSequence @($controlledMatrix.frame_caps) @(30, 60, 144) `
        'scenario manifest.required_controlled_matrix.frame_caps'
    if ([double]$controlledMatrix.flat_walk_min_magnitude -ne 0.25) {
        throw 'CandidatePhysics scenario must require a 0.25 flat-walk magnitude'
    }
    if ([double]$controlledMatrix.diagonal_walk_min_axis_magnitude -ne 0.25) {
        throw 'CandidatePhysics scenario must require 0.25 on each diagonal-walk axis'
    }
    if ([int]$controlledMatrix.single_jump_non_repeated_min_count -ne 1) {
        throw 'CandidatePhysics scenario must require one non-repeated single jump'
    }
    foreach ($binding in @(
        @('camera_wall_outcome', 'WallBlocked'),
        @('camera_corner_outcome', 'CornerBlocked'),
        @('camera_ceiling_outcome', 'CeilingBlocked')
    )) {
        $name = [string]$binding[0]
        if ([string]$controlledMatrix.$name -cne [string]$binding[1]) {
            throw "CandidatePhysics scenario must require the exact $name outcome"
        }
    }
}
else {
    if (@($scenarioManifest.required_input_modes).Count -ne 0 -or
        @($scenarioManifest.required_perspective_sequence).Count -ne 0) {
        throw 'FreeCameraSilence scenario cannot require movement frame witnesses'
    }
    foreach ($name in @(
        'require_replay', 'require_snap', 'require_held_jump_rejump',
        'require_release_before_landing', 'require_camera_blocked', 'require_camera_fallback',
        'require_avatar_visibility_states'
    )) {
        if ([bool]$scenarioManifest.$name) {
            throw "FreeCameraSilence scenario cannot enable $name"
        }
    }
    if (@($controlledMatrix.liquids).Count -ne 0 -or
        @($controlledMatrix.special_surfaces).Count -ne 0 -or
        @($controlledMatrix.frame_caps).Count -ne 0) {
        throw 'FreeCameraSilence scenario cannot require controlled movement matrix arrays'
    }
    foreach ($name in @(
        'sprint', 'sneak_ledge', 'slabs_stairs', 'ladder', 'knockback', 'teleport',
        'dimension_change', 'focus_loss', 'controller_disconnect', 'targeting_ray_invariant'
    )) {
        if ([bool]$controlledMatrix.$name) {
            throw "FreeCameraSilence scenario cannot enable required_controlled_matrix.$name"
        }
    }
    if ([double]$controlledMatrix.flat_walk_min_magnitude -ne 0.0 -or
        [double]$controlledMatrix.diagonal_walk_min_axis_magnitude -ne 0.0 -or
        [int]$controlledMatrix.single_jump_non_repeated_min_count -ne 0) {
        throw 'FreeCameraSilence scenario cannot require controlled movement witnesses'
    }
    foreach ($name in @('camera_wall_outcome', 'camera_corner_outcome', 'camera_ceiling_outcome')) {
        if ([string]$controlledMatrix.$name -cne 'NotRequired') {
            throw "FreeCameraSilence scenario cannot require $name"
        }
    }
}

$log = Get-Item -LiteralPath $LogPath
if ($log.Length -gt 67108864) {
    throw 'Phase 3 client log exceeds the 64 MiB evidence bound'
}
$frameJson = [Collections.Generic.List[string]]::new()
$eventJson = [Collections.Generic.List[string]]::new()
$identityJson = [Collections.Generic.List[string]]::new()
$terminalJson = [Collections.Generic.List[string]]::new()
foreach ($line in Get-Content -LiteralPath $LogPath) {
    if ($line.StartsWith($violationPrefix, [StringComparison]::Ordinal)) {
        throw 'Phase 3 client log contains a production evidence violation marker'
    }
    if ($line.StartsWith($identityPrefix, [StringComparison]::Ordinal)) {
        if ($identityJson.Count -eq 1) { throw 'Phase 3 log contains conflicting aggregate identities' }
        $identityJson.Add($line.Substring($identityPrefix.Length))
    }
    elseif ($line.StartsWith($framePrefix, [StringComparison]::Ordinal)) {
        if ($frameJson.Count -eq 12000) { throw 'Phase 3 frame markers exceed 12000 records' }
        $frameJson.Add($line.Substring($framePrefix.Length))
    }
    elseif ($line.StartsWith($eventPrefix, [StringComparison]::Ordinal)) {
        if ($eventJson.Count -eq 256) { throw 'Phase 3 event markers exceed 256 records' }
        $eventJson.Add($line.Substring($eventPrefix.Length))
    }
    elseif ($line.StartsWith($terminalPrefix, [StringComparison]::Ordinal)) {
        if ($terminalJson.Count -eq 1) { throw 'Phase 3 log contains conflicting terminal records' }
        $terminalJson.Add($line.Substring($terminalPrefix.Length))
    }
}
if ($identityJson.Count -ne 1) {
    throw 'Phase 3 client log must contain exactly one registered production identity'
}
if ($terminalJson.Count -ne 1) {
    throw 'Phase 3 client log must contain exactly one terminal record'
}
if ($candidateScenario -and $frameJson.Count -eq 0) {
    throw 'Phase 3 client log contains no registered production frame markers'
}
if (-not $candidateScenario -and ($frameJson.Count -ne 0 -or $eventJson.Count -ne 0)) {
    throw 'FreeCameraSilence evidence cannot contain movement frames or events'
}

$identity = ConvertFrom-ExactMarkerJson $identityJson[0] $identityProperties 'identity'
if ([string]$identity.schema -cne 'rust-mcbe-phase3-identity-v1') {
    throw 'identity schema is unsupported'
}
if ($identity.build_commit -isnot [string] -or
    [string]$identity.build_commit -cnotmatch '^[0-9a-f]{40}$' -or
    [string]$identity.build_commit -cne $ExpectedBuildCommit) {
    throw 'identity build commit does not match the exact requested build'
}
if ($identity.target -isnot [string] -or [string]$identity.target -cne $ExpectedTarget) {
    throw 'identity target does not match the exact requested server target'
}
Assert-Integer $identity.protocol 'identity.protocol' 1001 1001
Assert-Integer $identity.session_generation 'identity.session_generation' 1 ([decimal][uint64]::MaxValue)
foreach ($hash in @(
    @('preg_sha256', $ExpectedPregSha256),
    @('breg_sha256', $ExpectedBregSha256)
)) {
    $name = [string]$hash[0]
    $expected = [string]$hash[1]
    $actual = $identity.$name
    if ($actual -isnot [string] -or [string]$actual -cnotmatch '^[0-9a-f]{64}$' -or
        [string]$actual -cne $expected) {
        throw "identity $name does not match the exact requested registry"
    }
}
Assert-Boolean $identity.candidate_physics 'identity.candidate_physics'
if ([bool]$identity.candidate_physics -ne $candidateScenario) {
    throw 'identity candidate mode does not match the scenario manifest'
}
Assert-Boolean $identity.source_dirty 'identity.source_dirty'
if ([bool]$identity.source_dirty) { throw 'identity was compiled from dirty source' }
if ($identity.run_id -isnot [string] -or [string]$identity.run_id -cne $ExpectedRunId) {
    throw 'identity run_id does not match the launched run'
}
if ($identity.endpoint -isnot [string] -or [string]$identity.endpoint -cne $ExpectedEndpoint) {
    throw 'identity endpoint does not match the actual upstream endpoint'
}
if ($identity.bridge_endpoint -isnot [string] -or
    [string]$identity.bridge_endpoint -cne $ExpectedBridgeEndpoint) {
    throw 'identity bridge_endpoint does not match the core-published endpoint'
}
if ($identity.core_sha256 -isnot [string] -or
    [string]$identity.core_sha256 -cne $ExpectedCoreSha256) {
    throw 'identity core_sha256 does not match the launched core'
}
Assert-Integer $identity.core_process_id 'identity.core_process_id' 1 ([decimal][int]::MaxValue)
Assert-Integer $identity.app_process_id 'identity.app_process_id' 1 ([decimal][int]::MaxValue)
if ([int]$identity.core_process_id -ne $ExpectedCoreProcessId -or
    [int]$identity.app_process_id -ne $ExpectedAppProcessId) {
    throw 'identity process IDs do not match the launched processes'
}

$frames = [Collections.Generic.List[object]]::new()
for ($index = 0; $index -lt $frameJson.Count; $index++) {
    $label = "frame[$index]"
    $frame = ConvertFrom-ExactMarkerJson $frameJson[$index] $frameProperties $label
    if ([string]$frame.schema -cne 'rust-mcbe-phase3-frame-v2') { throw "$label schema is unsupported" }
    Assert-Integer $frame.session_generation "$label.session_generation" 1 ([decimal][uint64]::MaxValue)
    if ([uint64]$frame.session_generation -ne [uint64]$identity.session_generation) {
        throw "$label session does not match the aggregate identity"
    }
    Assert-Integer $frame.fifo_sequence "$label.fifo_sequence" 0 ([decimal][uint64]::MaxValue)
    Assert-Integer $frame.physics_tick "$label.physics_tick" 0 ([decimal][uint64]::MaxValue)
    Assert-Integer $frame.pose_generation "$label.pose_generation" 1 ([decimal][uint64]::MaxValue)
    Assert-Integer $frame.dimension "$label.dimension" ([int32]::MinValue) ([int32]::MaxValue)
    Assert-Vector3 $frame.network_position "$label.network_position" -100000000.0 100000000.0
    if ($frame.input_mode -isnot [string] -or
        [string]$frame.input_mode -cnotin @('KeyboardMouse', 'GamePad', 'Touch')) {
        throw "$label.input_mode is unsupported"
    }
    if ($frame.perspective -isnot [string] -or
        [string]$frame.perspective -cnotin @('FirstPerson', 'ThirdPersonBack', 'ThirdPersonFront')) {
        throw "$label.perspective is unsupported"
    }
    Assert-Vector2 $frame.movement "$label.movement" -1.0 1.0
    Assert-Vector2 $frame.look_delta "$label.look_delta" -64.0 64.0
    Assert-Boolean $frame.jump_held "$label.jump_held"
    Assert-Boolean $frame.camera_blocked "$label.camera_blocked"
    Assert-Boolean $frame.camera_fallback "$label.camera_fallback"
    Assert-Boolean $frame.local_avatar_visible "$label.local_avatar_visible"
    if ([bool]$frame.camera_blocked -and [bool]$frame.camera_fallback) {
        throw "$label cannot be both camera blocked and fallback"
    }
    $expectedAvatarVisible = [string]$frame.perspective -cne 'FirstPerson'
    if ([bool]$frame.local_avatar_visible -ne $expectedAvatarVisible) {
        throw "$label local avatar visibility disagrees with perspective"
    }
    Assert-Boolean $frame.outbound_authorized "$label.outbound_authorized"
    if (-not [bool]$frame.outbound_authorized) { throw "$label did not authorize physics outbound movement" }
    Assert-Integer $frame.outbox_depth "$label.outbox_depth" 0 32
    Assert-Integer $frame.outbox_drops "$label.outbox_drops" 0 0
    Assert-Integer $frame.free_camera_packet_count "$label.free_camera_packet_count" 0 0
    Assert-Boolean $frame.grounded_before_tick "$label.grounded_before_tick"
    Assert-Boolean $frame.grounded_after_tick "$label.grounded_after_tick"
    Assert-Boolean $frame.jump_started "$label.jump_started"
    Assert-Boolean $frame.jump_repeated "$label.jump_repeated"
    Assert-Boolean $frame.jump_released "$label.jump_released"
    if ([bool]$frame.jump_started -and [bool]$frame.jump_released) {
        throw "$label cannot start and release jump in the same tick"
    }
    if ([bool]$frame.jump_repeated -and
        (-not [bool]$frame.jump_started -or -not [bool]$frame.jump_held -or
            -not [bool]$frame.grounded_before_tick)) {
        throw "$label repeated jump lacks a held grounded jump start"
    }
    if ([bool]$frame.jump_released -and [bool]$frame.jump_held) {
        throw "$label released jump while reporting jump held"
    }
    if ($frames.Count -gt 0) {
        $previous = $frames[$frames.Count - 1]
        if ([uint64]$frame.session_generation -ne [uint64]$previous.session_generation) {
            throw 'Phase 3 evidence crosses a session boundary'
        }
        $dimensionChanged = [int32]$frame.dimension -ne [int32]$previous.dimension
        if (-not $dimensionChanged -and
            [uint64]$frame.physics_tick -ne ([uint64]$previous.physics_tick + 1)) {
            throw 'Phase 3 frame physics ticks are duplicate retrograde or gapped'
        }
        if ([uint64]$frame.fifo_sequence -lt [uint64]$previous.fifo_sequence) {
            throw 'Phase 3 frame FIFO sequence is retrograde'
        }
        if ([uint64]$frame.pose_generation -lt [uint64]$previous.pose_generation) {
            throw 'Phase 3 pose generation is retrograde'
        }
    }
    $frames.Add($frame)
}

$terminal = ConvertFrom-ExactMarkerJson $terminalJson[0] $terminalProperties 'terminal'
if ([string]$terminal.schema -cne 'rust-mcbe-phase3-terminal-v1') {
    throw 'terminal schema is unsupported'
}
Assert-Integer $terminal.session_generation 'terminal.session_generation' 1 ([decimal][uint64]::MaxValue)
if ([uint64]$terminal.session_generation -ne [uint64]$identity.session_generation) {
    throw 'terminal session does not match the aggregate identity'
}
if ($terminal.source -isnot [string] -or [string]$terminal.source -cnotin @('Physics', 'FreeCamera')) {
    throw 'terminal source is unsupported'
}
Assert-Integer $terminal.physics_packet_count 'terminal.physics_packet_count' 0 ([decimal][uint64]::MaxValue)
Assert-Integer $terminal.free_camera_packet_count 'terminal.free_camera_packet_count' 0 ([decimal][uint64]::MaxValue)
Assert-Integer $terminal.pending_outbox_depth 'terminal.pending_outbox_depth' 0 32
if ($terminal.outbox_reconciliation -isnot [string] -or
    [string]$terminal.outbox_reconciliation -cnotin @(
        'Drained', 'BudgetDeferred', 'TransportRestored', 'FullRestored', 'NotAuthoritative'
    )) {
    throw 'terminal outbox_reconciliation is unsupported'
}
if ($candidateScenario) {
    if ([string]$terminal.source -cne 'Physics' -or
        [uint64]$terminal.physics_packet_count -eq 0 -or
        [uint64]$terminal.free_camera_packet_count -ne 0 -or
        [uint64]$terminal.pending_outbox_depth -ne 0 -or
        [string]$terminal.outbox_reconciliation -cne 'Drained') {
        throw 'CandidatePhysics terminal does not prove Physics packet production'
    }
}
elseif ([string]$terminal.source -cne 'FreeCamera' -or
    [uint64]$terminal.physics_packet_count -ne 0 -or
    [uint64]$terminal.free_camera_packet_count -ne 0 -or
    [uint64]$terminal.pending_outbox_depth -ne 0 -or
    [string]$terminal.outbox_reconciliation -cne 'NotAuthoritative') {
    throw 'FreeCameraSilence terminal is not network silent'
}

$events = [Collections.Generic.List[object]]::new()
$eventIdentities = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
for ($index = 0; $index -lt $eventJson.Count; $index++) {
    $label = "event[$index]"
    try { $eventPreview = $eventJson[$index] | ConvertFrom-Json }
    catch { throw "$label JSON is malformed" }
    $expectedEventProperties = if ([string]$eventPreview.kind -ceq 'correction') {
        $correctionEventProperties
    }
    else {
        $eventProperties
    }
    $event = ConvertFrom-ExactMarkerJson $eventJson[$index] $expectedEventProperties $label
    if ([string]$event.schema -cne 'rust-mcbe-phase3-event-v1') { throw "$label schema is unsupported" }
    if ($event.kind -isnot [string] -or
        [string]$event.kind -cnotin @('correction', 'dimension', 'session')) {
        throw "$label.kind is unsupported"
    }
    Assert-Integer $event.session_generation "$label.session_generation" 1 ([decimal][uint64]::MaxValue)
    Assert-Integer $event.event_sequence "$label.event_sequence" 0 ([decimal][uint64]::MaxValue)
    Assert-Integer $event.fifo_sequence "$label.fifo_sequence" 0 ([decimal][uint64]::MaxValue)
    Assert-Integer $event.physics_tick "$label.physics_tick" 0 ([decimal][uint64]::MaxValue)
    Assert-Integer $event.dimension "$label.dimension" ([int32]::MinValue) ([int32]::MaxValue)
    if ([string]$event.kind -ceq 'correction') {
        if ($event.correction_outcome -isnot [string] -or
            [string]$event.correction_outcome -cnotin @('replayed', 'snapped')) {
            throw "$label correction_outcome is unsupported"
        }
        Assert-Integer $event.corrected_tick "$label.corrected_tick" 0 ([decimal][uint64]::MaxValue)
        Assert-Integer $event.replayed_ticks "$label.replayed_ticks" 0 256
        Assert-Number $event.correction_magnitude "$label.correction_magnitude" 0.0 100000000.0
        if ([string]$event.correction_outcome -ceq 'snapped' -and [uint64]$event.replayed_ticks -ne 0) {
            throw "$label snapped correction cannot report replayed ticks"
        }
    }
    $eventIdentity = '{0}:{1}' -f $event.session_generation, $event.event_sequence
    if (-not $eventIdentities.Add($eventIdentity)) { throw "$label duplicates a Phase 3 event identity" }
    $correlated = @($frames | Where-Object {
        [uint64]$_.session_generation -eq [uint64]$event.session_generation -and
        [uint64]$_.fifo_sequence -eq [uint64]$event.fifo_sequence -and
        [uint64]$_.physics_tick -eq [uint64]$event.physics_tick -and
        [int32]$_.dimension -eq [int32]$event.dimension
    })
    if ($correlated.Count -ne 1) { throw "$label does not correlate to exactly one production frame" }
    $events.Add($event)
}

for ($index = 1; $index -lt $frames.Count; $index++) {
    if ([int32]$frames[$index].dimension -ne [int32]$frames[$index - 1].dimension) {
        $matches = @($events | Where-Object {
            [string]$_.kind -ceq 'dimension' -and
            [uint64]$_.session_generation -eq [uint64]$frames[$index].session_generation -and
            [uint64]$_.fifo_sequence -eq [uint64]$frames[$index].fifo_sequence -and
            [uint64]$_.physics_tick -eq [uint64]$frames[$index].physics_tick -and
            [int32]$_.dimension -eq [int32]$frames[$index].dimension
        })
        if ($matches.Count -ne 1) { throw "frame[$index] dimension transition has no exact production event" }
    }
}

$logSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $LogPath).Hash.ToLowerInvariant()
. (Join-Path $PSScriptRoot 'Phase3Aggregate.ps1')
$aggregate = Write-Phase3FinalAggregate `
    -Identity $identity `
    -Frames @($frames) `
    -Events @($events) `
    -ScenarioManifest $scenarioManifest `
    -Terminal $terminal `
    -RunMetadataPath $RunMetadataPath `
    -MetricsPath $MetricsPath `
    -OutputPath $OutputPath `
    -LogSha256 $logSha256
$aggregateSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $OutputPath).Hash.ToLowerInvariant()
Write-Output "PHASE3_EVIDENCE_VALID target=$ExpectedTarget scenario=$($scenarioManifest.scenario) build=$ExpectedBuildCommit run=$ExpectedRunId frames=$($frames.Count) events=$($events.Count) log_sha256=$logSha256 aggregate_sha256=$aggregateSha256"
