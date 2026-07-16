function New-ModelGalleryWitnessRequest {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][ValidateRange(1, [long]::MaxValue)][uint64]$Revision
    )

    $fixtureKind = [string]$Plan.Manifest.fixture_kind
    $expectedWitnessCount = switch -CaseSensitive ($fixtureKind) {
        'SlabStairGallery' { 43 }
        'VineGallery' { 16 }
        default { throw 'model witness request requires a recognized exact model gallery' }
    }
    if ([uint64]$Plan.Manifest.central_witness_count -ne $expectedWitnessCount) {
        throw "model witness request requires the exact $expectedWitnessCount-entry $fixtureKind gallery"
    }
    $mutation = $Plan.Manifest.mutation
    $witnesses = @($Plan.Manifest.witnesses)
    if ($witnesses.Count -ne $expectedWitnessCount) {
        throw "$fixtureKind model witness count changed: $($witnesses.Count)"
    }
    $byIdentity = [ordered]@{}
    foreach ($witness in $witnesses) {
        $offset = @($witness.center_offset)
        if ($offset.Count -ne 3) {
            throw "$fixtureKind model witness lost its central block offset"
        }
        $x = [int][Math]::Floor(([double]$mutation.x + [double]$offset[0]) / 16.0)
        $y = [int][Math]::Floor(([double]$mutation.y + [double]$offset[1]) / 16.0)
        $z = [int][Math]::Floor(([double]$mutation.z + [double]$offset[2]) / 16.0)
        $identity = "$x,$y,$z"
        if (-not $byIdentity.Contains($identity)) {
            $byIdentity[$identity] = [pscustomobject][ordered]@{ x = $x; y = $y; z = $z }
        }
    }
    $keys = @($byIdentity.Values | Sort-Object x, y, z)
    if ($keys.Count -eq 0 -or $keys.Count -gt 64) {
        throw "$fixtureKind model witness key count is outside 1..64: $($keys.Count)"
    }
    $hashInput = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-model-witness-v1'
        revision = $Revision
        dimension = 0
        sub_chunks = $keys
    }
    return [pscustomobject][ordered]@{
        schema = [string]$hashInput.schema
        revision = [uint64]$hashInput.revision
        dimension = [int]$hashInput.dimension
        request_sha256 = Get-CanonicalObjectHash -Value $hashInput
        sub_chunks = $keys
    }
}

function New-SlabStairGalleryModelWitnessRequest {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][ValidateRange(1, [long]::MaxValue)][uint64]$Revision
    )
    return New-ModelGalleryWitnessRequest -Plan $Plan -Revision $Revision
}

function Get-StrictMcbeas05ModelTables {
    param([Parameter(Mandatory = $true)][string]$Path)

    $assetLength = [int64](Get-Item -LiteralPath $Path -ErrorAction Stop).Length
    if ($assetLength -gt 16 * 1024 * 1024) {
        throw "MCBEAS05 blob exceeds the app 16 MiB ceiling: $assetLength"
    }
    if ($assetLength -lt 232) { throw "MCBEAS05 blob is shorter than header plus SHA-256: $assetLength" }
    $bytes = [IO.File]::ReadAllBytes($Path)
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    if ($utf8.GetString($bytes, 0, 8) -cne 'MCBEAS05') { throw 'MCBEAS05 model-table validator received the wrong magic' }
    if ([BitConverter]::ToUInt32($bytes, 8) -ne 5 -or [BitConverter]::ToUInt32($bytes, 12) -ne 16 -or
        [BitConverter]::ToUInt32($bytes, 16) -ne 5 -or [BitConverter]::ToUInt32($bytes, 52) -ne 8 -or
        [BitConverter]::ToUInt32($bytes, 56) -ne 256) {
        throw 'MCBEAS05 header constants are noncanonical'
    }
    if (@($bytes[64..95] | Where-Object { $_ -ne 0 }).Count -ne 0) { throw 'MCBEAS05 reserved header bytes are nonzero' }

    $payloadLength = $bytes.Length - 32
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try { $actualDigest = $sha256.ComputeHash($bytes, 0, $payloadLength) }
    finally { $sha256.Dispose() }
    for ($index = 0; $index -lt 32; $index++) {
        if ($actualDigest[$index] -ne $bytes[$payloadLength + $index]) {
            throw 'MCBEAS05 slab/stair integrity SHA-256 mismatch'
        }
    }

    $counts = @(foreach ($countOffset in @(20, 24, 28, 32, 36, 40, 44, 48, 60)) {
        [uint64][BitConverter]::ToUInt32($bytes, $countOffset)
    })
    $limits = @([uint64]65536, [uint64]65536, [uint64]65536, [uint64]65536, [uint64]2097152, [uint64]65536, [uint64]1048576, [uint64]2, [uint64]1024)
    $labels = @('visual', 'hash', 'material', 'template', 'quad', 'animation', 'frame', 'page', 'biome')
    for ($index = 0; $index -lt $counts.Count; $index++) {
        if ($counts[$index] -gt $limits[$index]) { throw "MCBEAS05 $($labels[$index]) count exceeds canonical ceiling: $($counts[$index])" }
    }
    if ($counts[2] -eq 0 -or $counts[7] -eq 0) { throw 'MCBEAS05 material and page counts must be nonzero' }

    $offsets = @(0..12 | ForEach-Object { [uint64][BitConverter]::ToUInt64($bytes, 96 + 8 * $_) })
    $fixedSizes = @(
        ($counts[0] * 40)
        ($counts[1] * 8)
        ($counts[2] * 12)
        ($counts[3] * 12)
        ($counts[4] * 48)
        ($counts[5] * 28)
        ($counts[6] * 4)
        ($counts[7] * 64)
    )
    $expectedOffset = [uint64]200
    for ($index = 0; $index -lt $fixedSizes.Count; $index++) {
        if ($offsets[$index] -ne $expectedOffset) { throw "MCBEAS05 section $index offset is noncanonical" }
        $expectedOffset += $fixedSizes[$index]
    }
    if ($offsets[8] -ne $expectedOffset -or $expectedOffset -gt [uint64]$payloadLength) { throw 'MCBEAS05 texture payload offset is noncanonical or out of bounds' }

    $textureCursor = $offsets[8]
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        for ($pageIndex = 0; $pageIndex -lt $counts[7]; $pageIndex++) {
            $descriptor = [int]($offsets[7] + 64 * $pageIndex)
            $layers = [uint64][BitConverter]::ToUInt32($bytes, $descriptor + 4)
            $pagePayloadOffset = [uint64][BitConverter]::ToUInt64($bytes, $descriptor + 16)
            $pagePayloadLength = [uint64][BitConverter]::ToUInt64($bytes, $descriptor + 24)
            $expectedPageLength = $layers * 1364
            if ([BitConverter]::ToUInt32($bytes, $descriptor) -ne $pageIndex -or $layers -eq 0 -or $layers -gt 2048 -or
                [BitConverter]::ToUInt32($bytes, $descriptor + 8) -ne 5 -or [BitConverter]::ToUInt32($bytes, $descriptor + 12) -ne 0 -or
                $pagePayloadOffset -ne $textureCursor -or $pagePayloadLength -ne $expectedPageLength -or
                $pagePayloadOffset -gt [uint64]$payloadLength -or
                $pagePayloadLength -gt [uint64]$payloadLength - $pagePayloadOffset) {
                throw "MCBEAS05 texture page $pageIndex descriptor is noncanonical"
            }
            $pageDigest = $sha256.ComputeHash($bytes, [int]$pagePayloadOffset, [int]$pagePayloadLength)
            for ($digestIndex = 0; $digestIndex -lt 32; $digestIndex++) {
                if ($pageDigest[$digestIndex] -ne $bytes[$descriptor + 32 + $digestIndex]) { throw "MCBEAS05 texture page $pageIndex SHA-256 mismatch" }
            }
            $textureCursor += $pagePayloadLength
        }
    }
    finally { $sha256.Dispose() }
    if ($offsets[9] -ne $textureCursor -or $offsets[10] -ne $offsets[9] + 8 * 256 * 256 * 3 -or
        $offsets[11] -ne $offsets[10] + $counts[8] * 36 -or $offsets[12] -lt $offsets[11] -or
        $offsets[12] - $offsets[11] -gt 256 * 1024 -or $offsets[12] -ne [uint64]$payloadLength) {
        throw 'MCBEAS05 variable section offsets or exact total length are noncanonical'
    }

    $templates = [Collections.Generic.List[object]]::new()
    # Exact values from model_template_flags_are_valid; gate-axis bits are only
    # canonical when paired with the compound-head bit.
    $canonicalTemplateFlags = [uint32[]]@(
        0, 1, 2, 4, 8, 16, 32, 64, 132, 260, 512
    )
    $expectedQuad = [uint64]0
    for ($templateIndex = 0; $templateIndex -lt $counts[3]; $templateIndex++) {
        $descriptor = [int]($offsets[3] + 12 * $templateIndex)
        $quadStart = [uint64][BitConverter]::ToUInt32($bytes, $descriptor)
        $quadCount = [uint64][BitConverter]::ToUInt32($bytes, $descriptor + 4)
        $flags = [uint32][BitConverter]::ToUInt32($bytes, $descriptor + 8)
        if ($quadStart -ne $expectedQuad -or $quadCount -gt 32 -or $flags -notin $canonicalTemplateFlags -or
            ($flags -in @([uint32]1, [uint32]512) -and $quadCount -ne 6)) {
            throw "MCBEAS05 model template $templateIndex span or flags are noncanonical"
        }
        $templates.Add([pscustomobject][ordered]@{ quad_start = $quadStart; quad_count = $quadCount; flags = $flags })
        $expectedQuad += $quadCount
    }
    if ($expectedQuad -ne $counts[4]) { throw 'MCBEAS05 model templates do not exactly cover the quad table' }

    $stairBases = [Collections.Generic.HashSet[uint32]]::new()
    $templateIndex = 0
    while ($templateIndex -lt $templates.Count) {
        if (($templates[$templateIndex].flags -band 2) -eq 0) { $templateIndex++; continue }
        if ($templateIndex + 5 -gt $templates.Count) { throw 'MCBEAS05 stair template group is truncated' }
        foreach ($shape in 0..4) {
            $shapeTemplate = $templates[$templateIndex + $shape]
            if ($shapeTemplate.flags -ne 2 -or $shapeTemplate.quad_count -eq 0) { throw 'MCBEAS05 stair template group is noncanonical' }
        }
        $null = $stairBases.Add([uint32]$templateIndex)
        $templateIndex += 5
    }

    for ($kelpIndex = 0; $kelpIndex -lt $templates.Count; $kelpIndex++) {
        $kelpTemplate = $templates[$kelpIndex]
        if (($kelpTemplate.flags -band 1) -eq 0) { continue }
        foreach ($shapeIndex in 0..5) {
            $kelpQuad = [int]($offsets[4] + 48 * ($kelpTemplate.quad_start + $shapeIndex))
            $kelpQuadFlags = [uint32][BitConverter]::ToUInt32($bytes, $kelpQuad + 44)
            $twoSided = ($kelpQuadFlags -band 8) -ne 0
            if (($shapeIndex -lt 4 -and $twoSided) -or ($shapeIndex -ge 4 -and -not $twoSided)) {
                throw "MCBEAS05 kelp template $kelpIndex has noncanonical sidedness"
            }
        }
    }

    for ($quadIndex = 0; $quadIndex -lt $counts[4]; $quadIndex++) {
        $quad = [int]($offsets[4] + 48 * $quadIndex)
        $material = [uint64][BitConverter]::ToUInt32($bytes, $quad + 40)
        $flags = [uint32][BitConverter]::ToUInt32($bytes, $quad + 44)
        if ($material -ge $counts[2] -or ($flags -band (-bnot 127)) -ne 0 -or ($flags -band 7) -gt 6 -or (($flags -shr 4) -band 7) -gt 6) {
            throw "MCBEAS05 model quad $quadIndex has an invalid material or flags"
        }
    }

    $referencedStairBases = [Collections.Generic.HashSet[uint32]]::new()
    for ($visualIndex = 0; $visualIndex -lt $counts[0]; $visualIndex++) {
        $visual = [int]($offsets[0] + 40 * $visualIndex)
        $kind = $bytes[$visual + 25]
        $template = [uint32][BitConverter]::ToUInt32($bytes, $visual + 28)
        $variant = [uint32][BitConverter]::ToUInt32($bytes, $visual + 36)
        if ($kind -gt 5) { throw "MCBEAS05 visual $visualIndex has unknown kind" }
        if ($template -eq [uint32]::MaxValue) { continue }
        if ([uint64]$template -ge $counts[3]) { throw "MCBEAS05 visual $visualIndex references an invalid template" }
        if (($templates[[int]$template].flags -band 2) -ne 0) {
            if (-not $stairBases.Contains($template) -or $kind -ne 3 -or ($variant -band (-bnot 7)) -ne 0) {
                throw "MCBEAS05 stair visual $visualIndex does not reference an exact group base or has reserved variant bits"
            }
            $null = $referencedStairBases.Add($template)
        }
    }
    foreach ($base in $stairBases) {
        if (-not $referencedStairBases.Contains($base)) { throw "MCBEAS05 stair template group $base is unreferenced" }
    }
    return [pscustomobject][ordered]@{
        bytes = $bytes; counts = $counts; offsets = $offsets; templates = @($templates); stair_bases = $stairBases
    }
}

function Get-SlabStairCoverageEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )
    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $reader = [IO.BinaryReader]::new([IO.MemoryStream]::new($registryBytes, $false))
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try {
        if ($utf8.GetString($reader.ReadBytes(8)) -cne 'BREG1003' -or $reader.ReadUInt32() -ne 1001) {
            throw 'slab/stair coverage requires the protocol-1001 BREG1003 registry'
        }
        $null = $reader.ReadUInt32()
        $recordCount = [int]$reader.ReadUInt32()
        foreach ($ignored in 1..4) { $null = $reader.ReadUInt32() }
        if ($recordCount -ne 16913) { throw "slab/stair registry record count changed: $recordCount" }
        $entries = [Collections.Generic.List[object]]::new()
        for ($recordIndex = 0; $recordIndex -lt $recordCount; $recordIndex++) {
            $sequentialId = $reader.ReadUInt32(); $null = $reader.ReadUInt32(); $null = $reader.ReadByte()
            $family = $reader.ReadByte(); $null = $reader.ReadByte(); $modelMask = $reader.ReadByte()
            foreach ($ignored in 1..3) { $null = $reader.ReadByte() }
            $boxCount = [int]$reader.ReadByte(); $null = $reader.ReadUInt16()
            $nameLength = [int]$reader.ReadUInt16(); $stateLength = [int]$reader.ReadUInt32()
            $values = @(for ($valueIndex = 0; $valueIndex -lt 8; $valueIndex++) { $reader.ReadUInt32() })
            $null = $reader.ReadBytes(24 * $boxCount)
            $name = $utf8.GetString($reader.ReadBytes($nameLength))
            $canonicalState = $utf8.GetString($reader.ReadBytes($stateLength))
            if ($family -in @(7, 8)) {
                $entries.Add([pscustomobject][ordered]@{
                    sequential_id = $sequentialId; family = if ($family -eq 7) { 'Slab' } else { 'Stair' }
                    name = $name; canonical_state = $canonicalState; model_mask = $modelMask
                    orientation = if (($modelMask -band 1) -ne 0) { [int]$values[0] } else { $null }
                    half = if (($modelMask -band 2) -ne 0) { [int]$values[1] } else { $null }
                })
            }
        }
        if ($reader.BaseStream.Position -ne $reader.BaseStream.Length) { throw 'BREG1003 registry has trailing bytes' }
    }
    finally { $reader.Dispose() }

    $slabs = @($entries | Where-Object family -CEQ 'Slab')
    $stairs = @($entries | Where-Object family -CEQ 'Stair')
    $stairNames = @($stairs | ForEach-Object name | Sort-Object -Unique)
    if ($slabs.Count -ne 272 -or $stairs.Count -ne 512 -or $stairNames.Count -ne 64) {
        throw "slab/stair registry coverage changed: slabs=$($slabs.Count) stairs=$($stairs.Count) stair_names=$($stairNames.Count)"
    }
    $slabHalves = @(0..2 | ForEach-Object { $half = $_; @($slabs | Where-Object half -eq $half).Count })
    if (($slabHalves -join ',') -cne '68,68,136') { throw "slab half selector counts changed: $($slabHalves -join ',')" }
    foreach ($name in $stairNames) {
        $selectors = @($stairs | Where-Object name -CEQ $name | ForEach-Object { "$($_.orientation),$($_.half)" } | Sort-Object -Unique)
        $expected = @(0..3 | ForEach-Object { $orientation = $_; 0..1 | ForEach-Object { "$orientation,$_" } })
        if ($selectors.Count -ne 8 -or ($selectors -join ';') -cne (($expected | Sort-Object) -join ';')) {
            throw "stair selector matrix changed for ${name}: $($selectors -join ';')"
        }
    }

    $modelTables = Get-StrictMcbeas05ModelTables -Path $AssetsPath
    $assetBytes = $modelTables.bytes
    $visualCount = $modelTables.counts[0]; $templateCount = $modelTables.counts[3]
    $visualOffset = $modelTables.offsets[0]
    $diagnostic = 0
    foreach ($entry in $entries) {
        if ([uint64]$entry.sequential_id -ge [uint64]$visualCount) { throw "registry sequential ID $($entry.sequential_id) is absent from MCBEAS05" }
        $visual = [int]($visualOffset + 40 * [uint64]$entry.sequential_id)
        $template = [BitConverter]::ToUInt32($assetBytes, $visual + 28)
        if ($assetBytes[$visual + 25] -ne 3 -or $template -eq [uint32]::MaxValue -or $template -ge $templateCount) { $diagnostic++; continue }
        $descriptor = $modelTables.templates[[int]$template]
        if ($descriptor.quad_count -eq 0) { $diagnostic++; continue }
        if ($entry.family -ceq 'Stair') {
            if (-not $modelTables.stair_bases.Contains([uint32]$template) -or ([BitConverter]::ToUInt32($assetBytes, $visual + 36) -band (-bnot 7)) -ne 0) { $diagnostic++; continue }
        }
        elseif ($descriptor.flags -ne 0) {
            $diagnostic++; continue
        }
    }
    if ($diagnostic -ne 0) { throw "slab/stair compiled coverage contains diagnostic or malformed visuals: $diagnostic" }
    $stateSetHash = Get-CanonicalObjectHash -Value @($entries | ForEach-Object { [pscustomobject][ordered]@{
        sequential_id = $_.sequential_id; family = $_.family; name = $_.name; canonical_state = $_.canonical_state
    } })
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-slab-stair-coverage-v1'; registry_protocol = 1001; compiler_schema = 'MCBEAS05'
        registry_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $RegistryPath).Hash.ToLowerInvariant()
        assets_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $AssetsPath).Hash.ToLowerInvariant()
        state_set_sha256 = $stateSetHash; state_count = $entries.Count; slab_state_count = $slabs.Count
        stair_state_count = $stairs.Count; stair_name_count = $stairNames.Count; diagnostic_slab_stair = $diagnostic
        entries = @($entries)
    }
}

function New-SlabStairGalleryPlan {
    param(
        [Parameter(Mandatory = $true)][ValidateCount(3, 3)][int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite')]
        [string]$Pose,
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $coverage = Get-SlabStairCoverageEvidence -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 23; y = $my + 1; z = $mz - 15 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 23; y = $my + 7; z = $mz + 15 }
    $clearVolume = 47 * 7 * 31
    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 2; z = $mz }
    $relativeCameras = [ordered]@{
        SlabStairGalleryTop = @(0, 38, 0)
        SlabStairGalleryNorth = @(0, 13, -48)
        SlabStairGalleryEast = @(48, 13, 0)
        SlabStairGalleryOblique = @(-42, 30, -42)
        SlabStairGalleryObliqueOpposite = @(42, 30, 42)
    }
    $cameraPoses = [ordered]@{}
    foreach ($cameraName in $relativeCameras.Keys) {
        $offset = $relativeCameras[$cameraName]
        $position = [pscustomobject][ordered]@{ x = $mx + $offset[0]; y = $my + $offset[1]; z = $mz + $offset[2] }
        $cameraPoses[$cameraName] = [pscustomobject][ordered]@{
            position = $position
            target = $galleryCenter
            command = "tp @a[name=RustMCBE] $($position.x) $($position.y) $($position.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
        }
    }

    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $fixtureCommands.Add("fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air")
    $fixtureCommands.Add("fill $($clearMin.x) $($my + 1) $($clearMin.z) $($clearMax.x) $($my + 1) $($clearMax.z) minecraft:stone")
    $witnesses = [Collections.Generic.List[object]]::new()
    foreach ($slab in @(
        [pscustomobject][ordered]@{ label = 'bottom_slab'; block = 'minecraft:smooth_stone_slab ["minecraft:vertical_half"="bottom"]'; offset = @(-4, 0, -13) },
        [pscustomobject][ordered]@{ label = 'top_slab'; block = 'minecraft:smooth_stone_slab ["minecraft:vertical_half"="top"]'; offset = @(0, 0, -13) },
        [pscustomobject][ordered]@{ label = 'double_slab'; block = 'minecraft:smooth_stone_double_slab'; offset = @(4, 0, -13) }
    )) {
        $x = $mx + $slab.offset[0]; $y = $my + 2; $z = $mz + $slab.offset[2]
        $fixtureCommands.Add("setblock $x $y $z $($slab.block)")
        $witnesses.Add([pscustomobject][ordered]@{ kind = 'slab'; shape = $slab.label; orientation = $null; orientation_value = $null; upside_down = $null; center_offset = @($slab.offset[0], 2, $slab.offset[2]); neighbor_offset = $null })
    }

    $orientationNames = @('south', 'west', 'north', 'east')
    $directionOffsets = @(@(0, 1), @(-1, 0), @(0, -1), @(1, 0))
    $shapeNames = @('straight', 'right_inner', 'left_inner', 'right_outer', 'left_outer')
    for ($half = 0; $half -lt 2; $half++) {
        for ($orientation = 0; $orientation -lt 4; $orientation++) {
            for ($shape = 0; $shape -lt 5; $shape++) {
                $index = $half * 20 + $orientation * 5 + $shape
                $cx = $mx - 18 + 5 * ($index % 8)
                $cy = $my + 2
                $cz = $mz - 9 + 5 * [Math]::Floor($index / 8)
                $upside = if ($half -eq 0) { 'false' } else { 'true' }
                $state = "[`"weirdo_direction`"=$orientation,`"upside_down_bit`"=$upside]"
                $fixtureCommands.Add("setblock $cx $cy $cz minecraft:oak_stairs $state")
                $neighborOffset = $null
                if ($shape -ne 0) {
                    $right = ($orientation + 1) % 4
                    $left = ($orientation + 3) % 4
                    $opposite = ($orientation + 2) % 4
                    $neighborDirection = if ($shape -le 2) { $opposite } else { $orientation }
                    $neighborFacing = switch ($shape) { 1 { $right } 2 { $left } 3 { $left } 4 { $right } }
                    $delta = $directionOffsets[$neighborDirection]
                    $nx = $cx + $delta[0]; $nz = $cz + $delta[1]
                    $neighborState = "[`"weirdo_direction`"=$neighborFacing,`"upside_down_bit`"=$upside]"
                    $fixtureCommands.Add("setblock $nx $cy $nz minecraft:oak_stairs $neighborState")
                    $neighborOffset = @(($nx - $mx), 2, ($nz - $mz))
                }
                $witnesses.Add([pscustomobject][ordered]@{
                    kind = 'stair'; shape = $shapeNames[$shape]; orientation = $orientationNames[$orientation]
                    orientation_value = $orientation; upside_down = [bool]$half
                    center_offset = @(($cx - $mx), 2, ($cz - $mz)); neighbor_offset = $neighborOffset
                })
            }
        }
    }
    if ($witnesses.Count -ne 43 -or $fixtureCommands.Count -ne 77) {
        throw "slab/stair gallery layout changed: witnesses=$($witnesses.Count) commands=$($fixtureCommands.Count)"
    }
    $stateSetHash = $coverage.state_set_sha256
    $relativeLayout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-slab-stair-layout-v1'; witness_count = 43; state_set_sha256 = $stateSetHash
        clear_min = @(-23, 1, -15); clear_max = @(23, 7, 15); support_y = 1; support_block = 'minecraft:stone'
        witnesses = @($witnesses); camera_offsets = [pscustomobject]$relativeCameras
    }
    $layoutHash = Get-CanonicalObjectHash -Value $relativeLayout
    $fenceCommand = 'list'; $fenceMarker = 'players online:'
    $loadAreaName = 'rust_mcbe_slab_stair_gallery'
    $loadAreaCommand = "tickingarea add $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) $loadAreaName true"
    $cleanupCommand = "tickingarea remove $loadAreaName"
    $teleportCommand = [string]$cameraPoses[$Pose].command
    $cameraTarget = $cameraPoses[$Pose].position
    $commands = @($loadAreaCommand) + @($fixtureCommands) + @($fenceCommand, $teleportCommand)
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v2'; fixture_kind = 'SlabStairGallery'; pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        fixture_layout_hash = $layoutHash; state_set_sha256 = $stateSetHash; relative_layout = $relativeLayout
        clear = [pscustomobject][ordered]@{ min = $clearMin; max = $clearMax; volume = $clearVolume }
        gallery_center = $galleryCenter; camera = $cameraPoses[$Pose]; camera_poses = [pscustomobject]$cameraPoses
        central_witness_count = 43; witnesses = @($witnesses)
        coverage_evidence = [pscustomobject][ordered]@{
            schema = $coverage.schema; registry_protocol = $coverage.registry_protocol; compiler_schema = $coverage.compiler_schema
            registry_sha256 = $coverage.registry_sha256; assets_sha256 = $coverage.assets_sha256; state_set_sha256 = $coverage.state_set_sha256
            state_count = $coverage.state_count; slab_state_count = $coverage.slab_state_count; stair_state_count = $coverage.stair_state_count; stair_name_count = $coverage.stair_name_count
            diagnostic_slab_stair = $coverage.diagnostic_slab_stair
        }
        load_area = [pscustomobject][ordered]@{ name = $loadAreaName; command = $loadAreaCommand; acknowledgement_marker = 'marked for preload.'; cleanup_command = $cleanupCommand; cleanup_acknowledgement_marker = 'Removed ticking area(s)'; settle_milliseconds = 3000 }
        processing_fence = [pscustomobject][ordered]@{ command = $fenceCommand; stdout_marker = $fenceMarker }
        fixture_commands = @($fixtureCommands); commands = $commands; command_count = $commands.Count
        teleport_command = $teleportCommand; settle_milliseconds = 3000
    }
    return [pscustomobject][ordered]@{
        Pose = $Pose; LoadAreaName = $loadAreaName; LoadAreaCommand = $loadAreaCommand; LoadAreaMarker = 'marked for preload.'; LoadAreaSettleMilliseconds = 3000
        CleanupCommand = $cleanupCommand; CleanupMarker = 'Removed ticking area(s)'; FixtureCommands = @($fixtureCommands); GalleryCommands = @($fixtureCommands)
        FenceMarker = $fenceMarker; FenceCommand = $fenceCommand; TeleportCommand = $teleportCommand; CameraTarget = $cameraTarget; Commands = $commands; Manifest = $manifest
    }
}
