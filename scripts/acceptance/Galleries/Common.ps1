function New-OpaqueVisualFixturePlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('Front', 'Back')]
        [string]$Pose
    )

    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 24; y = $my + 1; z = $mz - 16 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 24; y = $my + 12; z = $mz + 16 }
    $clearVolume = ($clearMax.x - $clearMin.x + 1) *
        ($clearMax.y - $clearMin.y + 1) *
        ($clearMax.z - $clearMin.z + 1)
    if ($clearVolume -gt 32768) {
        throw "visual fixture clear volume exceeds BDS fill limit: $clearVolume"
    }

    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 3; z = $mz + 4 }
    $camera = if ($Pose -eq 'Front') {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 12; z = $mz - 24 }
    }
    else {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 10; z = $mz + 32 }
    }

    $galleryCommands = [Collections.Generic.List[string]]::new()
    $galleryCommands.Add(
        "fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air"
    )
    $galleryCommands.Add("fill $($mx - 3) $($my + 1) $($mz - 11) $($mx + 3) $($my + 1) $($mz + 3) minecraft:oak_planks")
    $galleryCommands.Add("setblock $mx $($my + 1) $mz minecraft:air")
    $galleryCommands.Add("fill $($mx + 14) $my $($mz + 5) $($mx + 15) $my $($mz + 6) minecraft:stone")

    $blockDefinitions = @(
        [pscustomobject][ordered]@{ label = 'stone'; block = 'minecraft:stone'; x_offset = -21 },
        [pscustomobject][ordered]@{ label = 'dirt'; block = 'minecraft:dirt'; x_offset = -16 },
        [pscustomobject][ordered]@{ label = 'grass'; block = 'minecraft:grass_block'; x_offset = -11 },
        [pscustomobject][ordered]@{ label = 'oak_planks'; block = 'minecraft:oak_planks'; x_offset = -6 },
        [pscustomobject][ordered]@{ label = 'coal_ore'; block = 'minecraft:coal_ore'; x_offset = -1 },
        [pscustomobject][ordered]@{ label = 'iron_ore'; block = 'minecraft:iron_ore'; x_offset = 4 },
        [pscustomobject][ordered]@{ label = 'diamond_ore'; block = 'minecraft:diamond_ore'; x_offset = 9 },
        [pscustomobject][ordered]@{ label = 'sand'; block = 'minecraft:sand'; x_offset = 14 },
        [pscustomobject][ordered]@{ label = 'glass'; block = 'minecraft:glass'; x_offset = 19 }
    )
    $manifestBlocks = [Collections.Generic.List[object]]::new()
    foreach ($definition in $blockDefinitions) {
        $minimum = [pscustomobject][ordered]@{
            x = $mx + [int]$definition.x_offset
            y = $my + 1
            z = $mz + 5
        }
        $maximum = [pscustomobject][ordered]@{
            x = $minimum.x + 1
            y = $minimum.y + 1
            z = $minimum.z + 1
        }
        $galleryCommands.Add(
            "fill $($minimum.x) $($minimum.y) $($minimum.z) $($maximum.x) $($maximum.y) $($maximum.z) $($definition.block)"
        )
        $manifestBlocks.Add([pscustomobject][ordered]@{
            label = $definition.label
            block = $definition.block
            min = $minimum
            max = $maximum
            size = @(2, 2, 2)
        })
    }

    foreach ($xOffset in @(-9, -8, -7)) {
        $galleryCommands.Add("setblock $($mx + $xOffset) $($my + 2) $mz minecraft:oak_stairs")
    }
    foreach ($xOffset in @(7, 8, 9)) {
        $galleryCommands.Add("setblock $($mx + $xOffset) $($my + 2) $mz minecraft:glass_pane")
    }
    $galleryCommands.Add("fill $($mx - 9) $($my + 5) $($mz + 1) $($mx - 5) $($my + 5) $($mz + 1) minecraft:oak_log [`"pillar_axis`"=`"x`"]")
    $galleryCommands.Add("fill $mx $($my + 3) $($mz + 1) $mx $($my + 7) $($mz + 1) minecraft:oak_log [`"pillar_axis`"=`"y`"]")
    $galleryCommands.Add("fill $($mx + 5) $($my + 4) $($mz - 2) $($mx + 5) $($my + 4) $($mz + 2) minecraft:oak_log [`"pillar_axis`"=`"z`"]")
    $galleryCommands.Add("fill $($mx - 2) $($my + 7) $($mz - 15) $($mx + 2) $($my + 7) $($mz - 13) minecraft:glass")
    $galleryCommands.Add("fill $($mx - 2) $($my + 3) $($mz + 13) $($mx + 2) $($my + 3) $($mz + 15) minecraft:glass")

    $fenceMarker = 'players online:'
    $fenceCommand = 'list'
    $teleportCommand = "tp @a[name=RustMCBE] $($camera.x) $($camera.y) $($camera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $commands = @($galleryCommands) + @($fenceCommand, $teleportCommand)
    if ($commands.Count -gt 64) {
        throw "visual fixture command list is not bounded: $($commands.Count)"
    }

    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v1'
        pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        clear = [pscustomobject][ordered]@{
            min = $clearMin
            max = $clearMax
            volume = $clearVolume
        }
        gallery_center = $galleryCenter
        camera = [pscustomobject][ordered]@{
            position = $camera
            target = $galleryCenter
        }
        runway = [pscustomobject][ordered]@{
            min = [pscustomobject][ordered]@{ x = $mx - 3; y = $my + 1; z = $mz - 11 }
            max = [pscustomobject][ordered]@{ x = $mx + 3; y = $my + 1; z = $mz + 3 }
            mutation_aperture = [pscustomobject][ordered]@{ x = $mx; y = $my + 1; z = $mz }
        }
        blocks = @($manifestBlocks)
        diagnostics = [pscustomobject][ordered]@{
            non_full_blocks = @('minecraft:oak_stairs', 'minecraft:glass_pane')
            log_axes = @('x', 'y', 'z')
        }
        processing_fence = [pscustomobject][ordered]@{
            command = $fenceCommand
            stdout_marker = $fenceMarker
        }
        teleport_command = $teleportCommand
        settle_milliseconds = 3000
    }

    return [pscustomobject][ordered]@{
        Pose = $Pose
        GalleryCommands = @($galleryCommands)
        FenceMarker = $fenceMarker
        FenceCommand = $fenceCommand
        TeleportCommand = $teleportCommand
        Commands = $commands
        Manifest = $manifest
    }
}

function ConvertTo-BdsCanonicalStateSuffix {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$CanonicalState)

    if ($CanonicalState -ceq '{}') {
        return ''
    }
    $state = $CanonicalState | ConvertFrom-Json
    $assignments = [Collections.Generic.List[string]]::new()
    foreach ($property in @($state.PSObject.Properties | Sort-Object Name)) {
        $typed = $property.Value
        $value = switch ([string]$typed.type) {
            'byte' {
                if ([int]$typed.value -eq 0) { 'false' }
                elseif ([int]$typed.value -eq 1) { 'true' }
                else { [string][int]$typed.value }
            }
            'int' { [string][int]$typed.value }
            'string' { '"' + ([string]$typed.value).Replace('\', '\\').Replace('"', '\"') + '"' }
            default { throw "unsupported canonical state type '$($typed.type)' for '$($property.Name)'" }
        }
        $assignments.Add(('"{0}"={1}' -f $property.Name, $value))
    }
    return ' [' + ($assignments -join ',') + ']'
}

function New-VisualFixturePlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('Front', 'Back', 'LeafGalleryFront', 'LeafGalleryBack', 'CrossCropGalleryFront', 'CrossCropGalleryBack', 'AquaticGalleryFront', 'AquaticGalleryBack', 'WaterGalleryFront', 'WaterGalleryBack', 'FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite', 'SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite', 'VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')]
        [string]$Pose,
        [string]$RegistryPath,
        [string]$AssetsPath
    )

    if ($Pose.StartsWith('LeafGallery', [StringComparison]::Ordinal)) {
        return New-LeafGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose
    }
    if ($Pose.StartsWith('CrossCropGallery', [StringComparison]::Ordinal)) {
        return New-CrossCropGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    }
    if ($Pose.StartsWith('AquaticGallery', [StringComparison]::Ordinal)) {
        return New-AquaticGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    }
    if ($Pose.StartsWith('WaterGallery', [StringComparison]::Ordinal)) {
        return New-WaterGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    }
    if ($Pose.StartsWith('FlowerBedGallery', [StringComparison]::Ordinal)) {
        return New-FlowerBedGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose -RegistryPath $RegistryPath
    }
    if ($Pose.StartsWith('SlabStairGallery', [StringComparison]::Ordinal)) {
        return New-SlabStairGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    }
    if ($Pose.StartsWith('VineGallery', [StringComparison]::Ordinal)) {
        return New-VineGalleryPlan -MutationCoordinate $MutationCoordinate -Pose $Pose -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    }
    return New-OpaqueVisualFixturePlan -MutationCoordinate $MutationCoordinate -Pose $Pose
}
