function Get-CrossCropCoverageEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $reader = [IO.BinaryReader]::new([IO.MemoryStream]::new($registryBytes, $false))
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try {
        if ($utf8.GetString($reader.ReadBytes(8)) -cne 'BREG1003' -or $reader.ReadUInt32() -ne 1001) {
            throw 'cross/crop coverage requires the protocol-1001 BREG1003 registry'
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
            $null = $reader.ReadByte()
            $null = $reader.ReadByte()
            $null = $reader.ReadByte()
            $null = $reader.ReadByte()
            $null = $reader.ReadByte()
            $boxCount = [int]$reader.ReadByte()
            $null = $reader.ReadUInt16()
            $nameLength = [int]$reader.ReadUInt16()
            $stateLength = [int]$reader.ReadUInt32()
            $null = $reader.ReadBytes(32)
            $null = $reader.ReadBytes(24 * $boxCount)
            $name = $utf8.GetString($reader.ReadBytes($nameLength))
            $canonicalState = $utf8.GetString($reader.ReadBytes($stateLength))
            if ($family -in @(4, 5)) {
                $entries.Add([pscustomobject][ordered]@{
                    sequential_id = $sequentialId
                    family = if ($family -eq 4) { 'Cross' } else { 'Crop' }
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

    $assetBytes = [IO.File]::ReadAllBytes($AssetsPath)
    if ($assetBytes.Length -lt 200 -or $utf8.GetString($assetBytes, 0, 8) -cne 'MCBEAS05' -or
        [BitConverter]::ToUInt32($assetBytes, 8) -ne 5) {
        throw 'cross/crop coverage requires an MCBEAS05 compiled asset blob'
    }
    $visualCount = [BitConverter]::ToUInt32($assetBytes, 20)
    $visualOffset = [BitConverter]::ToUInt64($assetBytes, 96)
    if ($visualOffset -gt [uint64]$assetBytes.Length -or
        [uint64]$visualCount * 40 -gt [uint64]$assetBytes.Length - $visualOffset) {
        throw 'MCBEAS05 visual table is out of bounds'
    }
    $diagnosticCross = 0
    $diagnosticCrop = 0
    foreach ($entry in $entries) {
        if ([uint64]$entry.sequential_id -ge [uint64]$visualCount) {
            throw "registry sequential ID $($entry.sequential_id) is absent from the MCBEAS05 visual table"
        }
        $offset = [int]($visualOffset + 40 * [uint64]$entry.sequential_id)
        $isDiagnostic = $assetBytes[$offset + 25] -ne 2 -or [BitConverter]::ToUInt32($assetBytes, $offset + 28) -eq [uint32]::MaxValue
        if ($isDiagnostic) {
            if ($entry.family -ceq 'Cross') { $diagnosticCross++ } else { $diagnosticCrop++ }
        }
    }
    if ($diagnosticCross -ne 0 -or $diagnosticCrop -ne 0) {
        throw "cross/crop compiled coverage contains diagnostic visuals: cross=$diagnosticCross crop=$diagnosticCrop"
    }
    $crossCount = @($entries | Where-Object family -CEQ 'Cross').Count
    $cropCount = @($entries | Where-Object family -CEQ 'Crop').Count
    $stateSetHash = Get-CanonicalObjectHash -Value @($entries | ForEach-Object {
        [pscustomobject][ordered]@{
            sequential_id = $_.sequential_id
            family = $_.family
            name = $_.name
            canonical_state = $_.canonical_state
        }
    })
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-cross-crop-coverage-v1'
        registry_protocol = 1001
        compiler_schema = 'MCBEAS05'
        registry_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $RegistryPath).Hash.ToLowerInvariant()
        assets_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $AssetsPath).Hash.ToLowerInvariant()
        state_set_sha256 = $stateSetHash
        state_count = $entries.Count
        cross_state_count = $crossCount
        crop_state_count = $cropCount
        diagnostic_cross = $diagnosticCross
        diagnostic_crop = $diagnosticCrop
        entries = @($entries)
    }
}

function New-CrossCropGalleryPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('CrossCropGalleryFront', 'CrossCropGalleryBack')]
        [string]$Pose,
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$AssetsPath
    )

    $coverage = Get-CrossCropCoverageEvidence -RegistryPath $RegistryPath -AssetsPath $AssetsPath
    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $columns = 24
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 24; y = $my + 1; z = $mz - 20 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 24; y = $my + 5; z = $mz + 20 }
    $clearVolume = ($clearMax.x - $clearMin.x + 1) *
        ($clearMax.y - $clearMin.y + 1) *
        ($clearMax.z - $clearMin.z + 1)
    if ($clearVolume -gt 32768) {
        throw "cross/crop gallery clear volume exceeds BDS fill limit: $clearVolume"
    }
    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 3; z = $mz }
    $camera = if ($Pose -ceq 'CrossCropGalleryFront') {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 10; z = $mz - 70 }
    }
    else {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 9; z = $mz + 70 }
    }

    $fixtureCommands = [Collections.Generic.List[string]]::new()
    $fixtureCommands.Add("fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air")
    $fixtureCommands.Add("fill $($clearMin.x) $($my + 1) $($clearMin.z) $($clearMax.x) $($my + 1) $($clearMax.z) minecraft:farmland")
    for ($index = 0; $index -lt $coverage.entries.Count; $index++) {
        $x = $mx - 23 + 2 * ($index % $columns)
        $z = $mz - 18 + 2 * [Math]::Floor($index / $columns)
        $entry = $coverage.entries[$index]
        $stateSuffix = ConvertTo-BdsCanonicalStateSuffix -CanonicalState $entry.canonical_state
        $fixtureCommands.Add("setblock $x $($my + 2) $z $($entry.name)$stateSuffix")
    }
    $fenceMarker = 'players online:'
    $fenceCommand = 'list'
    $teleportCommand = "tp @a[name=RustMCBE] $($camera.x) $($camera.y) $($camera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $commands = @($fixtureCommands) + @($fenceCommand, $teleportCommand)
    if ($commands.Count -gt 512) {
        throw "cross/crop gallery command list is not bounded: $($commands.Count)"
    }
    $relativeLayout = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-cross-crop-layout-v1'
        state_set_sha256 = $coverage.state_set_sha256
        gallery_state_count = $coverage.state_count
        clear_min = @(-24, 1, -20)
        clear_max = @(24, 5, 20)
        support_min = @(-24, 1, -20)
        support_max = @(24, 1, 20)
        support_block = 'minecraft:farmland'
        grid_origin = @(-23, 2, -18)
        columns = $columns
        spacing = @(2, 2)
    }
    $layoutHash = Get-CanonicalObjectHash -Value $relativeLayout
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v1'
        fixture_kind = 'CrossCropGallery'
        pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        fixture_layout_hash = $layoutHash
        relative_layout = $relativeLayout
        gallery_center = $galleryCenter
        camera = [pscustomobject][ordered]@{ position = $camera; target = $galleryCenter }
        gallery_state_count = $coverage.state_count
        gallery_states = @($coverage.entries | ForEach-Object { "$($_.name)|$($_.canonical_state)" })
        family_diagnostics = [pscustomobject][ordered]@{ cross = $coverage.diagnostic_cross; crop = $coverage.diagnostic_crop }
        coverage_evidence = [pscustomobject][ordered]@{
            schema = $coverage.schema
            state_set_sha256 = $coverage.state_set_sha256
            state_count = $coverage.state_count
            cross_state_count = $coverage.cross_state_count
            crop_state_count = $coverage.crop_state_count
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
        Commands = $commands
        Manifest = $manifest
        CoverageEntries = @($coverage.entries)
    }
}
