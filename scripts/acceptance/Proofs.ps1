function ConvertFrom-FullViewSettleMarker {
    param(
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line,
        [Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind
    )

    $prefix = if ($Kind -ceq 'Teleport') {
        'RUST_MCBE_TELEPORT_SETTLED'
    }
    else {
        'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED'
    }
    if (-not $Line.StartsWith($prefix + ' ', [StringComparison]::Ordinal)) {
        throw "invalid $Kind settle marker prefix: $Line"
    }

    $values = [ordered]@{}
    foreach ($token in $Line.Substring($prefix.Length + 1).Split(
        [char[]]@(' '),
        [StringSplitOptions]::RemoveEmptyEntries
    )) {
        if ($token -notmatch '^(?<key>[a-z][a-z0-9_]*)=(?<value>\S+)$') {
            throw "invalid $Kind settle marker token: $token"
        }
        $key = $Matches['key']
        $text = $Matches['value']
        if ($values.Contains($key)) {
            throw "duplicate $Kind settle marker field: $key"
        }
        if ($text -ceq 'null') {
            $values[$key] = $null
            continue
        }
        if ($key -ceq 'ms' -or $key.EndsWith('_ms', [StringComparison]::Ordinal)) {
            $number = 0.0
            if (-not [double]::TryParse(
                $text,
                [Globalization.NumberStyles]::Float,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$number
            ) -or [double]::IsNaN($number) -or [double]::IsInfinity($number)) {
                throw "invalid $Kind settle marker number for ${key}: $text"
            }
            $values[$key] = $number
            continue
        }
        if ($key.EndsWith('_hash', [StringComparison]::Ordinal)) {
            if ($text -notmatch '^[0-9a-fA-F]{16}$') {
                throw "invalid $Kind settle marker hash for ${key}: $text"
            }
            $values[$key] = $text.ToLowerInvariant()
            continue
        }
        if ($text -match '^\d+$') {
            $number = [uint64]0
            if (-not [uint64]::TryParse(
                $text,
                [Globalization.NumberStyles]::None,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$number
            )) {
                throw "invalid $Kind settle marker integer for ${key}: $text"
            }
            $values[$key] = $number
            continue
        }
        $values[$key] = $text
    }
    foreach ($required in @('target', 'ms', 'transparent_sort_generation')) {
        if (-not $values.Contains($required) -or $null -eq $values[$required]) {
            throw "$Kind settle marker is missing $required"
        }
    }
    return [pscustomobject]$values
}

function ConvertFrom-TargetMutationArmedMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    if ($Line -notmatch '^RUST_MCBE_TARGET_MUTATION_ARMED source=(-?\d+),(-?\d+),(-?\d+) target=(-?\d+),(-?\d+),(-?\d+) view_generation=(\d+)$') {
        throw "invalid target mutation armed marker: $Line"
    }
    $generation = [uint64]0
    if (-not [uint64]::TryParse(
        $Matches[7],
        [Globalization.NumberStyles]::None,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$generation
    ) -or $generation -eq 0) {
        throw "invalid target mutation armed marker: $Line"
    }
    return [pscustomobject][ordered]@{
        source = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
        target = @([int]$Matches[4], [int]$Matches[5], [int]$Matches[6])
        view_generation = $generation
    }
}

function ConvertFrom-MovePlayerIngressMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    $number = '[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?'
    if ($Line -notmatch "^RUST_MCBE_MOVE_PLAYER_INGRESS sequence=(\d+) position=($number),($number),($number)$") {
        throw "invalid MovePlayer ingress marker: $Line"
    }
    $sequence = [uint64]0
    if (-not [uint64]::TryParse(
        $Matches[1],
        [Globalization.NumberStyles]::None,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$sequence
    ) -or $sequence -eq 0) {
        throw "invalid MovePlayer ingress marker: $Line"
    }
    $position = [double[]]::new(3)
    for ($index = 0; $index -lt 3; $index++) {
        $value = [double]0
        if (-not [double]::TryParse(
            $Matches[$index + 2],
            [Globalization.NumberStyles]::Float,
            [Globalization.CultureInfo]::InvariantCulture,
            [ref]$value
        ) -or [double]::IsNaN($value) -or [double]::IsInfinity($value)) {
            throw "invalid MovePlayer ingress marker: $Line"
        }
        $position[$index] = $value
    }
    return [pscustomobject][ordered]@{
        sequence = $sequence
        position = $position
    }
}

function Get-RequiredEvidenceProperty {
    param(
        [Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Label,
        [switch]$AllowNull
    )

    $property = $Evidence.PSObject.Properties[$Name]
    if ($null -eq $property -or (-not $AllowNull -and $null -eq $property.Value)) {
        throw "$Label.$Name was missing"
    }
    return $property.Value
}

function ConvertTo-EvidenceUInt64 {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Field
    )

    $text = [Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture)
    $number = [uint64]0
    if ($text -notmatch '^\d+$' -or -not [uint64]::TryParse(
        $text,
        [Globalization.NumberStyles]::None,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$number
    )) {
        throw "$Field was not an unsigned integer: $text"
    }
    return $number
}

function ConvertTo-EvidenceDouble {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Field
    )

    $number = 0.0
    $text = [Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture)
    if (-not [double]::TryParse(
        $text,
        [Globalization.NumberStyles]::Float,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$number
    ) -or [double]::IsNaN($number) -or [double]::IsInfinity($number)) {
        throw "$Field was not finite: $text"
    }
    return $number
}

function Get-FullViewProofFieldNames {
    param([Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind)

    $fields = @(
        'target', 'committed', 'ms', 'view_generation', 'transparent_sort_generation', 'render_ready_ms',
        'first_frame_sequence', 'stable_frame_sequence',
        'first_present_ms', 'first_gpu_ms', 'stable_present_ms', 'stable_gpu_ms', 'frame_count',
        'expected_manifest_count', 'expected_manifest_hash',
        'first_presented_manifest_count', 'first_presented_manifest_hash',
        'stable_presented_manifest_count', 'stable_presented_manifest_hash',
        'expected', 'loaded_target', 'missing_target', 'foreign_loaded', 'foreign_requested',
        'foreign_resident', 'source_leftover', 'resident_count', 'resident_hash',
        'known_air_count', 'known_air_hash', 'missing_target_instances',
        'unexpected_target_instances', 'source_instances', 'foreign_instances',
        'stale_generation_instances', 'orphan_allocations'
    )
    if ($Kind -ceq 'Teleport') {
        $fields += @(
            'publisher_ms', 'first_level_ms', 'last_level_ms', 'level_events',
            'first_sub_ms', 'last_sub_ms', 'sub_events'
        )
    }
    return $fields
}

function Assert-OptionalStageOffset {
    param(
        [Parameter(Mandatory = $true)]$Proof,
        [Parameter(Mandatory = $true)][string]$Field,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][double]$Maximum
    )

    $value = Get-RequiredEvidenceProperty -Evidence $Proof -Name $Field -Label $Label -AllowNull
    if ($null -eq $value) {
        return $null
    }
    try {
        $number = ConvertTo-EvidenceDouble -Value $value -Field "$Label.$Field"
    }
    catch {
        throw "$Label.$Field must be JSON null or a nonnegative finite value: $value"
    }
    if ($number -lt 0.0 -or $number -gt $Maximum) {
        throw "$Label.$Field must be JSON null or a nonnegative finite value at or before ${Maximum}ms: $value"
    }
    return $number
}

function Assert-ExactFullViewProof {
    param(
        [Parameter(Mandatory = $true)]$Proof,
        [Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$ExpectedTargetCohort
    )

    foreach ($field in Get-FullViewProofFieldNames -Kind $Kind) {
        $allowNull = $Kind -ceq 'Teleport' -and $field -in @(
            'publisher_ms', 'first_level_ms', 'last_level_ms', 'first_sub_ms', 'last_sub_ms'
        )
        $null = Get-RequiredEvidenceProperty `
            -Evidence $Proof `
            -Name $field `
            -Label $Label `
            -AllowNull:$allowNull
    }

    $target = [string]$Proof.target
    if ($target -notmatch '^-?\d+:-?\d+:-?\d+:16$') {
        throw "$Label.target was not a radius-16 cohort: $target"
    }
    if ($target -cne $ExpectedTargetCohort) {
        throw "$Label target cohort mismatch: expected=$ExpectedTargetCohort actual=$target"
    }
    if ([string]$Proof.committed -cne $target) {
        throw "$Label committed cohort did not equal target: committed=$($Proof.committed) target=$target"
    }

    $latency = ConvertTo-EvidenceDouble -Value $Proof.ms -Field "$Label.ms"
    if ($latency -le 0.0) {
        throw "$Label.ms must be positive: $latency"
    }
    $renderReady = ConvertTo-EvidenceDouble -Value $Proof.render_ready_ms -Field "$Label.render_ready_ms"
    $firstPresent = ConvertTo-EvidenceDouble -Value $Proof.first_present_ms -Field "$Label.first_present_ms"
    $firstGpu = ConvertTo-EvidenceDouble -Value $Proof.first_gpu_ms -Field "$Label.first_gpu_ms"
    $stablePresent = ConvertTo-EvidenceDouble -Value $Proof.stable_present_ms -Field "$Label.stable_present_ms"
    $stableGpu = ConvertTo-EvidenceDouble -Value $Proof.stable_gpu_ms -Field "$Label.stable_gpu_ms"
    if ($renderReady -lt 0.0 -or
        $firstPresent -lt $renderReady -or
        $firstGpu -lt $firstPresent -or
        $stablePresent -lt $renderReady -or
        $stablePresent -lt $firstPresent -or
        $stableGpu -lt $firstGpu -or
        $stableGpu -lt $stablePresent -or
        [Math]::Abs($stableGpu - $latency) -gt 0.001) {
        throw "$Label presentation timestamps were not monotonic through the binding GPU completion"
    }

    $firstSequence = ConvertTo-EvidenceUInt64 -Value $Proof.first_frame_sequence -Field "$Label.first_frame_sequence"
    $stableSequence = ConvertTo-EvidenceUInt64 -Value $Proof.stable_frame_sequence -Field "$Label.stable_frame_sequence"
    if ($firstSequence -eq [uint64]::MaxValue -or $stableSequence -ne $firstSequence + 1) {
        throw "$Label frame sequences were not adjacent: first=$firstSequence stable=$stableSequence"
    }

    $expectedColumns = ConvertTo-EvidenceUInt64 -Value $Proof.expected -Field "$Label.expected"
    $loadedColumns = ConvertTo-EvidenceUInt64 -Value $Proof.loaded_target -Field "$Label.loaded_target"
    if ($expectedColumns -ne 1089 -or $loadedColumns -ne $expectedColumns) {
        throw "$Label loaded/expected cohort counts were not exact: expected=$expectedColumns loaded=$loadedColumns"
    }
    foreach ($field in @(
        'missing_target', 'foreign_loaded', 'foreign_requested', 'foreign_resident', 'source_leftover'
    )) {
        $value = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
        if ($value -ne 0) {
            throw "$Label.$field=$value, expected zero"
        }
    }

    foreach ($field in @(
        'resident_count', 'known_air_count', 'view_generation', 'transparent_sort_generation', 'frame_count'
    )) {
        $null = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
    }
    if ([uint64]$Proof.resident_count + [uint64]$Proof.known_air_count -eq 0) {
        throw "$Label resident and known-air identities were both empty"
    }
    foreach ($field in @('resident_hash', 'known_air_hash')) {
        if ([string]$Proof.$field -notmatch '^[0-9a-fA-F]{16}$') {
            throw "$Label.$field was not a 16-digit deterministic hash: $($Proof.$field)"
        }
    }

    $expectedManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $Proof.expected_manifest_count `
        -Field "$Label.expected_manifest_count"
    $firstManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $Proof.first_presented_manifest_count `
        -Field "$Label.first_presented_manifest_count"
    $stableManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $Proof.stable_presented_manifest_count `
        -Field "$Label.stable_presented_manifest_count"
    if ($expectedManifestCount -eq 0 -or
        $firstManifestCount -ne $expectedManifestCount -or
        $stableManifestCount -ne $expectedManifestCount) {
        throw "$Label presented manifest count did not equal expected: expected=$expectedManifestCount first=$firstManifestCount stable=$stableManifestCount"
    }
    $expectedManifestHash = ([string]$Proof.expected_manifest_hash).ToLowerInvariant()
    $firstManifestHash = ([string]$Proof.first_presented_manifest_hash).ToLowerInvariant()
    $stableManifestHash = ([string]$Proof.stable_presented_manifest_hash).ToLowerInvariant()
    foreach ($entry in @(
        [pscustomobject]@{ Name = 'expected_manifest_hash'; Value = $expectedManifestHash },
        [pscustomobject]@{ Name = 'first_presented_manifest_hash'; Value = $firstManifestHash },
        [pscustomobject]@{ Name = 'stable_presented_manifest_hash'; Value = $stableManifestHash }
    )) {
        if ($entry.Value -notmatch '^[0-9a-f]{16}$') {
            throw "$Label.$($entry.Name) was not a 16-digit deterministic hash: $($entry.Value)"
        }
    }
    if ($firstManifestHash -cne $expectedManifestHash -or $stableManifestHash -cne $expectedManifestHash) {
        throw "$Label presented manifest hash did not equal expected: expected=$expectedManifestHash first=$firstManifestHash stable=$stableManifestHash"
    }

    foreach ($field in @(
        'missing_target_instances', 'unexpected_target_instances', 'source_instances',
        'foreign_instances', 'stale_generation_instances', 'orphan_allocations'
    )) {
        $value = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
        if ($value -ne 0) {
            throw "$Label.$field=$value, expected zero"
        }
    }

    $intervalFrameCount = ConvertTo-EvidenceUInt64 -Value $Proof.frame_count -Field "$Label.frame_count"
    if ($intervalFrameCount -lt 2) {
        throw "$Label.frame_count must cover at least two presented frames: $intervalFrameCount"
    }
    # One boundary frame is allowed because the measured interval begins and
    # ends on callbacks rather than between frame ticks.
    $maximumCappedFrames = [uint64][Math]::Ceiling($latency * 60.0 / 1000.0) + 1
    if ($intervalFrameCount -gt $maximumCappedFrames) {
        throw "$Label exceeded its 60fps cap: frames=$intervalFrameCount maximum=$maximumCappedFrames interval_ms=$latency"
    }

    if ($Kind -ceq 'Teleport') {
        $publisher = Assert-OptionalStageOffset -Proof $Proof -Field 'publisher_ms' -Label $Label -Maximum $renderReady
        $firstLevel = Assert-OptionalStageOffset -Proof $Proof -Field 'first_level_ms' -Label $Label -Maximum $renderReady
        $lastLevel = Assert-OptionalStageOffset -Proof $Proof -Field 'last_level_ms' -Label $Label -Maximum $renderReady
        $firstSub = Assert-OptionalStageOffset -Proof $Proof -Field 'first_sub_ms' -Label $Label -Maximum $renderReady
        $lastSub = Assert-OptionalStageOffset -Proof $Proof -Field 'last_sub_ms' -Label $Label -Maximum $renderReady
        $null = $publisher
        foreach ($pair in @(
            [pscustomobject]@{ Name = 'level'; First = $firstLevel; Last = $lastLevel },
            [pscustomobject]@{ Name = 'sub'; First = $firstSub; Last = $lastSub }
        )) {
            if (($null -eq $pair.First) -ne ($null -eq $pair.Last) -or
                ($null -ne $pair.First -and $pair.First -gt $pair.Last)) {
                throw "$Label $($pair.Name) stage offsets were not a monotonic pair"
            }
        }
        foreach ($field in @('level_events', 'sub_events')) {
            $null = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
        }
    }
}

function Assert-MarkerMatchesProof {
    param(
        [Parameter(Mandatory = $true)]$Marker,
        [Parameter(Mandatory = $true)]$Proof,
        [Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind,
        [Parameter(Mandatory = $true)][string]$Label
    )

    foreach ($field in Get-FullViewProofFieldNames -Kind $Kind) {
        $markerProperty = $Marker.PSObject.Properties[$field]
        if ($null -eq $markerProperty) {
            throw "$Label marker is missing $field"
        }
        $proofValue = $Proof.PSObject.Properties[$field].Value
        $markerValue = $markerProperty.Value
        if ($null -eq $proofValue -or $null -eq $markerValue) {
            if (-not ($null -eq $proofValue -and $null -eq $markerValue)) {
                throw "$Label marker/metrics mismatch for ${field}: marker=$markerValue metrics=$proofValue"
            }
            continue
        }
        if ($field -ceq 'ms' -or $field.EndsWith('_ms', [StringComparison]::Ordinal)) {
            $markerNumber = ConvertTo-EvidenceDouble -Value $markerValue -Field "$Label marker $field"
            $proofNumber = ConvertTo-EvidenceDouble -Value $proofValue -Field "$Label metrics $field"
            if ([Math]::Abs($markerNumber - $proofNumber) -gt 0.001) {
                throw "$Label marker/metrics mismatch for ${field}: marker=$markerNumber metrics=$proofNumber"
            }
            continue
        }
        if ([string]$markerValue -cne [string]$proofValue) {
            throw "$Label marker/metrics mismatch for ${field}: marker=$markerValue metrics=$proofValue"
        }
    }
}

function Assert-FullViewProofCohortContinuity {
    param(
        [Parameter(Mandatory = $true)]$TeleportProof,
        [Parameter(Mandatory = $true)]$ForcedRemeshProof
    )

    foreach ($field in @(
        'target', 'committed', 'expected', 'loaded_target', 'resident_count', 'resident_hash',
        'known_air_count', 'known_air_hash'
    )) {
        $teleportValue = [string]$TeleportProof.PSObject.Properties[$field].Value
        $remeshValue = [string]$ForcedRemeshProof.PSObject.Properties[$field].Value
        if ($field.EndsWith('_hash', [StringComparison]::Ordinal)) {
            $teleportValue = $teleportValue.ToLowerInvariant()
            $remeshValue = $remeshValue.ToLowerInvariant()
        }
        if ($teleportValue -cne $remeshValue) {
            throw "full-view proof cohort changed between teleport and forced remesh at ${field}: teleport=$teleportValue remesh=$remeshValue"
        }
    }
    $teleportStableFrame = ConvertTo-EvidenceUInt64 `
        -Value $TeleportProof.stable_frame_sequence `
        -Field 'teleport_proof.stable_frame_sequence'
    $remeshFirstFrame = ConvertTo-EvidenceUInt64 `
        -Value $ForcedRemeshProof.first_frame_sequence `
        -Field 'forced_full_view_remesh_proof.first_frame_sequence'
    if ($remeshFirstFrame -le $teleportStableFrame) {
        throw "forced remesh frames were not later than teleport frames: teleport_stable=$teleportStableFrame remesh_first=$remeshFirstFrame"
    }
    $teleportGeneration = ConvertTo-EvidenceUInt64 `
        -Value $TeleportProof.view_generation `
        -Field 'teleport_proof.view_generation'
    $remeshGeneration = ConvertTo-EvidenceUInt64 `
        -Value $ForcedRemeshProof.view_generation `
        -Field 'forced_full_view_remesh_proof.view_generation'
    if ($remeshGeneration -le $teleportGeneration) {
        throw "forced remesh view generation did not advance beyond teleport: teleport=$teleportGeneration remesh=$remeshGeneration"
    }
    $teleportManifestHash = ([string]$TeleportProof.expected_manifest_hash).ToLowerInvariant()
    $remeshManifestHash = ([string]$ForcedRemeshProof.expected_manifest_hash).ToLowerInvariant()
    $teleportManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $TeleportProof.expected_manifest_count `
        -Field 'teleport_proof.expected_manifest_count'
    $remeshManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $ForcedRemeshProof.expected_manifest_count `
        -Field 'forced_full_view_remesh_proof.expected_manifest_count'
    if ($remeshManifestCount -ne $teleportManifestCount) {
        throw "forced remesh expected manifest count changed from teleport: teleport=$teleportManifestCount remesh=$remeshManifestCount"
    }
    if ($remeshManifestHash -ceq $teleportManifestHash) {
        throw "forced remesh expected manifest hash did not change from teleport: $remeshManifestHash"
    }
}

function New-FullViewResourceTrigger {
    param(
        [Parameter(Mandatory = $true)]$TeleportMarker,
        [Parameter(Mandatory = $true)]$ForcedRemeshMarker
    )

    foreach ($entry in @(
        [pscustomobject]@{ Marker = $TeleportMarker; Label = 'teleport marker' },
        [pscustomobject]@{ Marker = $ForcedRemeshMarker; Label = 'forced-remesh marker' }
    )) {
        foreach ($field in @('target', 'view_generation', 'stable_frame_sequence')) {
            $null = Get-RequiredEvidenceProperty `
                -Evidence $entry.Marker `
                -Name $field `
                -Label $entry.Label
        }
    }
    if ([string]$TeleportMarker.target -cne [string]$ForcedRemeshMarker.target) {
        throw "steady-resource trigger targets differ: teleport=$($TeleportMarker.target) remesh=$($ForcedRemeshMarker.target)"
    }
    return [pscustomobject][ordered]@{
        kind = 'FullViewPresented'
        target = [string]$TeleportMarker.target
        teleport_view_generation = ConvertTo-EvidenceUInt64 `
            -Value $TeleportMarker.view_generation `
            -Field 'teleport marker view_generation'
        teleport_stable_frame_sequence = ConvertTo-EvidenceUInt64 `
            -Value $TeleportMarker.stable_frame_sequence `
            -Field 'teleport marker stable_frame_sequence'
        forced_remesh_view_generation = ConvertTo-EvidenceUInt64 `
            -Value $ForcedRemeshMarker.view_generation `
            -Field 'forced-remesh marker view_generation'
        forced_remesh_stable_frame_sequence = ConvertTo-EvidenceUInt64 `
            -Value $ForcedRemeshMarker.stable_frame_sequence `
            -Field 'forced-remesh marker stable_frame_sequence'
    }
}
