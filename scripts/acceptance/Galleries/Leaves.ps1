function New-LeafGalleryPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('LeafGalleryFront', 'LeafGalleryBack')]
        [string]$Pose
    )

    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $sourceMutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 18; y = $my + 1; z = $mz - 14 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 18; y = $my + 12; z = $mz + 18 }
    $clearVolume = ($clearMax.x - $clearMin.x + 1) *
        ($clearMax.y - $clearMin.y + 1) *
        ($clearMax.z - $clearMin.z + 1)
    if ($clearVolume -gt 32768) {
        throw "leaf gallery clear volume exceeds BDS fill limit: $clearVolume"
    }

    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 5; z = $mz + 7 }
    $camera = if ($Pose -ceq 'LeafGalleryFront') {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 12; z = $mz - 22 }
    }
    else {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 10; z = $mz + 36 }
    }
    $selfColored = @(
        'minecraft:cherry_leaves',
        'minecraft:azalea_leaves',
        'minecraft:azalea_leaves_flowered'
    )
    $tintDeferred = @(
        'minecraft:oak_leaves',
        'minecraft:birch_leaves',
        'minecraft:spruce_leaves'
    )
    $definitions = @(
        [pscustomobject][ordered]@{ label = 'cherry_self_colored'; category = 'self_colored'; block = $selfColored[0]; x_offset = -10 },
        [pscustomobject][ordered]@{ label = 'azalea_self_colored'; category = 'self_colored'; block = $selfColored[1]; x_offset = -8 },
        [pscustomobject][ordered]@{ label = 'azalea_flowered_self_colored'; category = 'self_colored'; block = $selfColored[2]; x_offset = -6 },
        [pscustomobject][ordered]@{ label = 'oak_tint_deferred'; category = 'tint_deferred'; block = $tintDeferred[0]; x_offset = 4 },
        [pscustomobject][ordered]@{ label = 'birch_tint_deferred'; category = 'tint_deferred'; block = $tintDeferred[1]; x_offset = 6 },
        [pscustomobject][ordered]@{ label = 'spruce_tint_deferred'; category = 'tint_deferred'; block = $tintDeferred[2]; x_offset = 8 }
    )
    $layoutBlocks = @($definitions | ForEach-Object {
        [pscustomobject][ordered]@{
            label = $_.label
            category = $_.category
            block = $_.block
            min_offset = @(([int]$_.x_offset), 2, 5)
            max_offset = @(([int]$_.x_offset + 1), 3, 6)
            persistent_bit = $true
            update_bit = $false
        }
    })
    $layout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-leaf-gallery-layout-v1'
        blocks = $layoutBlocks
        leaf_adjacency = @(
            @('cherry_self_colored', 'azalea_self_colored'),
            @('azalea_self_colored', 'azalea_flowered_self_colored'),
            @('oak_tint_deferred', 'birch_tint_deferred'),
            @('birch_tint_deferred', 'spruce_tint_deferred')
        )
        opaque_backing = @(
            [pscustomobject][ordered]@{ min_offset = @(-10, 2, 7); max_offset = @(-5, 3, 7); block = 'minecraft:stone' },
            [pscustomobject][ordered]@{ min_offset = @(4, 2, 7); max_offset = @(9, 3, 7); block = 'minecraft:stone' }
        )
        panels = @(
            [pscustomobject][ordered]@{ distance = 'near'; block = 'minecraft:cherry_leaves'; min_offset = @(-3, 4, 1); max_offset = @(3, 9, 1); backing_z = 2 },
            [pscustomobject][ordered]@{ distance = 'far'; block = 'minecraft:azalea_leaves'; min_offset = @(-3, 4, 15); max_offset = @(3, 9, 15); backing_z = 16 }
        )
    }
    $layoutHash = Get-CanonicalObjectHash -Value $layout

    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $fillVolumes = [Collections.Generic.List[int]]::new()
    $fixtureCommands.Add(
        "fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air"
    )
    $fillVolumes.Add($clearVolume)
    $fixtureCommands.Add("fill $($mx - 14) $my $($mz - 2) $($mx + 14) $my $($mz + 17) minecraft:oak_planks")
    $fillVolumes.Add(580)
    $manifestBlocks = [Collections.Generic.List[object]]::new()
    foreach ($definition in $definitions) {
        $minimum = [pscustomobject][ordered]@{
            x = $mx + [int]$definition.x_offset
            y = $my + 2
            z = $mz + 5
        }
        $maximum = [pscustomobject][ordered]@{
            x = $minimum.x + 1
            y = $minimum.y + 1
            z = $minimum.z + 1
        }
        $fixtureCommands.Add(
            "fill $($minimum.x) $($minimum.y) $($minimum.z) $($maximum.x) $($maximum.y) $($maximum.z) $($definition.block) $LeafStateSuffix"
        )
        $fillVolumes.Add(8)
        $manifestBlocks.Add([pscustomobject][ordered]@{
            label = $definition.label
            category = $definition.category
            block = $definition.block
            min = $minimum
            max = $maximum
            size = @(2, 2, 2)
            persistent_bit = $true
            update_bit = $false
        })
    }
    $fixtureCommands.Add("fill $($mx - 10) $($my + 2) $($mz + 7) $($mx - 5) $($my + 3) $($mz + 7) minecraft:stone")
    $fillVolumes.Add(12)
    $fixtureCommands.Add("fill $($mx + 4) $($my + 2) $($mz + 7) $($mx + 9) $($my + 3) $($mz + 7) minecraft:stone")
    $fillVolumes.Add(12)
    $fixtureCommands.Add("fill $($mx - 3) $($my + 4) $($mz + 1) $($mx + 3) $($my + 9) $($mz + 1) minecraft:cherry_leaves $LeafStateSuffix")
    $fillVolumes.Add(42)
    $fixtureCommands.Add("fill $($mx - 3) $($my + 4) $($mz + 2) $($mx + 3) $($my + 9) $($mz + 2) minecraft:stone")
    $fillVolumes.Add(42)
    $fixtureCommands.Add("fill $($mx - 3) $($my + 4) $($mz + 15) $($mx + 3) $($my + 9) $($mz + 15) minecraft:azalea_leaves $LeafStateSuffix")
    $fillVolumes.Add(42)
    $fixtureCommands.Add("fill $($mx - 3) $($my + 4) $($mz + 16) $($mx + 3) $($my + 9) $($mz + 16) minecraft:stone")
    $fillVolumes.Add(42)

    $fenceCommand = 'list'
    $fenceMarker = 'players online:'
    $teleportCommand = "tp @a[name=RustMCBE] $($camera.x) $($camera.y) $($camera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $commands = @($fixtureCommands) + @($fenceCommand, $teleportCommand)
    if ($commands.Count -gt 64) {
        throw "leaf gallery command list is not bounded: $($commands.Count)"
    }
    foreach ($volume in $fillVolumes) {
        if ($volume -gt 32768) {
            throw "leaf gallery fill exceeds BDS limit: $volume"
        }
    }

    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v2'
        fixture_kind = 'LeafGallery'
        pose = $Pose
        source_mutation = $sourceMutation
        fixture_layout_hash = $layoutHash
        layout = $layout
        self_colored = $selfColored
        tint_deferred = $tintDeferred
        blocks = @($manifestBlocks)
        leaf_adjacency = @($layout.leaf_adjacency)
        opaque_backing = @($layout.opaque_backing)
        panels = @($layout.panels)
        clear = [pscustomobject][ordered]@{ min = $clearMin; max = $clearMax; volume = $clearVolume }
        fill_volumes = @($fillVolumes)
        gallery_center = $galleryCenter
        camera = [pscustomobject][ordered]@{ position = $camera; target = $galleryCenter }
        processing_fence = [pscustomobject][ordered]@{ command = $fenceCommand; stdout_marker = $fenceMarker }
        fixture_commands = @($fixtureCommands)
        commands = $commands
        command_count = $commands.Count
        teleport_command = $teleportCommand
        settle_milliseconds = 3000
    }
    return [pscustomobject][ordered]@{
        Pose = $Pose
        FixtureCommands = @($fixtureCommands)
        GalleryCommands = @($fixtureCommands)
        FenceMarker = $fenceMarker
        FenceCommand = $fenceCommand
        TeleportCommand = $teleportCommand
        Commands = $commands
        Manifest = $manifest
    }
}

function New-LeafForestPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('Baseline', 'FullView')]
        [string]$Mode
    )

    $sx = [int]$MutationCoordinate[0]
    $sy = [int]$MutationCoordinate[1]
    $sz = [int]$MutationCoordinate[2]
    $offsetBlocks = $LeafForestOffsetChunks * 16
    $camera = [pscustomobject][ordered]@{
        x = $sx + $offsetBlocks
        y = $sy + 12
        z = $sz + $offsetBlocks
    }
    $targetMutation = [pscustomobject][ordered]@{
        x = $camera.x
        y = $sy
        z = $camera.z + $LeafForestMutationZOffset
    }
    $sourceMutation = [pscustomobject][ordered]@{ x = $sx; y = $sy; z = $sz }
    $selfColored = @(
        'minecraft:cherry_leaves',
        'minecraft:azalea_leaves',
        'minecraft:azalea_leaves_flowered'
    )
    $tintDeferred = @(
        'minecraft:oak_leaves',
        'minecraft:birch_leaves',
        'minecraft:spruce_leaves'
    )
    $canopies = @(
        [pscustomobject][ordered]@{ label = 'northwest_cherry'; category = 'self_colored'; x_offset = -12; z_offset = -10; block = $selfColored[0] },
        [pscustomobject][ordered]@{ label = 'north_oak'; category = 'tint_deferred'; x_offset = 0; z_offset = -10; block = $tintDeferred[0] },
        [pscustomobject][ordered]@{ label = 'northeast_azalea'; category = 'self_colored'; x_offset = 12; z_offset = -10; block = $selfColored[1] },
        [pscustomobject][ordered]@{ label = 'west_birch'; category = 'tint_deferred'; x_offset = -12; z_offset = 4; block = $tintDeferred[1] },
        [pscustomobject][ordered]@{ label = 'center_flowered'; category = 'self_colored'; x_offset = 0; z_offset = 4; block = $selfColored[2] },
        [pscustomobject][ordered]@{ label = 'east_spruce'; category = 'tint_deferred'; x_offset = 12; z_offset = 4; block = $tintDeferred[2] },
        [pscustomobject][ordered]@{ label = 'southwest_cherry'; category = 'self_colored'; x_offset = -6; z_offset = 17; block = $selfColored[0] },
        [pscustomobject][ordered]@{ label = 'southeast_azalea'; category = 'self_colored'; x_offset = 6; z_offset = 17; block = $selfColored[1] }
    )
    $layoutCanopies = @($canopies | ForEach-Object {
        [pscustomobject][ordered]@{
            label = $_.label
            category = $_.category
            x_offset = [int]$_.x_offset
            z_offset = [int]$_.z_offset
            block = $_.block
            leaf_min_offset = @(([int]$_.x_offset - 2), 3, ([int]$_.z_offset - 2))
            leaf_max_offset = @(([int]$_.x_offset + 2), 7, ([int]$_.z_offset + 2))
            trunk_height = 6
            persistent_bit = $true
            update_bit = $false
        }
    })
    $layout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-leaf-forest-layout-v1'
        clear_min_offset = @(-24, 0, -24)
        clear_max_offset = @(24, 12, 24)
        platform_min_offset = @(-20, -1, -20)
        platform_max_offset = @(20, -1, 20)
        camera_position_offset = @(0, 12, -12)
        camera_target_offset = @(0, 4, 0)
        target_mutation_offset = @(0, 0, 0)
        canopies = $layoutCanopies
    }
    $layoutHash = Get-CanonicalObjectHash -Value $layout
    $clearMin = [pscustomobject][ordered]@{ x = $targetMutation.x - 24; y = $sy; z = $targetMutation.z - 24 }
    $clearMax = [pscustomobject][ordered]@{ x = $targetMutation.x + 24; y = $sy + 12; z = $targetMutation.z + 24 }
    $clearVolume = 49 * 13 * 49
    if ($clearVolume -gt 32768) {
        throw "leaf forest clear volume exceeds BDS fill limit: $clearVolume"
    }
    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $fillVolumes = [Collections.Generic.List[int]]::new()
    $fixtureCommands.Add("fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air")
    $fillVolumes.Add($clearVolume)
    $fixtureCommands.Add("fill $($targetMutation.x - 20) $($sy - 1) $($targetMutation.z - 20) $($targetMutation.x + 20) $($sy - 1) $($targetMutation.z + 20) minecraft:stone")
    $fillVolumes.Add(1681)
    $fixtureCommands.Add("setblock $($targetMutation.x) $($targetMutation.y) $($targetMutation.z) minecraft:diamond_block")
    foreach ($canopy in $canopies) {
        $x = $targetMutation.x + [int]$canopy.x_offset
        $z = $targetMutation.z + [int]$canopy.z_offset
        $fixtureCommands.Add("fill $($x - 2) $($sy + 3) $($z - 2) $($x + 2) $($sy + 7) $($z + 2) $($canopy.block) $LeafStateSuffix")
        $fillVolumes.Add(125)
        $fixtureCommands.Add("fill $x $sy $z $x $($sy + 5) $z minecraft:oak_log [`"pillar_axis`"=`"y`"]")
        $fillVolumes.Add(6)
    }
    $fenceCommand = 'list'
    $fenceMarker = 'players online:'
    $loadAreaName = $LeafForestLoadAreaName
    $loadAreaCommand = "tickingarea add $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) $loadAreaName true"
    $loadAreaMarker = 'marked for preload.'
    $cleanupCommand = "tickingarea remove $loadAreaName"
    $cleanupMarker = 'Removed ticking area(s)'
    $teleportCommand = "tp @a[name=RustMCBE] $($camera.x) $($camera.y) $($camera.z) facing $($targetMutation.x) $($targetMutation.y + 4) $($targetMutation.z)"
    $commands = @($loadAreaCommand) + @($fixtureCommands) + @($fenceCommand, $teleportCommand)
    if ($commands.Count -gt 64) {
        throw "leaf forest command list is not bounded: $($commands.Count)"
    }
    foreach ($volume in $fillVolumes) {
        if ($volume -gt 32768) {
            throw "leaf forest fill exceeds BDS limit: $volume"
        }
    }
    $pose = if ($Mode -ceq 'FullView') { 'LeafForestFullView' } else { 'LeafForestBaseline' }
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v2'
        fixture_kind = 'LeafForest'
        pose = $pose
        source_mutation = $sourceMutation
        target_mutation = $targetMutation
        fixture_layout_hash = $layoutHash
        layout = $layout
        self_colored = $selfColored
        tint_deferred = $tintDeferred
        canopies = $layoutCanopies
        clear = [pscustomobject][ordered]@{ min = $clearMin; max = $clearMax; volume = $clearVolume }
        fill_volumes = @($fillVolumes)
        camera = [pscustomobject][ordered]@{
            position = $camera
            target = [pscustomobject][ordered]@{ x = $targetMutation.x; y = $targetMutation.y + 4; z = $targetMutation.z }
        }
        offset_chunks = $LeafForestOffsetChunks
        radius_chunks = 16
        load_area = [pscustomobject][ordered]@{
            name = $loadAreaName
            requested_min = $clearMin
            requested_max = $clearMax
            preload = $true
            command = $loadAreaCommand
            acknowledgement_marker = $loadAreaMarker
            settle_milliseconds = $LeafForestLoadAreaSettleMilliseconds
            cleanup_command = $cleanupCommand
            cleanup_acknowledgement_marker = $cleanupMarker
        }
        processing_fence = [pscustomobject][ordered]@{ command = $fenceCommand; stdout_marker = $fenceMarker }
        fixture_commands = @($fixtureCommands)
        commands = $commands
        command_count = $commands.Count
        teleport_command = $teleportCommand
        mutation_blocks = @('minecraft:gold_block', 'minecraft:diamond_block')
    }
    return [pscustomobject][ordered]@{
        Pose = $pose
        Target = $camera
        TargetMutation = $targetMutation
        OffsetChunks = $LeafForestOffsetChunks
        LoadAreaName = $loadAreaName
        LoadAreaCommand = $loadAreaCommand
        LoadAreaMarker = $loadAreaMarker
        LoadAreaSettleMilliseconds = $LeafForestLoadAreaSettleMilliseconds
        CleanupCommand = $cleanupCommand
        CleanupMarker = $cleanupMarker
        FixtureCommands = @($fixtureCommands)
        GalleryCommands = @($fixtureCommands)
        FenceMarker = $fenceMarker
        FenceCommand = $fenceCommand
        TeleportCommand = $teleportCommand
        Commands = $commands
        Manifest = $manifest
    }
}
