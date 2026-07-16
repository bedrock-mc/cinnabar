function New-WaterGalleryPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('WaterGalleryFront', 'WaterGalleryBack')]
        [string]$Pose,
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $coverage = Get-AquaticCoverageEvidence -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    $seagrass = @($coverage.entries | Where-Object {
        $_.name -ceq 'minecraft:seagrass' -and $_.canonical_state -notmatch 'double_(top|bot)'
    })
    if ($seagrass.Count -ne 1) {
        throw "water gallery expected one canonical single seagrass state, found $($seagrass.Count)"
    }

    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 4; z = $mz }
    $frontCamera = [pscustomobject][ordered]@{ x = $mx; y = $my + 10; z = $mz - 24 }
    $backCamera = [pscustomobject][ordered]@{ x = $mx; y = $my + 9; z = $mz + 24 }
    $initialCamera = if ($Pose -ceq 'WaterGalleryFront') { $frontCamera } else { $backCamera }
    $resortCamera = if ($Pose -ceq 'WaterGalleryFront') { $backCamera } else { $frontCamera }

    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $liquidWitnessBlocks = [Collections.Generic.List[object]]::new()
    $fixtureCommands.Add("fill $($mx - 18) $($my + 1) $($mz - 12) $($mx + 18) $($my + 9) $($mz + 12) minecraft:air")
    $fixtureCommands.Add("fill $($mx - 18) $($my + 1) $($mz - 12) $($mx + 18) $($my + 1) $($mz + 12) minecraft:stone")
    $fixtureCommands.Add("fill $($mx - 17) $($my + 2) $($mz - 8) $($mx - 5) $($my + 2) $($mz - 2) minecraft:stone")
    $fixtureCommands.Add("fill $($mx - 17) $($my + 3) $($mz - 8) $($mx - 5) $($my + 3) $($mz - 8) minecraft:glass")
    $fixtureCommands.Add("fill $($mx - 17) $($my + 3) $($mz - 2) $($mx - 5) $($my + 3) $($mz - 2) minecraft:glass")
    $fixtureCommands.Add("fill $($mx - 17) $($my + 3) $($mz - 7) $($mx - 17) $($my + 3) $($mz - 3) minecraft:glass")
    $fixtureCommands.Add("fill $($mx - 5) $($my + 3) $($mz - 7) $($mx - 5) $($my + 3) $($mz - 3) minecraft:glass")
    $fixtureCommands.Add("fill $($mx - 16) $($my + 3) $($mz - 7) $($mx - 6) $($my + 3) $($mz - 3) minecraft:water")
    foreach ($x in ($mx - 16)..($mx - 6)) {
        foreach ($z in ($mz - 7)..($mz - 3)) {
            $liquidWitnessBlocks.Add([pscustomobject][ordered]@{ x = $x; y = $my + 3; z = $z })
        }
    }

    $flowWitnesses = [Collections.Generic.List[object]]::new()
    $fixtureCommands.Add("fill $($mx - 3) $($my + 5) $($mz - 1) $($mx + 4) $($my + 8) $($mz + 1) minecraft:glass")
    foreach ($depth in 0..5) {
        $x = $mx - 2 + $depth
        $supportY = $my + 6 - [Math]::Floor($depth / 2)
        $waterY = $supportY + 1
        $fixtureCommands.Add("setblock $x $waterY $mz minecraft:water [`"liquid_depth`"=$depth]")
        $flowWitnesses.Add([pscustomobject][ordered]@{ x = $x; y = $waterY; z = $mz; liquid_depth = $depth })
        $liquidWitnessBlocks.Add([pscustomobject][ordered]@{ x = $x; y = $waterY; z = $mz })
    }

    $plant = [pscustomobject][ordered]@{ x = $mx + 8; y = $my + 3; z = $mz - 5 }
    $fixtureCommands.Add("setblock $($plant.x) $($plant.y - 1) $($plant.z) minecraft:dirt")
    $fixtureCommands.Add("setblock $($plant.x) $($plant.y) $($plant.z) $($seagrass[0].name)$(ConvertTo-BdsCanonicalStateSuffix -CanonicalState $seagrass[0].canonical_state)")
    $liquidWitnessBlocks.Add([pscustomobject][ordered]@{ x = $plant.x; y = $plant.y; z = $plant.z })

    $tintWitnesses = @(
        [pscustomobject][ordered]@{ label = 'near'; x = $mx + 10; y = $my + 3; z = $mz + 4 },
        [pscustomobject][ordered]@{ label = 'far'; x = $mx + 14; y = $my + 3; z = $mz + 4 }
    )
    foreach ($witness in $tintWitnesses) {
        $fixtureCommands.Add("fill $($witness.x - 1) $($witness.y - 1) $($witness.z - 1) $($witness.x + 1) $($witness.y + 1) $($witness.z + 1) minecraft:glass")
        $fixtureCommands.Add("setblock $($witness.x) $($witness.y) $($witness.z) minecraft:water")
        $liquidWitnessBlocks.Add([pscustomobject][ordered]@{ x = $witness.x; y = $witness.y; z = $witness.z })
    }

    $fixtureCommands.Add("fill $($mx + 5) $($my + 2) $($mz - 8) $($mx + 5) $($my + 6) $($mz - 2) minecraft:glass")
    $fixtureCommands.Add("fill $($mx + 7) $($my + 2) $($mz - 8) $($mx + 7) $($my + 6) $($mz - 2) minecraft:stone")
    $fixtureCommands.Add("setblock $($mx + 6) $($my + 3) $($mz - 5) minecraft:water")
    $liquidWitnessBlocks.Add([pscustomobject][ordered]@{ x = $mx + 6; y = $my + 3; z = $mz - 5 })
    if ($fixtureCommands.Count -ne 24) {
        throw "water gallery command count changed: $($fixtureCommands.Count)"
    }

    $initialTeleport = "tp @a[name=RustMCBE] $($initialCamera.x) $($initialCamera.y) $($initialCamera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $resortTeleport = "tp @a[name=RustMCBE] $($resortCamera.x) $($resortCamera.y) $($resortCamera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $relativeLayout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-water-layout-v1'
        clear = @(-18, 1, -12, 18, 9, 12)
        floor = @(-18, 1, -12, 18, 1, 12)
        still_pool = [pscustomobject][ordered]@{ min = @(-16, 3, -7); max = @(-6, 3, -3); liquid_depth = 0 }
        downhill_flow_edge = @($flowWitnesses | ForEach-Object {
            [pscustomobject][ordered]@{
                offset = @(([int]$_.x - $mx), ([int]$_.y - $my), ([int]$_.z - $mz))
                liquid_depth = [int]$_.liquid_depth
            }
        })
        flow_enclosure = [pscustomobject][ordered]@{
            block = 'glass'
            min = @(-3, 5, -1)
            max = @(4, 8, 1)
        }
        waterlogged_plant = [pscustomobject][ordered]@{
            offset = @(([int]$plant.x - $mx), ([int]$plant.y - $my), ([int]$plant.z - $mz))
            block = [string]$seagrass[0].name
            canonical_state = [string]$seagrass[0].canonical_state
        }
        biome_tint_witnesses = @($tintWitnesses | ForEach-Object {
            [pscustomobject][ordered]@{
                label = [string]$_.label
                offset = @(([int]$_.x - $mx), ([int]$_.y - $my), ([int]$_.z - $mz))
            }
        })
        biome_tint_evidence = [pscustomobject][ordered]@{
            kind = 'runtime-biome-index-water-tint-lookup'
            distinct_biome_colours_claimed = $false
            minimum_rendered_distinct_tint_count = [uint64]1
            multi_biome_lookup_parity_test = 'bedrock-client::tests::compiled_and_live_biome_tables_preserve_raw_id_water_colour_parity'
            note = 'Witnesses require one real rendered runtime tint. This fixture cannot set BDS biomes and does not claim they cross a biome boundary; the named app test separately proves distinct raw-biome water colours survive live lookup into the render table.'
        }
        blend_edge = [pscustomobject][ordered]@{
            water = @(6, 3, -5)
            glass = @(5, 2, -8, 5, 6, -2)
            opaque_backdrop = @(7, 2, -8, 7, 6, -2)
        }
    }
    $layoutHash = Get-CanonicalObjectHash -Value $relativeLayout
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v1'
        fixture_kind = 'WaterGallery'
        pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        fixture_layout_hash = $layoutHash
        relative_layout = $relativeLayout
        gallery_center = $galleryCenter
        camera = [pscustomobject][ordered]@{ position = $initialCamera; target = $galleryCenter }
        camera_poses = [pscustomobject][ordered]@{
            initial = [pscustomobject][ordered]@{ position = $initialCamera; target = $galleryCenter; command = $initialTeleport }
            resort = [pscustomobject][ordered]@{ position = $resortCamera; target = $galleryCenter; command = $resortTeleport }
        }
        fixture_commands = @($fixtureCommands)
        processing_fence = [pscustomobject][ordered]@{ command = 'list'; stdout_marker = 'players online:' }
        teleport_command = $initialTeleport
        initial_camera_fence = [pscustomobject][ordered]@{ command = 'list'; stdout_marker = 'players online:' }
        camera_resort_command = $resortTeleport
        camera_resort_fence = [pscustomobject][ordered]@{ command = 'list'; stdout_marker = 'players online:' }
        performance = [pscustomobject][ordered]@{
            maximum_p99_frame_ms = [double](1000.0 / 60.0)
            measured_session_excludes_presentation_exit_grace = $true
        }
        camera_resort_settle_milliseconds = 1000
        settle_milliseconds = 3000
        coverage_evidence = [pscustomobject][ordered]@{
            state_set_sha256 = $coverage.state_set_sha256
            assets_sha256 = $coverage.assets_sha256
            registry_sha256 = $coverage.registry_sha256
        }
    }
    return [pscustomobject][ordered]@{
        Pose = $Pose
        FixtureCommands = @($fixtureCommands)
        GalleryCommands = @($fixtureCommands)
        FenceMarker = 'players online:'
        FenceCommand = 'list'
        TeleportCommand = $initialTeleport
        CameraResortCommand = $resortTeleport
        LiquidWitnessBlocks = @($liquidWitnessBlocks)
        ValidateFixtureCommandResults = $true
        Commands = @($fixtureCommands) + @('list', $initialTeleport, 'list', $resortTeleport, 'list')
        Manifest = $manifest
    }
}

function New-WaterGalleryTransparentWitnessRequest {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][ValidateRange(1, [long]::MaxValue)][uint64]$Revision
    )

    $blocksProperty = $Plan.PSObject.Properties['LiquidWitnessBlocks']
    if ($null -eq $blocksProperty -or @($blocksProperty.Value).Count -eq 0) {
        throw 'water gallery transparent witness has no liquid-bearing blocks'
    }
    $byIdentity = [ordered]@{}
    foreach ($block in @($blocksProperty.Value)) {
        $x = [int][Math]::Floor([double]$block.x / 16.0)
        $y = [int][Math]::Floor([double]$block.y / 16.0)
        $z = [int][Math]::Floor([double]$block.z / 16.0)
        $identity = "$x,$y,$z"
        if (-not $byIdentity.Contains($identity)) {
            $byIdentity[$identity] = [pscustomobject][ordered]@{ x = $x; y = $y; z = $z }
        }
    }
    $keys = @($byIdentity.Values | Sort-Object x, y, z)
    if ($keys.Count -eq 0 -or $keys.Count -gt 64) {
        throw "water gallery transparent witness key count is outside 1..64: $($keys.Count)"
    }
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-transparent-witness-v1'
        revision = $Revision
        dimension = 0
        sub_chunks = $keys
    }
}
