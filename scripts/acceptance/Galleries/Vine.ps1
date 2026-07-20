function Get-VineCoverageEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $reader = [IO.BinaryReader]::new([IO.MemoryStream]::new($registryBytes, $false))
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try {
        if ($utf8.GetString($reader.ReadBytes(8)) -cne 'BREG1003' -or $reader.ReadUInt32() -ne 1001) {
            throw 'vine coverage requires the protocol-1001 BREG1003 registry'
        }
        $null = $reader.ReadUInt32()
        $recordCount = [int]$reader.ReadUInt32()
        foreach ($ignored in 1..4) { $null = $reader.ReadUInt32() }
        if ($recordCount -ne 16913) { throw "vine registry record count changed: $recordCount" }
        $entries = [Collections.Generic.List[object]]::new()
        for ($recordIndex = 0; $recordIndex -lt $recordCount; $recordIndex++) {
            $sequentialId = $reader.ReadUInt32(); $null = $reader.ReadUInt32(); $null = $reader.ReadByte()
            $family = $reader.ReadByte(); $null = $reader.ReadByte(); $modelMask = $reader.ReadByte()
            foreach ($ignored in 1..3) { $null = $reader.ReadByte() }
            $boxCount = [int]$reader.ReadByte(); $null = $reader.ReadUInt16()
            $nameLength = [int]$reader.ReadUInt16(); $stateLength = [int]$reader.ReadUInt32()
            $null = $reader.ReadBytes(32 + 24 * $boxCount)
            $name = $utf8.GetString($reader.ReadBytes($nameLength))
            $canonicalState = $utf8.GetString($reader.ReadBytes($stateLength))
            if ($name -ceq 'minecraft:vine') {
                $state = $canonicalState | ConvertFrom-Json
                $stateProperties = @($state.PSObject.Properties.Name)
                $vineProperty = $state.PSObject.Properties['vine_direction_bits']
                if ($stateProperties.Count -ne 1 -or $null -eq $vineProperty) {
                    throw "vine registry state is noncanonical: $canonicalState"
                }
                $value = $vineProperty.Value
                if (@($value.PSObject.Properties.Name).Count -ne 2 -or
                    [string]$value.type -cne 'int' -or $null -eq $value.PSObject.Properties['value']) {
                    throw "vine registry selector is noncanonical: $canonicalState"
                }
                $entries.Add([pscustomobject][ordered]@{
                    sequential_id = $sequentialId; family = $family; model_mask = $modelMask
                    name = $name; canonical_state = $canonicalState; mask = [int]$value.value
                })
            }
        }
        if ($reader.BaseStream.Position -ne $reader.BaseStream.Length) { throw 'BREG1003 registry has trailing bytes' }
    }
    finally { $reader.Dispose() }

    $orderedEntries = @($entries | Sort-Object mask)
    if ($orderedEntries.Count -ne 16) { throw "vine registry coverage changed: $($orderedEntries.Count)" }
    for ($mask = 0; $mask -lt 16; $mask++) {
        $entry = $orderedEntries[$mask]
        $expectedState = "{`"vine_direction_bits`":{`"type`":`"int`",`"value`":$mask}}"
        if ([int]$entry.mask -ne $mask -or [byte]$entry.family -ne 32 -or [byte]$entry.model_mask -ne 16 -or
            [string]$entry.name -cne 'minecraft:vine' -or [string]$entry.canonical_state -cne $expectedState) {
            throw "vine registry masks are not the exact protocol-1001 bijection 0..15 at mask $mask"
        }
    }

    $modelTables = Get-StrictMcbeas05ModelTables -Path $AssetsPath
    $assetBytes = $modelTables.bytes
    $visualCount = $modelTables.counts[0]; $templateCount = $modelTables.counts[3]
    $visualOffset = $modelTables.offsets[0]
    $diagnostic = 0
    foreach ($entry in $orderedEntries) {
        if ([uint64]$entry.sequential_id -ge [uint64]$visualCount) {
            throw "registry sequential ID $($entry.sequential_id) is absent from MCBEAS05"
        }
        $visual = [int]($visualOffset + 40 * [uint64]$entry.sequential_id)
        $template = [uint32][BitConverter]::ToUInt32($assetBytes, $visual + 28)
        if ($assetBytes[$visual + 25] -ne 3 -or $template -eq [uint32]::MaxValue -or
            [uint64]$template -ge [uint64]$templateCount -or [BitConverter]::ToUInt32($assetBytes, $visual + 36) -ne 0) {
            $diagnostic++; continue
        }
        $descriptor = $modelTables.templates[[int]$template]
        $expectedQuads = 0
        foreach ($bit in @(1, 2, 4, 8)) {
            if (([int]$entry.mask -band $bit) -ne 0) { $expectedQuads++ }
        }
        if ([uint32]$descriptor.flags -ne 0 -or [uint64]$descriptor.quad_count -ne [uint64]$expectedQuads) {
            $diagnostic++
        }
    }
    if ($diagnostic -ne 0) { throw "vine compiled coverage contains diagnostic or malformed visuals: $diagnostic" }
    $stateSetHash = Get-CanonicalObjectHash -Value @($orderedEntries | ForEach-Object { [pscustomobject][ordered]@{
        sequential_id = $_.sequential_id; name = $_.name; canonical_state = $_.canonical_state; mask = $_.mask
    } })
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-vine-coverage-v1'; registry_protocol = 1001; compiler_schema = 'MCBEAS05'
        registry_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $RegistryPath).Hash.ToLowerInvariant()
        assets_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $AssetsPath).Hash.ToLowerInvariant()
        state_set_sha256 = $stateSetHash; state_count = $orderedEntries.Count; diagnostic_vine = $diagnostic
        entries = $orderedEntries
    }
}

function New-VineGalleryPlan {
    param(
        [Parameter(Mandatory = $true)][ValidateCount(3, 3)][int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')]
        [string]$Pose,
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $coverage = Get-VineCoverageEvidence -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    $mx = [int]$MutationCoordinate[0]; $my = [int]$MutationCoordinate[1]; $mz = [int]$MutationCoordinate[2]
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 10; y = $my + 1; z = $mz - 10 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 9; y = $my + 5; z = $mz + 9 }
    $clearVolume = 20 * 5 * 20
    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 2; z = $mz }
    $relativeCameras = [ordered]@{
        VineGalleryTop = @(0, 32, 0)
        VineGalleryNorth = @(0, 10, -40)
        VineGalleryEast = @(40, 10, 0)
        VineGalleryOblique = @(-36, 26, -36)
        VineGalleryObliqueOpposite = @(36, 26, 36)
    }
    $cameraPoses = [ordered]@{}
    foreach ($cameraName in $relativeCameras.Keys) {
        $offset = $relativeCameras[$cameraName]
        $position = [pscustomobject][ordered]@{ x = $mx + $offset[0]; y = $my + $offset[1]; z = $mz + $offset[2] }
        $cameraPoses[$cameraName] = [pscustomobject][ordered]@{
            position = $position; target = $galleryCenter
            command = "tp @a[name=RustMCBE] $($position.x) $($position.y) $($position.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
        }
    }

    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $fixtureCommands.Add("fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air")
    $fixtureCommands.Add("fill $($clearMin.x) $($my + 1) $($clearMin.z) $($clearMax.x) $($my + 1) $($clearMax.z) minecraft:stone")
    $witnesses = [Collections.Generic.List[object]]::new()
    $supportDefinitions = @(
        [pscustomobject][ordered]@{ bit = 1; direction = 'south'; delta = @(0, 1) }
        [pscustomobject][ordered]@{ bit = 2; direction = 'west'; delta = @(-1, 0) }
        [pscustomobject][ordered]@{ bit = 4; direction = 'north'; delta = @(0, -1) }
        [pscustomobject][ordered]@{ bit = 8; direction = 'east'; delta = @(1, 0) }
    )
    foreach ($mask in 0..15) {
        $cx = $mx - 8 + 5 * ($mask % 4); $cy = $my + 2; $cz = $mz - 8 + 5 * [Math]::Floor($mask / 4)
        $supports = [Collections.Generic.List[object]]::new()
        foreach ($definition in $supportDefinitions) {
            if (($mask -band $definition.bit) -eq 0) { continue }
            $sx = $cx + $definition.delta[0]; $sz = $cz + $definition.delta[1]
            $fixtureCommands.Add("setblock $sx $cy $sz minecraft:stone")
            $supports.Add([pscustomobject][ordered]@{
                bit = $definition.bit; direction = $definition.direction
                offset = @(($sx - $mx), 2, ($sz - $mz))
            })
        }
        $fixtureCommands.Add("setblock $cx $cy $cz minecraft:vine [`"vine_direction_bits`"=$mask]")
        $witnesses.Add([pscustomobject][ordered]@{
            kind = 'vine'; mask = $mask; center_offset = @(($cx - $mx), 2, ($cz - $mz)); supports = @($supports)
        })
    }
    if ($witnesses.Count -ne 16 -or $fixtureCommands.Count -ne 50) {
        throw "vine gallery layout changed: witnesses=$($witnesses.Count) commands=$($fixtureCommands.Count)"
    }
    $relativeLayout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-vine-layout-v1'; witness_count = 16; state_set_sha256 = $coverage.state_set_sha256
        clear_min = @(-10, 1, -10); clear_max = @(9, 5, 9); floor_y = 1; witness_y = 2
        floor_block = 'minecraft:stone'; support_block = 'minecraft:stone'
        bit_order = @('south', 'west', 'north', 'east'); cell_spacing = @(5, 5)
        witnesses = @($witnesses); camera_offsets = [pscustomobject]$relativeCameras
    }
    $layoutHash = Get-CanonicalObjectHash -Value $relativeLayout
    $fenceCommand = 'list'; $fenceMarker = 'players online:'
    $loadAreaName = 'rust_mcbe_vine_gallery'
    $loadAreaCommand = "tickingarea add $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) $loadAreaName true"
    $cleanupCommand = "tickingarea remove $loadAreaName"
    $teleportCommand = [string]$cameraPoses[$Pose].command
    $cameraTarget = $cameraPoses[$Pose].position
    $commands = @($loadAreaCommand) + @($fixtureCommands) + @($fenceCommand, $teleportCommand)
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v2'; fixture_kind = 'VineGallery'; pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        fixture_layout_hash = $layoutHash; state_set_sha256 = $coverage.state_set_sha256; relative_layout = $relativeLayout
        clear = [pscustomobject][ordered]@{ min = $clearMin; max = $clearMax; volume = $clearVolume }
        gallery_center = $galleryCenter; camera = $cameraPoses[$Pose]; camera_poses = [pscustomobject]$cameraPoses
        central_witness_count = 16; witnesses = @($witnesses)
        coverage_evidence = [pscustomobject][ordered]@{
            schema = $coverage.schema; registry_protocol = $coverage.registry_protocol; compiler_schema = $coverage.compiler_schema
            registry_sha256 = $coverage.registry_sha256; assets_sha256 = $coverage.assets_sha256
            state_set_sha256 = $coverage.state_set_sha256; state_count = $coverage.state_count; diagnostic_vine = $coverage.diagnostic_vine
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
