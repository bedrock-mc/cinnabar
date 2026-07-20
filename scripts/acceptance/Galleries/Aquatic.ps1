function Get-AquaticCoverageEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $reader = [IO.BinaryReader]::new([IO.MemoryStream]::new($registryBytes, $false))
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try {
        if ($utf8.GetString($reader.ReadBytes(8)) -cne 'BREG1003' -or $reader.ReadUInt32() -ne 1001) {
            throw 'aquatic coverage requires the protocol-1001 BREG1003 registry'
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
            if ($family -eq 27 -and $name -in @('minecraft:seagrass', 'minecraft:kelp')) {
                $entries.Add([pscustomobject][ordered]@{
                    sequential_id = $sequentialId
                    family = 'Aquatic'
                    name = $name
                    canonical_state = $canonicalState
                })
            }
        }
        if ($reader.BaseStream.Position -ne $reader.BaseStream.Length) {
            throw 'BREG1003 registry has trailing bytes'
        }
    }
    finally {
        $reader.Dispose()
    }

    $seagrassCount = @($entries | Where-Object name -CEQ 'minecraft:seagrass').Count
    $kelpCount = @($entries | Where-Object name -CEQ 'minecraft:kelp').Count
    if ($entries.Count -ne 29 -or $seagrassCount -ne 3 -or $kelpCount -ne 26) {
        throw "aquatic registry coverage changed: total=$($entries.Count) seagrass=$seagrassCount kelp=$kelpCount"
    }

    $assetBytes = [IO.File]::ReadAllBytes($AssetsPath)
    if ($assetBytes.Length -lt 200 -or $utf8.GetString($assetBytes, 0, 8) -cne 'MCBEAS05' -or
        [BitConverter]::ToUInt32($assetBytes, 8) -ne 5) {
        throw 'aquatic coverage requires an MCBEAS05 compiled asset blob'
    }
    $visualCount = [BitConverter]::ToUInt32($assetBytes, 20)
    $visualOffset = [BitConverter]::ToUInt64($assetBytes, 96)
    if ($visualOffset -gt [uint64]$assetBytes.Length -or
        [uint64]$visualCount * 40 -gt [uint64]$assetBytes.Length - $visualOffset) {
        throw 'MCBEAS05 visual table is out of bounds'
    }
    $diagnosticCount = 0
    foreach ($entry in $entries) {
        if ([uint64]$entry.sequential_id -ge [uint64]$visualCount) {
            throw "registry sequential ID $($entry.sequential_id) is absent from the MCBEAS05 visual table"
        }
        $offset = [int]($visualOffset + 40 * [uint64]$entry.sequential_id)
        $expectedKind = if ($entry.name -ceq 'minecraft:seagrass') { 2 } else { 3 }
        if ($assetBytes[$offset + 25] -ne $expectedKind -or [BitConverter]::ToUInt32($assetBytes, $offset + 28) -eq [uint32]::MaxValue) {
            $diagnosticCount++
        }
    }
    if ($diagnosticCount -ne 0) {
        throw "seagrass/kelp compiled coverage contains diagnostic visuals: $diagnosticCount"
    }

    $stateSetHash = Get-CanonicalObjectHash -Value @($entries | ForEach-Object {
        [pscustomobject][ordered]@{
            sequential_id = $_.sequential_id
            family = $_.family
            name = $_.name
            canonical_state = $_.canonical_state
        }
    })
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-aquatic-coverage-v1'
        registry_protocol = 1001
        compiler_schema = 'MCBEAS05'
        registry_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $RegistryPath).Hash.ToLowerInvariant()
        assets_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $AssetsPath).Hash.ToLowerInvariant()
        state_set_sha256 = $stateSetHash
        state_count = $entries.Count
        seagrass_state_count = $seagrassCount
        kelp_state_count = $kelpCount
        diagnostic_seagrass_kelp = $diagnosticCount
        entries = @($entries)
    }
}

function New-AquaticGalleryPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('AquaticGalleryFront', 'AquaticGalleryBack')]
        [string]$Pose,
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $coverage = Get-AquaticCoverageEvidence -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $columns = 11
    $gridOrigin = @(-20, 2, -8)
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 24; y = $my + 1; z = $mz - 12 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 24; y = $my + 6; z = $mz + 12 }
    $shellMin = [pscustomobject][ordered]@{ x = $mx - 23; y = $my + 1; z = $mz - 11 }
    $shellMax = [pscustomobject][ordered]@{ x = $mx + 23; y = $my + 5; z = $mz + 11 }
    $interiorMin = [pscustomobject][ordered]@{ x = $mx - 22; y = $my + 1; z = $mz - 10 }
    $interiorMax = [pscustomobject][ordered]@{ x = $mx + 22; y = $my + 4; z = $mz + 10 }
    $clearVolume = ($clearMax.x - $clearMin.x + 1) * ($clearMax.y - $clearMin.y + 1) * ($clearMax.z - $clearMin.z + 1)
    $shellVolume = ($shellMax.x - $shellMin.x + 1) * ($shellMax.y - $shellMin.y + 1) * ($shellMax.z - $shellMin.z + 1)
    $supportVolume = ($interiorMax.x - $interiorMin.x + 1) * ($interiorMax.z - $interiorMin.z + 1)
    $waterVolume = ($interiorMax.x - $interiorMin.x + 1) * 3 * ($interiorMax.z - $interiorMin.z + 1)
    foreach ($volume in @($clearVolume, $shellVolume, $supportVolume, $waterVolume)) {
        if ($volume -gt 32768) {
            throw "aquatic gallery fill exceeds BDS limit: $volume"
        }
    }

    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 3; z = $mz }
    $camera = if ($Pose -ceq 'AquaticGalleryFront') {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 3; z = $mz - 20 }
    }
    else {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 3; z = $mz + 20 }
    }
    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $fixtureCommands.Add("fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air")
    $fixtureCommands.Add("fill $($shellMin.x) $($shellMin.y) $($shellMin.z) $($shellMax.x) $($shellMax.y) $($shellMax.z) minecraft:bedrock")
    $fixtureCommands.Add("fill $($interiorMin.x) $($my + 1) $($interiorMin.z) $($interiorMax.x) $($my + 1) $($interiorMax.z) minecraft:dirt")
    $fixtureCommands.Add("fill $($interiorMin.x) $($my + 2) $($interiorMin.z) $($interiorMax.x) $($my + 4) $($interiorMax.z) minecraft:water")
    $fixtureCommands.Add("fill $($interiorMin.x) $($my + 2) $($shellMin.z) $($interiorMax.x) $($my + 4) $($shellMin.z) minecraft:air")
    $fixtureCommands.Add("fill $($interiorMin.x) $($my + 2) $($shellMax.z) $($interiorMax.x) $($my + 4) $($shellMax.z) minecraft:air")

    $seagrassEntries = @($coverage.entries | Where-Object name -CEQ 'minecraft:seagrass')
    $kelpEntries = @($coverage.entries | Where-Object name -CEQ 'minecraft:kelp')
    $seagrassDefault = @($seagrassEntries | Where-Object canonical_state -NotMatch 'double_(top|bot)')[0]
    $seagrassBottom = @($seagrassEntries | Where-Object canonical_state -Match 'double_bot')[0]
    $seagrassTop = @($seagrassEntries | Where-Object canonical_state -Match 'double_top')[0]
    if ($null -eq $seagrassDefault -or $null -eq $seagrassBottom -or $null -eq $seagrassTop) {
        throw 'aquatic gallery could not resolve the three canonical seagrass forms'
    }
    $statePlacements = [Collections.Generic.List[object]]::new()
    $stateSlot = 0
    foreach ($entry in @($seagrassDefault, $seagrassBottom, $seagrassTop)) {
        $slot = if ($entry -eq $seagrassTop) { 1 } elseif ($entry -eq $seagrassBottom) { 1 } else { 0 }
        $x = $mx + $gridOrigin[0] + 4 * ($slot % $columns)
        $z = $mz + $gridOrigin[2] + 4 * [Math]::Floor($slot / $columns)
        $y = if ($entry -eq $seagrassTop) { $my + 3 } else { $my + 2 }
        $fixtureCommands.Add("setblock $x $y $z $($entry.name)$(ConvertTo-BdsCanonicalStateSuffix -CanonicalState $entry.canonical_state)")
        $statePlacements.Add([pscustomobject][ordered]@{ sequential_id = $entry.sequential_id; x = $x; y = $y; z = $z })
    }
    $stateSlot = 2
    $isolatedHeads = [Collections.Generic.List[object]]::new()
    $headGrowthCaps = [Collections.Generic.List[object]]::new()
    foreach ($entry in $kelpEntries) {
        $x = $mx + $gridOrigin[0] + 4 * ($stateSlot % $columns)
        $z = $mz + $gridOrigin[2] + 4 * [Math]::Floor($stateSlot / $columns)
        $fixtureCommands.Add("setblock $x $($my + 2) $z $($entry.name)$(ConvertTo-BdsCanonicalStateSuffix -CanonicalState $entry.canonical_state)")
        $fixtureCommands.Add("setblock $x $($my + 3) $z minecraft:bedrock")
        $placement = [pscustomobject][ordered]@{ sequential_id = $entry.sequential_id; x = $x; y = $my + 2; z = $z }
        $statePlacements.Add($placement)
        $isolatedHeads.Add($placement)
        $headGrowthCaps.Add([pscustomobject][ordered]@{ x = $x; y = $my + 3; z = $z; block = 'minecraft:bedrock' })
        $stateSlot++
    }

    $bodyWitnesses = [Collections.Generic.List[object]]::new()
    $upperEntries = @($kelpEntries | Where-Object canonical_state -Match '"kelp_age".*"value":25')
    if ($upperEntries.Count -ne 1) {
        throw "aquatic gallery expected one canonical kelp_age=25 growth-capped tip, found $($upperEntries.Count)"
    }
    $upperEntry = $upperEntries[0]
    foreach ($entry in $kelpEntries) {
        $x = $mx + $gridOrigin[0] + 4 * ($stateSlot % $columns)
        $z = $mz + $gridOrigin[2] + 4 * [Math]::Floor($stateSlot / $columns)
        $fixtureCommands.Add("setblock $x $($my + 2) $z $($entry.name)$(ConvertTo-BdsCanonicalStateSuffix -CanonicalState $entry.canonical_state)")
        $fixtureCommands.Add("setblock $x $($my + 3) $z $($upperEntry.name)$(ConvertTo-BdsCanonicalStateSuffix -CanonicalState $upperEntry.canonical_state)")
        $bodyWitnesses.Add([pscustomobject][ordered]@{
            sequential_id = $entry.sequential_id
            lower = [pscustomobject][ordered]@{ x = $x; y = $my + 2; z = $z }
            upper = [pscustomobject][ordered]@{ x = $x; y = $my + 3; z = $z; sequential_id = $upperEntry.sequential_id }
        })
        $stateSlot++
    }
    if ($fixtureCommands.Count -ne 113) {
        throw "aquatic gallery command count changed: $($fixtureCommands.Count)"
    }

    $fenceMarker = 'players online:'
    $fenceCommand = 'list'
    $teleportCommand = "tp @a[name=RustMCBE] $($camera.x) $($camera.y) $($camera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $relativeLayout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-aquatic-layout-v1'
        state_set_sha256 = $coverage.state_set_sha256
        gallery_state_count = $coverage.state_count
        clear_min = @(-24, 1, -12)
        clear_max = @(24, 6, 12)
        shell_min = @(-23, 1, -11)
        shell_max = @(23, 5, 11)
        shell_block = 'minecraft:bedrock'
        support_min = @(-22, 1, -10)
        support_max = @(22, 1, 10)
        support_block = 'minecraft:dirt'
        water_min = @(-22, 2, -10)
        water_max = @(22, 4, 10)
        water_block = 'minecraft:water'
        open_front = @(-22, 2, -11, 22, 4, -11)
        open_back = @(-22, 2, 11, 22, 4, 11)
        grid_origin = $gridOrigin
        columns = $columns
        spacing = @(4, 4)
        isolated_head_count = $isolatedHeads.Count
        isolated_head_cap_block = 'minecraft:bedrock'
        body_witness_count = $bodyWitnesses.Count
    }
    $layoutHash = Get-CanonicalObjectHash -Value $relativeLayout
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v1'
        fixture_kind = 'AquaticGallery'
        pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        fixture_layout_hash = $layoutHash
        relative_layout = $relativeLayout
        gallery_center = $galleryCenter
        camera = [pscustomobject][ordered]@{ position = $camera; target = $galleryCenter }
        gallery_state_count = $coverage.state_count
        gallery_states = @($coverage.entries | ForEach-Object { "$($_.name)|$($_.canonical_state)" })
        state_placements = @($statePlacements)
        isolated_kelp_heads = @($isolatedHeads)
        head_growth_caps = @($headGrowthCaps)
        body_witnesses = @($bodyWitnesses)
        family_diagnostics = [pscustomobject][ordered]@{ seagrass_kelp = $coverage.diagnostic_seagrass_kelp }
        coverage_evidence = [pscustomobject][ordered]@{
            schema = $coverage.schema
            state_set_sha256 = $coverage.state_set_sha256
            state_count = $coverage.state_count
            seagrass_state_count = $coverage.seagrass_state_count
            kelp_state_count = $coverage.kelp_state_count
            registry_sha256 = $coverage.registry_sha256
            assets_sha256 = $coverage.assets_sha256
        }
        artifact_identity = [pscustomobject][ordered]@{
            assets_sha256 = $coverage.assets_sha256
            registry_sha256 = $coverage.registry_sha256
            registry_protocol = $coverage.registry_protocol
            compiler_schema = $coverage.compiler_schema
        }
        fixture_commands = @($fixtureCommands)
        processing_fence = [pscustomobject][ordered]@{ command = $fenceCommand; stdout_marker = $fenceMarker }
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
        Commands = @($fixtureCommands) + @($fenceCommand, $teleportCommand)
        Manifest = $manifest
        CoverageEntries = @($coverage.entries)
    }
}
