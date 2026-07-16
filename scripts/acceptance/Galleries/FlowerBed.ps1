function Get-FlowerBedCoverageEvidence {
    param([Parameter(Mandatory = $true)][string]$RegistryPath)

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $reader = [IO.BinaryReader]::new([IO.MemoryStream]::new($registryBytes, $false))
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try {
        if ($utf8.GetString($reader.ReadBytes(8)) -cne 'BREG1003' -or $reader.ReadUInt32() -ne 1001) {
            throw 'flowerbed coverage requires the protocol-1001 BREG1003 registry'
        }
        $null = $reader.ReadUInt32()
        $recordCount = [int]$reader.ReadUInt32()
        foreach ($ignored in 1..4) { $null = $reader.ReadUInt32() }
        $entries = [Collections.Generic.List[object]]::new()
        for ($recordIndex = 0; $recordIndex -lt $recordCount; $recordIndex++) {
            $sequentialId = $reader.ReadUInt32()
            $null = $reader.ReadUInt32()
            $null = $reader.ReadByte()
            $family = $reader.ReadByte()
            foreach ($ignored in 1..5) { $null = $reader.ReadByte() }
            $boxCount = [int]$reader.ReadByte()
            $null = $reader.ReadUInt16()
            $nameLength = [int]$reader.ReadUInt16()
            $stateLength = [int]$reader.ReadUInt32()
            $null = $reader.ReadBytes(32 + 24 * $boxCount)
            $name = $utf8.GetString($reader.ReadBytes($nameLength))
            $canonicalState = $utf8.GetString($reader.ReadBytes($stateLength))
            if ($family -eq 31) {
                if ($name -notin @('minecraft:wildflowers', 'minecraft:pink_petals')) {
                    throw "flowerbed family 31 contains unexpected block: $name"
                }
                $state = $canonicalState | ConvertFrom-Json
                $growthProperty = $state.PSObject.Properties['growth']
                $directionProperty = $state.PSObject.Properties['minecraft:cardinal_direction']
                if ($null -eq $growthProperty -or [string]$growthProperty.Value.type -cne 'int' -or
                    $null -eq $directionProperty -or [string]$directionProperty.Value.type -cne 'string') {
                    throw "flowerbed registry state is not exact typed growth/cardinal syntax: $name|$canonicalState"
                }
                $growth = [int]$growthProperty.Value.value
                $direction = [string]$directionProperty.Value.value
                if ($growth -lt 0 -or $growth -gt 7 -or $direction -notin @('south', 'west', 'north', 'east')) {
                    throw "flowerbed registry contains noncanonical selector: $name growth=$growth direction=$direction"
                }
                $entries.Add([pscustomobject][ordered]@{
                    sequential_id = $sequentialId
                    name = $name
                    growth = $growth
                    direction = $direction
                    canonical_state = $canonicalState
                })
            }
        }
        if ($reader.BaseStream.Position -ne $reader.BaseStream.Length) {
            throw 'BREG1003 registry has trailing bytes'
        }
    }
    finally { $reader.Dispose() }

    $selectors = @($entries | ForEach-Object { "$($_.name)|$($_.growth)|$($_.direction)" } | Sort-Object -Unique)
    if ($entries.Count -ne 64 -or $selectors.Count -ne 64) {
        throw "flowerbed registry coverage changed: records=$($entries.Count) unique_selectors=$($selectors.Count)"
    }
    foreach ($name in @('minecraft:wildflowers', 'minecraft:pink_petals')) {
        foreach ($growth in 0..7) {
            foreach ($direction in @('south', 'west', 'north', 'east')) {
                if ("$name|$growth|$direction" -notin $selectors) {
                    throw "flowerbed registry is missing exact selector: $name growth=$growth direction=$direction"
                }
            }
        }
    }
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-flowerbed-coverage-v1'
        registry_protocol = 1001
        registry_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $RegistryPath).Hash.ToLowerInvariant()
        state_set_sha256 = Get-CanonicalObjectHash -Value @($entries)
        state_count = $entries.Count
        entries = @($entries)
    }
}

function New-FlowerBedGalleryPlan {
    param(
        [Parameter(Mandatory = $true)][ValidateCount(3, 3)][int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite')]
        [string]$Pose,
        [Parameter(Mandatory = $true)][string]$RegistryPath
    )

    $coverage = Get-FlowerBedCoverageEvidence -RegistryPath $RegistryPath
    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $columns = 8
    $gridOrigin = @(-14, 2, -10)
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 18; y = $my + 1; z = $mz - 14 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 18; y = $my + 5; z = $mz + 15 }
    $clearVolume = 37 * 5 * 30
    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 2; z = $mz }
    $relativeCameras = [ordered]@{
        FlowerBedGalleryTop = @(0, 32, 0)
        FlowerBedGalleryNorth = @(0, 10, -44)
        FlowerBedGalleryEast = @(44, 10, 0)
        FlowerBedGalleryOblique = @(-38, 28, -38)
        FlowerBedGalleryObliqueOpposite = @(38, 28, 38)
    }
    # The v1 layout identity predates the opposite capture pose. Keep its
    # descriptor stable because capture coverage does not alter fixture blocks.
    $layoutIdentityCameraOffsets = [ordered]@{
        FlowerBedGalleryTop = $relativeCameras.FlowerBedGalleryTop
        FlowerBedGalleryNorth = $relativeCameras.FlowerBedGalleryNorth
        FlowerBedGalleryEast = $relativeCameras.FlowerBedGalleryEast
        FlowerBedGalleryOblique = $relativeCameras.FlowerBedGalleryOblique
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
    $fixtureCommands.Add("fill $($clearMin.x) $($my + 1) $($clearMin.z) $($clearMax.x) $($my + 1) $($clearMax.z) minecraft:grass_block")
    $referenceCubes = [Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $coverage.entries.Count; $index++) {
        $entry = $coverage.entries[$index]
        $x = $mx + $gridOrigin[0] + 4 * ($index % $columns)
        $y = $my + $gridOrigin[1]
        $z = $mz + $gridOrigin[2] + 3 * [Math]::Floor($index / $columns)
        $stateSuffix = ConvertTo-BdsCanonicalStateSuffix -CanonicalState $entry.canonical_state
        $fixtureCommands.Add("setblock $x $y $z $($entry.name)$stateSuffix")
        $fixtureCommands.Add("setblock $($x + 1) $y $z minecraft:polished_andesite")
        $referenceCubes.Add([pscustomobject][ordered]@{
            state = "$($entry.name)|$($entry.canonical_state)"
            state_offset = @(($x - $mx), ($y - $my), ($z - $mz))
            cube_offset = @(($x + 1 - $mx), ($y - $my), ($z - $mz))
            cube = 'minecraft:polished_andesite'
        })
    }
    if ($fixtureCommands.Count -gt 256) {
        throw "flowerbed gallery command list is not bounded: $($fixtureCommands.Count)"
    }
    $fenceCommand = 'list'
    $fenceMarker = 'players online:'
    $loadAreaName = 'rust_mcbe_flowerbed_gallery'
    $loadAreaCommand = "tickingarea add $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) $loadAreaName true"
    $loadAreaMarker = 'marked for preload.'
    $cleanupCommand = "tickingarea remove $loadAreaName"
    $cleanupMarker = 'Removed ticking area(s)'
    $teleportCommand = [string]$cameraPoses[$Pose].command
    $commands = @($loadAreaCommand) + @($fixtureCommands) + @($fenceCommand, $teleportCommand)
    $relativeLayout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-flowerbed-layout-v1'
        state_set_sha256 = $coverage.state_set_sha256
        gallery_state_count = $coverage.state_count
        clear_min = @(-18, 1, -14)
        clear_max = @(18, 5, 15)
        support_y = 1
        support_block = 'minecraft:grass_block'
        grid_origin = $gridOrigin
        columns = $columns
        spacing = @(4, 3)
        reference_cube_offset = @(1, 0, 0)
        reference_cube = 'minecraft:polished_andesite'
        camera_offsets = [pscustomobject]$layoutIdentityCameraOffsets
    }
    $layoutHash = Get-CanonicalObjectHash -Value $relativeLayout
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v2'
        fixture_kind = 'FlowerBedGallery'
        pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        fixture_layout_hash = $layoutHash
        relative_layout = $relativeLayout
        clear = [pscustomobject][ordered]@{ min = $clearMin; max = $clearMax; volume = $clearVolume }
        gallery_center = $galleryCenter
        camera = $cameraPoses[$Pose]
        camera_poses = [pscustomobject]$cameraPoses
        gallery_state_count = $coverage.state_count
        gallery_states = @($coverage.entries | ForEach-Object { "$($_.name)|$($_.canonical_state)" })
        reference_cubes = @($referenceCubes)
        coverage_evidence = [pscustomobject][ordered]@{
            schema = $coverage.schema
            registry_protocol = $coverage.registry_protocol
            registry_sha256 = $coverage.registry_sha256
            state_set_sha256 = $coverage.state_set_sha256
            state_count = $coverage.state_count
        }
        load_area = [pscustomobject][ordered]@{
            name = $loadAreaName
            requested_min = $clearMin
            requested_max = $clearMax
            preload = $true
            command = $loadAreaCommand
            acknowledgement_marker = $loadAreaMarker
            settle_milliseconds = 3000
            cleanup_command = $cleanupCommand
            cleanup_acknowledgement_marker = $cleanupMarker
        }
        processing_fence = [pscustomobject][ordered]@{ command = $fenceCommand; stdout_marker = $fenceMarker }
        fixture_commands = @($fixtureCommands)
        commands = $commands
        command_count = $commands.Count
        teleport_command = $teleportCommand
        settle_milliseconds = 3000
    }
    return [pscustomobject][ordered]@{
        Pose = $Pose
        LoadAreaName = $loadAreaName
        LoadAreaCommand = $loadAreaCommand
        LoadAreaMarker = $loadAreaMarker
        LoadAreaSettleMilliseconds = 3000
        CleanupCommand = $cleanupCommand
        CleanupMarker = $cleanupMarker
        FixtureCommands = @($fixtureCommands)
        GalleryCommands = @($fixtureCommands)
        FenceMarker = $fenceMarker
        FenceCommand = $fenceCommand
        TeleportCommand = $teleportCommand
        Commands = $commands
        Manifest = $manifest
        CoverageEntries = @($coverage.entries)
    }
}
