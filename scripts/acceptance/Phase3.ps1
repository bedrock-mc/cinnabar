[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateScript({ Test-Path -LiteralPath $_ -PathType Leaf })]
    [string]$EvidencePath,

    [Parameter(Mandatory = $true)]
    [ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg')]
    [string]$ExpectedTarget
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))

function Get-RequiredProperty {
    param(
        [Parameter(Mandatory = $true)]$Object,
        [Parameter(Mandatory = $true)][string]$Name
    )
    if ($null -eq $Object -or $Object.PSObject.Properties.Name -notcontains $Name) {
        throw "missing required Phase 3 evidence field: $Name"
    }
    return $Object.$Name
}

function Assert-Exact {
    param($Expected, $Actual, [string]$Name)
    if ($Expected -cne $Actual) {
        throw "Phase 3 evidence $Name mismatch: expected '$Expected', got '$Actual'"
    }
}

function Assert-UnsignedBounded {
    param($Value, [string]$Name, [long]$Maximum)
    if ($Value -isnot [byte] -and $Value -isnot [int16] -and $Value -isnot [int32] -and $Value -isnot [int64] -and $Value -isnot [uint16] -and $Value -isnot [uint32] -and $Value -isnot [uint64]) {
        throw "Phase 3 evidence $Name must be an integer"
    }
    $number = [decimal]$Value
    if ($number -lt 0 -or $number -gt $Maximum) {
        throw "Phase 3 evidence $Name is outside 0..$Maximum"
    }
}

function Assert-FiniteBounded {
    param($Value, [string]$Name, [double]$Minimum, [double]$Maximum)
    try {
        $number = [double]$Value
    }
    catch {
        throw "Phase 3 evidence $Name is not numeric"
    }
    if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or $number -lt $Minimum -or $number -gt $Maximum) {
        throw "Phase 3 evidence $Name is non-finite or outside $Minimum..$Maximum"
    }
}

function Assert-Sha256 {
    param([string]$Value, [string]$Name)
    if ($Value -cnotmatch '^[0-9a-f]{64}$') {
        throw "Phase 3 evidence $Name is not a lowercase SHA-256"
    }
}

$evidence = Get-Content -Raw -LiteralPath $EvidencePath | ConvertFrom-Json
$buildCommit = (& git -C $repoRoot rev-parse HEAD).Trim()
$pregSha = (Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'crates\assets\data\block-physics-v1001.sha256')).Trim()
$bregSha = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $repoRoot 'crates\assets\data\block-registry-v1001.bin')).Hash.ToLowerInvariant()

Assert-Exact 1 (Get-RequiredProperty $evidence 'schema_version') 'schema_version'
Assert-Exact $buildCommit ([string](Get-RequiredProperty $evidence 'build_commit')) 'build_commit'
Assert-Exact $ExpectedTarget ([string](Get-RequiredProperty $evidence 'target')) 'target'
$endpoint = [string](Get-RequiredProperty $evidence 'endpoint')
if ([string]::IsNullOrWhiteSpace($endpoint) -or $endpoint.Length -gt 255) {
    throw 'Phase 3 evidence endpoint is empty or oversized'
}
Assert-UnsignedBounded (Get-RequiredProperty $evidence 'session_generation') 'session_generation' ([long]::MaxValue)
if ([decimal]$evidence.session_generation -eq 0) {
    throw 'Phase 3 evidence session_generation must be nonzero'
}

$actualPregSha = [string](Get-RequiredProperty $evidence 'preg_sha256')
$actualBregSha = [string](Get-RequiredProperty $evidence 'breg_sha256')
Assert-Sha256 $actualPregSha 'preg_sha256'
Assert-Sha256 $actualBregSha 'breg_sha256'
Assert-Exact $pregSha $actualPregSha 'preg_sha256'
Assert-Exact $bregSha $actualBregSha 'breg_sha256'

$inputMode = [string](Get-RequiredProperty $evidence 'input_mode')
if ($inputMode -notin @('KeyboardMouse', 'GamePad', 'Touch')) {
    throw "Phase 3 evidence input_mode is unsupported: $inputMode"
}

$ticks = Get-RequiredProperty $evidence 'tick_range'
$firstTick = Get-RequiredProperty $ticks 'first'
$lastTick = Get-RequiredProperty $ticks 'last'
$tickCount = Get-RequiredProperty $ticks 'count'
Assert-UnsignedBounded $firstTick 'tick_range.first' ([long]::MaxValue)
Assert-UnsignedBounded $lastTick 'tick_range.last' ([long]::MaxValue)
Assert-UnsignedBounded $tickCount 'tick_range.count' 12000
$expectedTickCount = [decimal]$lastTick - [decimal]$firstTick + 1
if ($expectedTickCount -le 0 -or [decimal]$tickCount -ne $expectedTickCount) {
    throw 'Phase 3 evidence tick range is empty, retrograde, or contains gaps'
}

$corrections = Get-RequiredProperty $evidence 'corrections'
Assert-UnsignedBounded (Get-RequiredProperty $corrections 'replay_count') 'corrections.replay_count' 12000
Assert-UnsignedBounded (Get-RequiredProperty $corrections 'snap_count') 'corrections.snap_count' 12000
Assert-FiniteBounded (Get-RequiredProperty $corrections 'maximum_magnitude') 'corrections.maximum_magnitude' 0.0 1000000.0

$outbox = Get-RequiredProperty $evidence 'outbox'
Assert-UnsignedBounded (Get-RequiredProperty $outbox 'high_water') 'outbox.high_water' 32
Assert-Exact 0 (Get-RequiredProperty $outbox 'drops') 'outbox.drops'
Assert-Exact 0 (Get-RequiredProperty $evidence 'free_camera_packet_count') 'free_camera_packet_count'

$heldJump = Get-RequiredProperty $evidence 'held_jump'
Assert-UnsignedBounded (Get-RequiredProperty $heldJump 'landings') 'held_jump.landings' 12000
Assert-UnsignedBounded (Get-RequiredProperty $heldJump 'rejumps') 'held_jump.rejumps' 12000
Assert-Exact $true (Get-RequiredProperty $heldJump 'passed') 'held_jump.passed'
if ([decimal]$heldJump.rejumps -gt [decimal]$heldJump.landings) {
    throw 'Phase 3 evidence held_jump.rejumps exceeds landings'
}

$perspectives = @((Get-RequiredProperty $evidence 'perspective_cycle'))
$expectedPerspectives = @('FirstPerson', 'ThirdPersonBack', 'ThirdPersonFront', 'FirstPerson')
if ($perspectives.Count -ne $expectedPerspectives.Count) {
    throw 'Phase 3 evidence perspective_cycle has the wrong length'
}
for ($index = 0; $index -lt $expectedPerspectives.Count; $index++) {
    Assert-Exact $expectedPerspectives[$index] ([string]$perspectives[$index]) "perspective_cycle[$index]"
}

$camera = Get-RequiredProperty $evidence 'camera'
Assert-UnsignedBounded (Get-RequiredProperty $camera 'blocked_count') 'camera.blocked_count' 12000
Assert-UnsignedBounded (Get-RequiredProperty $camera 'fallback_count') 'camera.fallback_count' 12000

$avatar = Get-RequiredProperty $evidence 'local_avatar'
Assert-Exact 0 (Get-RequiredProperty $avatar 'first_person_visible') 'local_avatar.first_person_visible'
Assert-Exact 1 (Get-RequiredProperty $avatar 'third_person_back_visible') 'local_avatar.third_person_back_visible'
Assert-Exact 1 (Get-RequiredProperty $avatar 'third_person_front_visible') 'local_avatar.third_person_front_visible'

$frameRate = Get-RequiredProperty $evidence 'frame_rate'
$frameSamples = @((Get-RequiredProperty $frameRate 'samples'))
if ($frameSamples.Count -lt 1 -or $frameSamples.Count -gt 36000) {
    throw 'Phase 3 evidence frame_rate.samples is empty or unbounded'
}
for ($index = 0; $index -lt $frameSamples.Count; $index++) {
    Assert-FiniteBounded $frameSamples[$index] "frame_rate.samples[$index]" 0.001 1000.0
}

$process = Get-RequiredProperty $evidence 'process'
Assert-Exact 0 (Get-RequiredProperty $process 'exit_code') 'process.exit_code'
Assert-Exact $false (Get-RequiredProperty $process 'timed_out') 'process.timed_out'
Assert-UnsignedBounded (Get-RequiredProperty $process 'peak_private_bytes') 'process.peak_private_bytes' 17179869184

$resources = Get-RequiredProperty $evidence 'resources'
Assert-Exact $true (Get-RequiredProperty $resources 'bounded') 'resources.bounded'
Assert-Exact 0 (Get-RequiredProperty $resources 'queue_drops') 'resources.queue_drops'

$events = @((Get-RequiredProperty $evidence 'events'))
if ($events.Count -gt 256) {
    throw 'Phase 3 evidence events exceeds the 256-record bound'
}
foreach ($event in $events) {
    $kind = [string](Get-RequiredProperty $event 'kind')
    if ([string]::IsNullOrWhiteSpace($kind) -or $kind.Length -gt 64) {
        throw 'Phase 3 evidence event kind is empty or oversized'
    }
    Assert-UnsignedBounded (Get-RequiredProperty $event 'tick') 'events.tick' ([long]::MaxValue)
}

$evidenceSha = (Get-FileHash -Algorithm SHA256 -LiteralPath $EvidencePath).Hash.ToLowerInvariant()
Write-Output "PHASE3_EVIDENCE_VALID target=$ExpectedTarget ticks=$tickCount sha256=$evidenceSha"
