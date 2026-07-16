    $crossCropPlan = New-CrossCropGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose CrossCropGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $CrossCropAssets
    $aquaticPlan = New-AquaticGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose AquaticGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $AquaticAssets
    $waterPlan = New-WaterGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose WaterGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $AquaticAssets
    $flowerBedPlans = @('FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite') | ForEach-Object {
        New-FlowerBedGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose $_ -RegistryPath $BlockRegistry
    }
    # slab_stair_gallery_covers_all_variants
    $slabStairPlans = @('SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite') | ForEach-Object {
        New-SlabStairGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose $_ -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    }
    # vine_gallery_covers_all_direction_masks
    $vinePlans = @('VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite') | ForEach-Object {
        New-VineGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose $_ -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    }
    foreach ($vinePlan in $vinePlans) {
        Assert-Equal 'rust-mcbe-visual-fixture-v2' $vinePlan.Manifest.schema 'vine gallery lost schema-v2 fixture identity'
        Assert-Equal 'VineGallery' $vinePlan.Manifest.fixture_kind 'vine plan lost fixture kind'
        Assert-Equal 16 ([int]$vinePlan.Manifest.central_witness_count) 'vine plan lost the exact 16 central witnesses'
        Assert-Equal 16 @($vinePlan.Manifest.witnesses).Count 'vine witness inventory changed'
        Assert-Equal '0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15' (@($vinePlan.Manifest.witnesses | ForEach-Object mask) -join ',') 'vine masks are not the exact ordered bijection 0..15'
        Assert-Equal 5 @($vinePlan.Manifest.camera_poses.PSObject.Properties).Count 'vine plan lost a fixed diagnostic camera'
        Assert-Equal 50 @($vinePlan.FixtureCommands).Count 'vine fixture command bound changed'
        Assert-Equal 16 ([int]$vinePlan.Manifest.coverage_evidence.state_count) 'vine coverage lost exact BREG state count'
        Assert-Equal 0 ([int]$vinePlan.Manifest.coverage_evidence.diagnostic_vine) 'vine compiled coverage retained diagnostics'
        Assert-True (-not (($vinePlan.Commands -join "`n").Contains('`'))) "$($vinePlan.Manifest.pose) published a literal PowerShell escape in a BDS command"
        Assert-Equal (Get-CanonicalObjectHash -Value $vinePlan.Manifest.relative_layout) $vinePlan.Manifest.fixture_layout_hash 'vine layout hash was not derived from its complete relative layout'

        $centerIdentities = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        $supportIdentities = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
        foreach ($witness in @($vinePlan.Manifest.witnesses)) {
            $mask = [int]$witness.mask
            $expectedSupportDirections = @(
                if (($mask -band 1) -ne 0) { 'south' }
                if (($mask -band 2) -ne 0) { 'west' }
                if (($mask -band 4) -ne 0) { 'north' }
                if (($mask -band 8) -ne 0) { 'east' }
            )
            Assert-Equal ($expectedSupportDirections -join ',') (@($witness.supports | ForEach-Object direction) -join ',') "vine mask $mask changed exact horizontal support semantics"
            Assert-Equal $expectedSupportDirections.Count @($witness.supports).Count "vine mask $mask changed support popcount"
            $center = @($witness.center_offset)
            $centerIdentity = "$($center[0]),$($center[1]),$($center[2])"
            Assert-True ($centerIdentities.Add($centerIdentity)) "vine mask $mask reused another gallery center"
            foreach ($support in @($witness.supports)) {
                $offset = @($support.offset)
                $expectedDelta = switch -CaseSensitive ([string]$support.direction) {
                    'south' { @(0, 0, 1) }
                    'west' { @(-1, 0, 0) }
                    'north' { @(0, 0, -1) }
                    'east' { @(1, 0, 0) }
                    default { throw "vine mask $mask declared unknown support direction: $($support.direction)" }
                }
                $expectedOffset = @(
                    ([int]$center[0] + [int]$expectedDelta[0])
                    ([int]$center[1] + [int]$expectedDelta[1])
                    ([int]$center[2] + [int]$expectedDelta[2])
                )
                Assert-Equal ($expectedOffset -join ',') ($offset -join ',') "vine mask $mask $($support.direction) support moved away from its exact direction delta"
                $supportIdentity = "$($offset[0]),$($offset[1]),$($offset[2])"
                Assert-True ($supportIdentities.Add($supportIdentity)) "vine mask $mask reused another cell's support"
                Assert-True (-not $centerIdentities.Contains($supportIdentity)) "vine mask $mask support overlapped a gallery center"
                $supportCommand = "setblock $([int]$vinePlan.Manifest.mutation.x + [int]$offset[0]) $([int]$vinePlan.Manifest.mutation.y + [int]$offset[1]) $([int]$vinePlan.Manifest.mutation.z + [int]$offset[2]) minecraft:stone"
                Assert-Equal 1 @($vinePlan.FixtureCommands | Where-Object { $_ -ceq $supportCommand }).Count "vine mask $mask manifest support did not map to exactly one BDS command"
            }
            $expectedStateCommandSuffix = " minecraft:vine [`"vine_direction_bits`"=$mask]"
            Assert-Equal 1 @($vinePlan.FixtureCommands | Where-Object { $_.EndsWith($expectedStateCommandSuffix) }).Count "vine mask $mask lost its exact protocol-1001 state command"
        }
        Assert-Equal 0 @($vinePlan.Manifest.witnesses | Where-Object mask -eq 0 | ForEach-Object supports).Count 'vine mask 0 gained diagnostic/support geometry'
        Assert-Equal 32 $supportIdentities.Count 'vine gallery did not create the exact 32 isolated horizontal supports'
        Assert-Equal 32 @($vinePlan.FixtureCommands | Where-Object { $_ -cmatch '^setblock -?\d+ -?\d+ -?\d+ minecraft:stone$' }).Count 'vine gallery emitted extra or missing solid horizontal support commands'
        foreach ($centerIdentity in $centerIdentities) {
            Assert-True (-not $supportIdentities.Contains($centerIdentity)) "vine support overlapped center $centerIdentity"
        }
    }
    Assert-Equal 1 @($vinePlans | ForEach-Object { $_.Manifest.fixture_layout_hash } | Sort-Object -Unique).Count 'vine camera pose changed canonical layout identity'
    Assert-Equal 1 @($vinePlans | ForEach-Object { $_.Manifest.state_set_sha256 } | Sort-Object -Unique).Count 'vine camera pose changed exact state identity'
    $movedVinePlan = New-VineGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose VineGalleryTop -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    Assert-Equal $vinePlans[0].Manifest.fixture_layout_hash $movedVinePlan.Manifest.fixture_layout_hash 'vine absolute coordinate changed canonical layout identity'
    Assert-Equal $vinePlans[0].Manifest.state_set_sha256 $movedVinePlan.Manifest.state_set_sha256 'vine absolute coordinate changed state identity'
    Assert-Equal ($vinePlans[0].Manifest.relative_layout | ConvertTo-Json -Compress -Depth 12) ($movedVinePlan.Manifest.relative_layout | ConvertTo-Json -Compress -Depth 12) 'vine translated layout changed its relative fixture document'
    Assert-Equal '400,6,-500' (@(
        ([int]$movedVinePlan.Manifest.mutation.x - [int]$vinePlans[0].Manifest.mutation.x)
        ([int]$movedVinePlan.Manifest.mutation.y - [int]$vinePlans[0].Manifest.mutation.y)
        ([int]$movedVinePlan.Manifest.mutation.z - [int]$vinePlans[0].Manifest.mutation.z)
    ) -join ',') 'vine translated layout used the wrong deterministic displacement'
    foreach ($slabStairPlan in $slabStairPlans) {
        Assert-True (-not (($slabStairPlan.Commands -join "`n").Contains('`'))) "$($slabStairPlan.Manifest.pose) published a literal PowerShell escape in a BDS command"
        $slabCommands = @($slabStairPlan.FixtureCommands | Where-Object { $_ -match '^setblock .*slab' })
        Assert-Equal 3 $slabCommands.Count "$($slabStairPlan.Manifest.pose) changed the exact slab witness command count"
        Assert-Equal 1 @($slabCommands | Where-Object { $_.EndsWith(' minecraft:smooth_stone_slab ["minecraft:vertical_half"="bottom"]') }).Count "$($slabStairPlan.Manifest.pose) omitted the exact protocol-1001 bottom smooth-stone slab"
        Assert-Equal 1 @($slabCommands | Where-Object { $_.EndsWith(' minecraft:smooth_stone_slab ["minecraft:vertical_half"="top"]') }).Count "$($slabStairPlan.Manifest.pose) omitted the exact protocol-1001 top smooth-stone slab"
        Assert-Equal 1 @($slabCommands | Where-Object { $_.EndsWith(' minecraft:smooth_stone_double_slab') }).Count "$($slabStairPlan.Manifest.pose) omitted the exact protocol-1001 double smooth-stone slab"
    }
    $unsealedSlabStairAssets = Join-Path $TempRoot 'unsealed covered visual mutation.mcbea'
    $unsealedBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
    $firstStairId = [int](@(Get-TestRegistryEntries -RegistryPath $BlockRegistry | Where-Object family -eq 8)[0].sequential_id)
    $coveredVisualOffset = [int]([BitConverter]::ToUInt64($unsealedBytes, 96) + 40 * $firstStairId)
    $unsealedBytes[$coveredVisualOffset] = $unsealedBytes[$coveredVisualOffset] -bxor 1
    [IO.File]::WriteAllBytes($unsealedSlabStairAssets, $unsealedBytes)
    Assert-ThrowsLike {
        Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $unsealedSlabStairAssets
    } 'MCBEAS05 slab/stair integrity SHA-256 mismatch*' 'slab/stair coverage accepted an unsealed covered-visual mutation'
    $oversizedSlabStairAssets = Join-Path $TempRoot 'oversized slab stair assets.mcbea'
    $oversizedStream = [IO.File]::Create($oversizedSlabStairAssets)
    try { $oversizedStream.SetLength(16 * 1024 * 1024 + 1) }
    finally { $oversizedStream.Dispose() }
    Assert-ThrowsLike {
        Get-StrictMcbeas05ModelTables -Path $oversizedSlabStairAssets
    } 'MCBEAS05 blob exceeds the app 16 MiB ceiling:*' 'slab/stair validation allocated an oversized blob before rejecting it'
    $emptyWallMaskAssets = Join-Path $TempRoot 'canonical empty wall mask.mcbea'
    $emptyWallMaskBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
    $emptyWallTemplateOffset = [int][BitConverter]::ToUInt64($emptyWallMaskBytes, 120) + 12 * 12
    Assert-Equal 0 ([int][BitConverter]::ToUInt32($emptyWallMaskBytes, $emptyWallTemplateOffset + 4)) 'empty wall-mask fixture did not start from a zero-quad template'
    [BitConverter]::GetBytes([uint32]64).CopyTo($emptyWallMaskBytes, $emptyWallTemplateOffset + 8)
    Set-TestMcbeas05Seal -Bytes $emptyWallMaskBytes
    [IO.File]::WriteAllBytes($emptyWallMaskAssets, $emptyWallMaskBytes)
    $emptyWallMaskTables = Get-StrictMcbeas05ModelTables -Path $emptyWallMaskAssets
    Assert-Equal 0 ([int]$emptyWallMaskTables.templates[12].quad_count) 'strict model-table validation changed the canonical empty wall-mask span'
    Assert-Equal 64 ([int]$emptyWallMaskTables.templates[12].flags) 'strict model-table validation changed the canonical wall flag'
    $strictTemplateOffset = [int][BitConverter]::ToUInt64($emptyWallMaskBytes, 120)
    foreach ($invalidTemplateFlag in @(
        [pscustomobject]@{ name = 'standalone gate axis x'; template = 12; flags = 128; quad_count = 0 },
        [pscustomobject]@{ name = 'standalone gate axis z'; template = 12; flags = 256; quad_count = 0 },
        [pscustomobject]@{ name = 'combined kelp and stair'; template = 12; flags = 3; quad_count = 0 },
        [pscustomobject]@{ name = 'short transparent cube'; template = 0; flags = 512; quad_count = 1 }
    )) {
        $invalidTemplateFlagAssets = Join-Path $TempRoot ("resealed $($invalidTemplateFlag.name).mcbea")
        $invalidTemplateFlagBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
        $invalidTemplateDescriptor = $strictTemplateOffset + 12 * [int]$invalidTemplateFlag.template
        Assert-Equal ([int]$invalidTemplateFlag.quad_count) ([int][BitConverter]::ToUInt32($invalidTemplateFlagBytes, $invalidTemplateDescriptor + 4)) "$($invalidTemplateFlag.name) fixture started from the wrong quad count"
        [BitConverter]::GetBytes([uint32]$invalidTemplateFlag.flags).CopyTo($invalidTemplateFlagBytes, $invalidTemplateDescriptor + 8)
        Set-TestMcbeas05Seal -Bytes $invalidTemplateFlagBytes
        [IO.File]::WriteAllBytes($invalidTemplateFlagAssets, $invalidTemplateFlagBytes)
        Assert-ThrowsLike {
            Get-StrictMcbeas05ModelTables -Path $invalidTemplateFlagAssets
        } "MCBEAS05 model template $($invalidTemplateFlag.template) span or flags are noncanonical" "strict model-table validation accepted $($invalidTemplateFlag.name) flags"
    }
    $nonzeroLightAssets = Join-Path $TempRoot 'nonzero packed light metadata.mcbea'
    $nonzeroLightBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
    $nonzeroLightBytes[$coveredVisualOffset + 27] = 0xaf
    Set-TestMcbeas05Seal -Bytes $nonzeroLightBytes
    [IO.File]::WriteAllBytes($nonzeroLightAssets, $nonzeroLightBytes)
    $nonzeroLightTables = Get-StrictMcbeas05ModelTables -Path $nonzeroLightAssets
    Assert-Equal 0xaf ([int]$nonzeroLightTables.bytes[$coveredVisualOffset + 27]) 'strict model-table validation rejected or changed packed light metadata'
    $kelpBackedSlabAssets = Join-Path $TempRoot 'resealed kelp backed slab.mcbea'
    $kelpBackedBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
    $firstSlabId = [int](@(Get-TestRegistryEntries -RegistryPath $BlockRegistry | Where-Object family -eq 7)[0].sequential_id)
    $firstSlabVisual = [int]([BitConverter]::ToUInt64($kelpBackedBytes, 96) + 40 * $firstSlabId)
    [BitConverter]::GetBytes([uint32]11).CopyTo($kelpBackedBytes, $firstSlabVisual + 28)
    Set-TestMcbeas05Seal -Bytes $kelpBackedBytes
    [IO.File]::WriteAllBytes($kelpBackedSlabAssets, $kelpBackedBytes)
    Assert-ThrowsLike {
        Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $kelpBackedSlabAssets
    } 'slab/stair compiled coverage contains diagnostic or malformed visuals:*' 'slab/stair coverage accepted a canonical kelp template for a slab visual'
    $templateOffset = [int][BitConverter]::ToUInt64([IO.File]::ReadAllBytes($SlabStairAssets), 120)
    $quadOffset = [int][BitConverter]::ToUInt64([IO.File]::ReadAllBytes($SlabStairAssets), 128)
    foreach ($malformedTemplate in @(
        [pscustomobject]@{ name = 'resealed 33-quad template'; pattern = 'MCBEAS05 model template 0 span or flags are noncanonical*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]33).CopyTo($bytes, $templateOffset + 4) } },
        [pscustomobject]@{ name = 'resealed out-of-range template span'; pattern = 'MCBEAS05 model template 10 span or flags are noncanonical*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]11).CopyTo($bytes, $templateOffset + 120) } },
        [pscustomobject]@{ name = 'resealed middle-of-stair-group visual'; pattern = 'MCBEAS05 stair visual * does not reference an exact group base or has reserved variant bits*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]2).CopyTo($bytes, $coveredVisualOffset + 28) } },
        [pscustomobject]@{ name = 'resealed malformed kelp sidedness'; pattern = 'MCBEAS05 kelp template 11 has noncanonical sidedness*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]0).CopyTo($bytes, $quadOffset + 48 * 16 + 44) } }
    )) {
        $malformedPath = Join-Path $TempRoot ($malformedTemplate.name + '.mcbea')
        $malformedBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
        & $malformedTemplate.mutate $malformedBytes
        Set-TestMcbeas05Seal -Bytes $malformedBytes
        [IO.File]::WriteAllBytes($malformedPath, $malformedBytes)
        Assert-ThrowsLike {
            Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $malformedPath
        } $malformedTemplate.pattern "slab/stair coverage accepted $($malformedTemplate.name)"
    }

    $vineCoverage = Get-VineCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    Assert-Equal 'rust-mcbe-vine-coverage-v1' $vineCoverage.schema 'vine coverage lost strict schema identity'
    Assert-Equal 1001 ([int]$vineCoverage.registry_protocol) 'vine coverage lost protocol binding'
    Assert-Equal 'MCBEAS05' $vineCoverage.compiler_schema 'vine coverage lost compiler binding'
    Assert-Equal 16 ([int]$vineCoverage.state_count) 'vine coverage did not contain exactly 16 states'
    Assert-Equal '0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15' (@($vineCoverage.entries | ForEach-Object mask) -join ',') 'vine coverage masks were not an exact bijection 0..15'
    Assert-Equal 0 ([int]$vineCoverage.diagnostic_vine) 'vine coverage retained diagnostic/malformed visuals'
    Assert-ThrowsLike {
        Get-VineCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath (Join-Path $TempRoot 'missing vine assets.mcbea')
    } '*' 'vine coverage accepted a missing assets blob'

    $vineEntries = @(Get-TestRegistryEntries -RegistryPath $BlockRegistry | Where-Object name -CEQ 'minecraft:vine')
    $vineVisualOffset = [int][BitConverter]::ToUInt64([IO.File]::ReadAllBytes($SlabStairAssets), 96)
    foreach ($malformedVineVisual in @(
        [pscustomobject]@{
            name = 'diagnostic visual kind'
            mutate = {
                param($bytes)
                $entry = @($vineEntries | Where-Object { [int](($_.canonical_state | ConvertFrom-Json).vine_direction_bits.value) -eq 1 })[0]
                $bytes[$vineVisualOffset + 40 * [int]$entry.sequential_id + 25] = 0
            }
        },
        [pscustomobject]@{
            name = 'mask zero nonzero-quad template'
            mutate = {
                param($bytes)
                $entry = @($vineEntries | Where-Object { [int](($_.canonical_state | ConvertFrom-Json).vine_direction_bits.value) -eq 0 })[0]
                [BitConverter]::GetBytes([uint32]13).CopyTo($bytes, $vineVisualOffset + 40 * [int]$entry.sequential_id + 28)
            }
        },
        [pscustomobject]@{
            name = 'mask three wrong-popcount template'
            mutate = {
                param($bytes)
                $entry = @($vineEntries | Where-Object { [int](($_.canonical_state | ConvertFrom-Json).vine_direction_bits.value) -eq 3 })[0]
                [BitConverter]::GetBytes([uint32]13).CopyTo($bytes, $vineVisualOffset + 40 * [int]$entry.sequential_id + 28)
            }
        }
    )) {
        $malformedVinePath = Join-Path $TempRoot "$($malformedVineVisual.name).mcbea"
        $malformedVineBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
        & $malformedVineVisual.mutate $malformedVineBytes
        Set-TestMcbeas05Seal -Bytes $malformedVineBytes
        [IO.File]::WriteAllBytes($malformedVinePath, $malformedVineBytes)
        Assert-ThrowsLike {
            Get-VineCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $malformedVinePath
        } 'vine compiled coverage contains diagnostic or malformed visuals:*' "vine coverage accepted $($malformedVineVisual.name)"
    }

    $expectedFlowerBedStates = [Collections.Generic.List[string]]::new()
    $expectedFlowerBedReferences = [Collections.Generic.List[string]]::new()
    $expectedFlowerBedStateCommands = [Collections.Generic.List[string]]::new()
    foreach ($name in @('minecraft:pink_petals', 'minecraft:wildflowers')) {
        foreach ($direction in @('south', 'west', 'north', 'east')) {
            foreach ($growth in 0..7) {
                $canonical = '{"growth":{"type":"int","value":' + $growth + '},"minecraft:cardinal_direction":{"type":"string","value":"' + $direction + '"}}'
                $stateIdentity = "$name|$canonical"
                $index = $expectedFlowerBedStates.Count
                $xOffset = -14 + 4 * ($index % 8)
                $zOffset = -10 + 3 * [Math]::Floor($index / 8)
                $expectedFlowerBedStates.Add($stateIdentity)
                $expectedFlowerBedReferences.Add("$stateIdentity|state=$xOffset,2,$zOffset|cube=$($xOffset + 1),2,$zOffset|minecraft:polished_andesite")
                $expectedFlowerBedStateCommands.Add("setblock $($xOffset + 100) 66 $($zOffset + 200) $name [`"growth`"=$growth,`"minecraft:cardinal_direction`"=`"$direction`"]")
            }
        }
    }
    Assert-Equal 64 $expectedFlowerBedStates.Count 'pinned flowerbed test matrix changed'
    $expectedFirstFlowerBedState = 'minecraft:pink_petals|{"growth":{"type":"int","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"}}'
    $expectedLastFlowerBedState = 'minecraft:wildflowers|{"growth":{"type":"int","value":7},"minecraft:cardinal_direction":{"type":"string","value":"east"}}'

    foreach ($flowerBedPlan in $flowerBedPlans) {
        Assert-Equal 'FlowerBedGallery' $flowerBedPlan.Manifest.fixture_kind 'flowerbed plan lost fixture kind'
        Assert-Equal 64 ([int]$flowerBedPlan.Manifest.gallery_state_count) 'flowerbed plan did not enumerate exactly 64 states'
        Assert-Equal 64 @($flowerBedPlan.Manifest.gallery_states | Sort-Object -Unique).Count 'flowerbed manifest states were not unique'
        Assert-Equal 64 @($flowerBedPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:(wildflowers|pink_petals) ' }).Count 'flowerbed plan did not issue exactly one placement per canonical state'
        Assert-Equal 64 @($flowerBedPlan.Manifest.reference_cubes).Count 'flowerbed plan did not pair every state with a reference cube'
        Assert-Equal 5 @($flowerBedPlan.Manifest.camera_poses.PSObject.Properties).Count 'flowerbed plan lost a fixed diagnostic camera'
        Assert-Equal 'a2fe82092cb22835a0553091ecfcdd67cedcddc9e791feb2d0ddeff9fe091f15' ([string]$flowerBedPlan.Manifest.coverage_evidence.state_set_sha256) 'flowerbed exact ordered BREG state-set identity drifted'
        Assert-Equal 'e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763' ([string]$flowerBedPlan.Manifest.fixture_layout_hash) 'flowerbed canonical layout identity drifted'
        Assert-Equal $expectedFirstFlowerBedState ([string]$flowerBedPlan.Manifest.gallery_states[0]) 'flowerbed first canonical identity drifted'
        Assert-Equal $expectedLastFlowerBedState ([string]$flowerBedPlan.Manifest.gallery_states[-1]) 'flowerbed last canonical identity drifted'
        Assert-Equal ($expectedFlowerBedStates -join "`n") (@($flowerBedPlan.Manifest.gallery_states) -join "`n") 'flowerbed exact ordered 64-state manifest drifted'
        $actualReferences = @($flowerBedPlan.Manifest.reference_cubes | ForEach-Object {
            "$($_.state)|state=$($_.state_offset -join ',')|cube=$($_.cube_offset -join ',')|$($_.cube)"
        })
        Assert-Equal ($expectedFlowerBedReferences -join "`n") ($actualReferences -join "`n") 'flowerbed state-to-grid/reference-cube identity drifted'
        $actualStateCommands = @($flowerBedPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:(wildflowers|pink_petals) ' })
        Assert-Equal ($expectedFlowerBedStateCommands -join "`n") ($actualStateCommands -join "`n") 'flowerbed typed state-to-world-coordinate command identity drifted'
        Assert-True ($flowerBedPlan.LoadAreaCommand -match '^tickingarea add ') 'flowerbed plan did not preload its bounded gallery'
        Assert-True ($flowerBedPlan.CleanupCommand -match '^tickingarea remove ') 'flowerbed plan did not provide ticking-area cleanup'
        Assert-Equal (Get-CanonicalObjectHash -Value $flowerBedPlan.Manifest.relative_layout) $flowerBedPlan.Manifest.fixture_layout_hash 'flowerbed layout hash was not derived from the complete relative layout'

        $growths = @($flowerBedPlan.CoverageEntries | ForEach-Object growth | Sort-Object -Unique)
        $directions = @($flowerBedPlan.CoverageEntries | ForEach-Object direction | Sort-Object -Unique)
        $names = @($flowerBedPlan.CoverageEntries | ForEach-Object name | Sort-Object -Unique)
        Assert-Equal '0,1,2,3,4,5,6,7' ($growths -join ',') 'flowerbed gallery growth coverage changed'
        Assert-Equal 'east,north,south,west' ($directions -join ',') 'flowerbed gallery cardinal coverage changed'
        Assert-Equal 'minecraft:pink_petals,minecraft:wildflowers' ($names -join ',') 'flowerbed gallery name coverage changed'
    }
    $oppositeFlowerBedPlan = @($flowerBedPlans | Where-Object { $_.Manifest.pose -ceq 'FlowerBedGalleryObliqueOpposite' })[0]
    Assert-Equal '38,28,38' (@(
        ([int]$oppositeFlowerBedPlan.Manifest.camera.position.x - [int]$oppositeFlowerBedPlan.Manifest.mutation.x)
        ([int]$oppositeFlowerBedPlan.Manifest.camera.position.y - [int]$oppositeFlowerBedPlan.Manifest.mutation.y)
        ([int]$oppositeFlowerBedPlan.Manifest.camera.position.z - [int]$oppositeFlowerBedPlan.Manifest.mutation.z)
    ) -join ',') 'flowerbed opposite oblique camera lost its symmetric offset'
    Assert-Equal '138,92,238' (@($oppositeFlowerBedPlan.Manifest.camera.position.x, $oppositeFlowerBedPlan.Manifest.camera.position.y, $oppositeFlowerBedPlan.Manifest.camera.position.z) -join ',') 'flowerbed opposite oblique camera lost its exact position'
    Assert-Equal 'tp @a[name=RustMCBE] 138 92 238 facing 100 66 200' ([string]$oppositeFlowerBedPlan.TeleportCommand) 'flowerbed opposite oblique camera command drifted'
    Assert-Equal 'e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763' ([string]$oppositeFlowerBedPlan.Manifest.fixture_layout_hash) 'flowerbed opposite oblique camera changed canonical layout identity'
    Assert-Equal 'a2fe82092cb22835a0553091ecfcdd67cedcddc9e791feb2d0ddeff9fe091f15' ([string]$oppositeFlowerBedPlan.Manifest.coverage_evidence.state_set_sha256) 'flowerbed opposite oblique camera changed exact state-set identity'
    Assert-Equal 1 @($flowerBedPlans | ForEach-Object { $_.Manifest.fixture_layout_hash } | Sort-Object -Unique).Count 'flowerbed camera pose changed canonical layout identity'
    $movedFlowerBedPlan = New-FlowerBedGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose FlowerBedGalleryTop -RegistryPath $BlockRegistry
    Assert-Equal $flowerBedPlans[0].Manifest.fixture_layout_hash $movedFlowerBedPlan.Manifest.fixture_layout_hash 'flowerbed absolute coordinate changed canonical layout identity'
    Assert-Equal ($flowerBedPlans[0].Manifest.gallery_states -join "`n") ($movedFlowerBedPlan.Manifest.gallery_states -join "`n") 'flowerbed BREG state manifest was not deterministic'
    $tamperedFlowerBedLayout = $flowerBedPlans[0].Manifest.relative_layout | ConvertTo-Json -Depth 12 | ConvertFrom-Json
    $tamperedFlowerBedLayout.spacing[0] = 5
    Assert-True ((Get-CanonicalObjectHash -Value $tamperedFlowerBedLayout) -cne 'e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763') 'flowerbed pinned layout hash did not detect spacing drift'
    $reorderedFlowerBedStates = @($expectedFlowerBedStates)
    [array]::Reverse($reorderedFlowerBedStates)
    Assert-True (($reorderedFlowerBedStates -join "`n") -cne ($flowerBedPlans[0].Manifest.gallery_states -join "`n")) 'flowerbed exact ordered manifest assertion cannot detect reordering'

    foreach ($slabStairPlan in $slabStairPlans) {
        Assert-Equal 'SlabStairGallery' $slabStairPlan.Manifest.fixture_kind 'slab/stair plan lost fixture kind'
        Assert-Equal 43 ([int]$slabStairPlan.Manifest.central_witness_count) 'slab/stair plan lost the 43 central witnesses'
        Assert-Equal 43 @($slabStairPlan.Manifest.witnesses).Count 'slab/stair witness inventory changed'
        Assert-Equal 3 @($slabStairPlan.Manifest.witnesses | Where-Object kind -ceq 'slab').Count 'slab variants changed'
        Assert-Equal 40 @($slabStairPlan.Manifest.witnesses | Where-Object kind -ceq 'stair').Count 'stair state matrix changed'
        Assert-Equal 5 @($slabStairPlan.Manifest.camera_poses.PSObject.Properties).Count 'slab/stair plan lost a fixed diagnostic camera'
        Assert-Equal 77 @($slabStairPlan.FixtureCommands).Count 'slab/stair fixture command bound changed'
        Assert-Equal 784 ([int]$slabStairPlan.Manifest.coverage_evidence.state_count) 'slab/stair coverage lost exact BREG state count'
        Assert-Equal 272 ([int]$slabStairPlan.Manifest.coverage_evidence.slab_state_count) 'slab coverage count drifted'
        Assert-Equal 512 ([int]$slabStairPlan.Manifest.coverage_evidence.stair_state_count) 'stair coverage count drifted'
        Assert-Equal 64 ([int]$slabStairPlan.Manifest.coverage_evidence.stair_name_count) 'stair identifier count drifted'
        Assert-Equal 0 ([int]$slabStairPlan.Manifest.coverage_evidence.diagnostic_slab_stair) 'slab/stair compiled coverage retained diagnostics'
        Assert-Equal '860f1e5629d7d6f390d554cedcef16546237f9f9df9f24a2abaa5a22c785fbc8' ([string]$slabStairPlan.Manifest.state_set_sha256) 'slab/stair exact state-set identity drifted'
        Assert-Equal '8c035c430d72ce4e62df32a99d126608e2b476bb155f941c89671500f91f4448' ([string]$slabStairPlan.Manifest.fixture_layout_hash) 'slab/stair canonical layout identity drifted'
        Assert-True ($slabStairPlan.LoadAreaCommand -match '^tickingarea add ') 'slab/stair gallery omitted bounded preload'
        Assert-True ($slabStairPlan.CleanupCommand -match '^tickingarea remove ') 'slab/stair gallery omitted ticking-area cleanup'
        Assert-Equal (Get-CanonicalObjectHash -Value $slabStairPlan.Manifest.relative_layout) $slabStairPlan.Manifest.fixture_layout_hash 'slab/stair layout hash was not derived from its complete relative layout'
        foreach ($half in @($false, $true)) {
            foreach ($orientation in @('south', 'west', 'north', 'east')) {
                foreach ($shape in @('straight', 'right_inner', 'left_inner', 'right_outer', 'left_outer')) {
                    Assert-Equal 1 @($slabStairPlan.Manifest.witnesses | Where-Object { $_.kind -ceq 'stair' -and $_.upside_down -eq $half -and $_.orientation -ceq $orientation -and $_.shape -ceq $shape }).Count "slab/stair matrix missing half=$half orientation=$orientation shape=$shape"
                }
            }
        }
        Assert-Equal 32 @($slabStairPlan.Manifest.witnesses | Where-Object { $_.kind -ceq 'stair' -and $null -ne $_.neighbor_offset }).Count 'corner witnesses lost isolated neighbours'
    }
    Assert-Equal 1 @($slabStairPlans | ForEach-Object { $_.Manifest.fixture_layout_hash } | Sort-Object -Unique).Count 'slab/stair camera pose changed canonical layout identity'
    Assert-Equal 1 @($slabStairPlans | ForEach-Object { $_.Manifest.state_set_sha256 } | Sort-Object -Unique).Count 'slab/stair camera pose changed exact state identity'
    $movedSlabStairPlan = New-SlabStairGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose SlabStairGalleryTop -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    Assert-Equal $slabStairPlans[0].Manifest.fixture_layout_hash $movedSlabStairPlan.Manifest.fixture_layout_hash 'slab/stair absolute coordinate changed canonical layout identity'
    Assert-Equal $slabStairPlans[0].Manifest.state_set_sha256 $movedSlabStairPlan.Manifest.state_set_sha256 'slab/stair absolute coordinate changed state identity'

    $freshSource = Join-Path $TempRoot 'fresh gallery source'
    $freshRuntime = Join-Path $TempRoot 'fresh gallery runtime'
    New-Item -ItemType Directory -Path $freshSource, (Join-Path $freshRuntime 'worlds\Bedrock level') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $freshSource 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $freshRuntime 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $freshRuntime 'worlds\Bedrock level\level.dat') -Value 'fresh runtime world' -NoNewline
    $missingSourceIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $freshSource -AllowMissingWorld
    Assert-True ($null -eq $missingSourceIdentity) 'fresh BDS source unexpectedly reported a generated source world'
    $freshRuntimeIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $freshRuntime
    $freshPlan = New-CrossCropGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose CrossCropGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $CrossCropAssets
    Set-BdsSourceWorldIdentityOnPlan `
        -Plan $freshPlan `
        -Identity $missingSourceIdentity `
        -RuntimeIdentity $freshRuntimeIdentity
    Assert-True ($null -eq $freshPlan.Manifest.PSObject.Properties['source_world_identity']) 'fresh gallery mislabeled a runtime-created world as source evidence'
    Assert-Equal $freshRuntimeIdentity.sha256 $freshPlan.Manifest.runtime_world_identity.sha256 'fresh gallery did not bind the runtime-created world identity'
    $sourcePreferredPlan = New-CrossCropGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose CrossCropGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $CrossCropAssets
    Set-BdsSourceWorldIdentityOnPlan `
        -Plan $sourcePreferredPlan `
        -Identity $freshRuntimeIdentity `
        -RuntimeIdentity $freshRuntimeIdentity
    Assert-Equal $freshRuntimeIdentity.sha256 $sourcePreferredPlan.Manifest.source_world_identity.sha256 'existing source identity did not take precedence'
    Assert-True ($null -eq $sourcePreferredPlan.Manifest.PSObject.Properties['runtime_world_identity']) 'source-backed gallery also recorded runtime identity on its plan'

    $brokenSource = Join-Path $TempRoot 'broken reparse gallery source'
    $brokenTarget = Join-Path $TempRoot 'broken reparse gallery target'
    New-Item -ItemType Directory -Path $brokenSource, $brokenTarget -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $brokenSource 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    $null = New-Item -ItemType Junction -Path (Join-Path $brokenSource 'worlds') -Target $brokenTarget
    Remove-Item -LiteralPath $brokenTarget -Force
    Assert-ThrowsLike {
        Get-BdsSourceWorldIdentity -SourceDirectory $brokenSource -AllowMissingWorld
    } '*worlds*' 'fresh-world allowance accepted a broken worlds reparse point'

    $malformedSource = Join-Path $TempRoot 'malformed gallery source'
    New-Item -ItemType Directory -Path $malformedSource -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $malformedSource 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $malformedSource 'worlds') -Value 'not a directory' -NoNewline
    Assert-ThrowsLike {
        Get-BdsSourceWorldIdentity -SourceDirectory $malformedSource -AllowMissingWorld
    } '*worlds*' 'fresh-world allowance accepted a malformed worlds entry'
    Assert-Equal 'CrossCropGallery' $crossCropPlan.Manifest.fixture_kind 'cross/crop plan lost fixture kind'
    Assert-Equal 411 ([int]$crossCropPlan.Manifest.gallery_state_count) 'cross/crop plan did not enumerate the exact tracked Cross/Crop state set after flowerbeds moved to family 31'
    Assert-Equal 0 ([int]$crossCropPlan.Manifest.family_diagnostics.cross) 'cross family diagnostic contract changed'
    Assert-Equal 0 ([int]$crossCropPlan.Manifest.family_diagnostics.crop) 'crop family diagnostic contract changed'
    Assert-Equal $assetIdentity ([string]$crossCropPlan.Manifest.artifact_identity.assets_sha256) 'cross/crop plan lost asset identity'
    Assert-Equal 413 $crossCropPlan.GalleryCommands.Count 'cross/crop gallery command coverage is not one command per tracked state plus bounded setup'
    Assert-True (-not (($crossCropPlan.GalleryCommands -join "`n") -match 'seagrass|kelp')) 'Task 9 gallery included Task 10 aquatic plants'
    $firstPlan = $crossCropPlan.Manifest | ConvertTo-Json -Compress -Depth 12
    $secondPlan = (New-CrossCropGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose CrossCropGalleryFront -RegistryPath $BlockRegistry -AssetsPath $CrossCropAssets).Manifest | ConvertTo-Json -Compress -Depth 12
    Assert-Equal $firstPlan $secondPlan 'cross/crop gallery arguments were not deterministic'
    $backPlan = New-CrossCropGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose CrossCropGalleryBack -RegistryPath $BlockRegistry -AssetsPath $CrossCropAssets
    $movedPlan = New-CrossCropGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose CrossCropGalleryFront -RegistryPath $BlockRegistry -AssetsPath $CrossCropAssets
    Assert-Equal $crossCropPlan.Manifest.fixture_layout_hash $backPlan.Manifest.fixture_layout_hash 'front/back capture pose changed the fixture layout identity'
    Assert-Equal $crossCropPlan.Manifest.fixture_layout_hash $movedPlan.Manifest.fixture_layout_hash 'absolute mutation coordinate changed the fixture layout identity'
    Assert-Equal (Get-CanonicalObjectHash -Value $crossCropPlan.Manifest.relative_layout) $crossCropPlan.Manifest.fixture_layout_hash 'fixture layout hash is not the complete canonical relative layout descriptor'
    foreach ($plan in @($crossCropPlan, $backPlan)) {
        $cameraDistance = [Math]::Abs([double]$plan.Manifest.camera.position.z - [double]$plan.Manifest.gallery_center.z)
        $nearDepth = $cameraDistance - 18.0
        $requiredHorizontalFov = 2.0 * [Math]::Atan(23.0 / $nearDepth) * 180.0 / [Math]::PI
        Assert-True ($requiredHorizontalFov -le 60.0) "$($plan.Pose) cannot frame every exhaustive gallery column within the 60-degree horizontal-FOV contract"
    }

    $tamperedAssets = Join-Path $TempRoot 'tampered cross crop assets.mcbea'
    [IO.File]::WriteAllBytes($tamperedAssets, [IO.File]::ReadAllBytes($CrossCropAssets))
    $firstCrossCropId = [int]$crossCropPlan.CoverageEntries[0].sequential_id
    $tamperedBytes = [IO.File]::ReadAllBytes($tamperedAssets)
    $tamperedBytes[200 + 40 * $firstCrossCropId + 25] = 0
    [IO.File]::WriteAllBytes($tamperedAssets, $tamperedBytes)
    Assert-ThrowsLike {
        Get-CrossCropCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $tamperedAssets
    } '*diagnostic*' 'cross/crop coverage evidence accepted a diagnostic visual'

    $metadataIdentityIndex = $source.IndexOf("`$metadata['cross_crop_gallery']", [StringComparison]::Ordinal)
    $visualPublicationIndex = $source.IndexOf('`$fixturePublication = Publish-VisualFixture'.TrimStart('`'), [StringComparison]::Ordinal)
    Assert-True ($metadataIdentityIndex -ge 0 -and $visualPublicationIndex -gt $metadataIdentityIndex) 'cross/crop arguments and artifact identity were not recorded before visual fixture publication/capture'

    Assert-Equal 'AquaticGallery' $aquaticPlan.Manifest.fixture_kind 'aquatic plan lost fixture kind'
    Assert-Equal 29 ([int]$aquaticPlan.Manifest.gallery_state_count) 'aquatic plan did not enumerate exactly seagrass+kelp'
    Assert-Equal 3 ([int]$aquaticPlan.Manifest.coverage_evidence.seagrass_state_count) 'aquatic plan lost seagrass coverage'
    Assert-Equal 26 ([int]$aquaticPlan.Manifest.coverage_evidence.kelp_state_count) 'aquatic plan lost kelp coverage'
    Assert-Equal 0 ([int]$aquaticPlan.Manifest.family_diagnostics.seagrass_kelp) 'aquatic target diagnostic contract changed'
    Assert-Equal 113 $aquaticPlan.GalleryCommands.Count 'aquatic gallery command coverage changed'
    Assert-Equal 26 @($aquaticPlan.Manifest.body_witnesses).Count 'aquatic gallery did not provide one above-neighbor body witness per kelp age'
    $growthCappedTip = @($aquaticPlan.CoverageEntries | Where-Object { $_.name -ceq 'minecraft:kelp' -and $_.canonical_state -match '"kelp_age".*"value":25' })
    Assert-Equal 1 $growthCappedTip.Count 'aquatic fixture did not resolve one canonical age-25 kelp tip'
    Assert-Equal 26 @($aquaticPlan.Manifest.body_witnesses | Where-Object { $_.upper.sequential_id -eq $growthCappedTip[0].sequential_id }).Count 'body witnesses use a growable upper kelp tip'
    Assert-Equal 26 @($aquaticPlan.Manifest.isolated_kelp_heads).Count 'aquatic gallery did not provide one isolated head per kelp age'
    Assert-Equal 26 @($aquaticPlan.Manifest.head_growth_caps).Count 'isolated kelp heads can grow nondeterministically during capture'
    Assert-Equal 26 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:bedrock$' }).Count 'isolated kelp heads were not capped with a rendered non-kelp block'
    Assert-Equal 29 @($aquaticPlan.CoverageEntries).Count 'aquatic plan coverage entries included witness duplicates'
    Assert-Equal 1 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:water$' }).Count 'aquatic gallery did not build one source-water volume'
    Assert-Equal 1 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:bedrock$' }).Count 'aquatic gallery did not build one supported-texture tank shell'
    Assert-Equal 1 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:dirt$' }).Count 'aquatic gallery did not provide submerged plant support'
    Assert-Equal 3 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:air$' }).Count 'aquatic gallery did not clear setup volume and open both camera faces'
    $aquaticStateNames = @($aquaticPlan.CoverageEntries | ForEach-Object name | Sort-Object -Unique)
    Assert-Equal 'minecraft:kelp,minecraft:seagrass' ($aquaticStateNames -join ',') 'aquatic coverage admitted another Aquatic-family block'
    $aquaticFirst = $aquaticPlan.Manifest | ConvertTo-Json -Compress -Depth 12
    $aquaticSecond = (New-AquaticGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose AquaticGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets).Manifest | ConvertTo-Json -Compress -Depth 12
    Assert-Equal $aquaticFirst $aquaticSecond 'aquatic gallery arguments were not deterministic'
    $aquaticBack = New-AquaticGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose AquaticGalleryBack -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    $aquaticMoved = New-AquaticGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose AquaticGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    Assert-Equal $aquaticPlan.Manifest.fixture_layout_hash $aquaticBack.Manifest.fixture_layout_hash 'aquatic front/back pose changed fixture layout identity'
    Assert-Equal $aquaticPlan.Manifest.fixture_layout_hash $aquaticMoved.Manifest.fixture_layout_hash 'aquatic absolute coordinate changed fixture layout identity'
    Assert-Equal 20 ([Math]::Abs([int]$aquaticPlan.Manifest.camera.position.z - [int]$aquaticPlan.Manifest.gallery_center.z)) 'aquatic front camera is not outside the open tank face near the plants'
    Assert-Equal 20 ([Math]::Abs([int]$aquaticBack.Manifest.camera.position.z - [int]$aquaticBack.Manifest.gallery_center.z)) 'aquatic back camera is not outside the open tank face near the plants'

    Assert-Equal 'WaterGallery' $waterPlan.Manifest.fixture_kind 'water plan lost fixture kind'
    Assert-Equal 24 $waterPlan.GalleryCommands.Count 'water gallery command line changed'
    Assert-Equal 1 @($waterPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:water$' }).Count 'water gallery did not contain exactly one still-pool fill'
    Assert-True (@($waterPlan.GalleryCommands[3..6] | Where-Object { $_ -match ' minecraft:glass$' }).Count -eq 4) 'water gallery did not build its still-pool enclosure before placing water'
    Assert-True ($waterPlan.GalleryCommands[7] -match ' minecraft:water$') 'water gallery placed still water before its enclosure was complete'
    Assert-Equal 6 @($waterPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:water \["liquid_depth"=[0-5]\]$' }).Count 'water gallery lost its six-state downhill flow edge'
    Assert-Equal 'glass' $waterPlan.Manifest.relative_layout.flow_enclosure.block 'water gallery flow states were not enclosed against fluid ticks'
    Assert-Equal 1 @($waterPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:seagrass' }).Count 'water gallery lost its waterlogged plant witness'
    Assert-Equal 2 @($waterPlan.Manifest.relative_layout.biome_tint_witnesses).Count 'water gallery lost a biome-tint witness'
    Assert-Equal 'runtime-biome-index-water-tint-lookup' $waterPlan.Manifest.relative_layout.biome_tint_evidence.kind 'water gallery did not bind tint witnesses to runtime biome lookup'
    Assert-True (-not [bool]$waterPlan.Manifest.relative_layout.biome_tint_evidence.distinct_biome_colours_claimed) 'water gallery claimed nearby witnesses prove distinct biome colours'
    Assert-Equal 1 ([uint64]$waterPlan.Manifest.relative_layout.biome_tint_evidence.minimum_rendered_distinct_tint_count) 'water gallery did not state its honest single-biome live tint requirement'
    Assert-Equal 'bedrock-client::tests::compiled_and_live_biome_tables_preserve_raw_id_water_colour_parity' ([string]$waterPlan.Manifest.relative_layout.biome_tint_evidence.multi_biome_lookup_parity_test) 'water gallery did not retain the separate multi-biome lookup parity proof'
    Assert-True ([Math]::Abs([double]$waterPlan.Manifest.performance.maximum_p99_frame_ms - (1000.0 / 60.0)) -lt 0.0000001) 'water gallery did not manifest the exact 60fps p99 threshold'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.still_pool) 'water gallery did not manifest its still pool'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.downhill_flow_edge) 'water gallery did not manifest its downhill flow edge'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.waterlogged_plant) 'water gallery did not manifest its waterlogged plant'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.blend_edge) 'water gallery did not manifest its blend edge'
    Assert-True ($waterPlan.TeleportCommand -cne $waterPlan.CameraResortCommand) 'water gallery camera movement did not change the view'
    Assert-Equal $waterPlan.TeleportCommand $waterPlan.Manifest.camera_poses.initial.command 'water gallery manifest lost its initial fixed camera pose'
    Assert-Equal $waterPlan.CameraResortCommand $waterPlan.Manifest.camera_poses.resort.command 'water gallery manifest lost its moving-camera re-sort pose'
    $waterPlanAgain = New-WaterGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose WaterGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    $waterBack = New-WaterGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose WaterGalleryBack -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    $waterMoved = New-WaterGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose WaterGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    Assert-Equal ($waterPlan.Manifest | ConvertTo-Json -Compress -Depth 12) ($waterPlanAgain.Manifest | ConvertTo-Json -Compress -Depth 12) 'water gallery was not deterministic'
    Assert-Equal $waterPlan.Manifest.fixture_layout_hash $waterBack.Manifest.fixture_layout_hash 'water front/back pose changed fixture layout identity'
    Assert-Equal $waterPlan.Manifest.fixture_layout_hash $waterMoved.Manifest.fixture_layout_hash 'water absolute coordinate changed fixture layout identity'
    Assert-Equal $waterPlan.TeleportCommand $waterBack.CameraResortCommand 'water front initial pose did not equal back resort pose'
    Assert-Equal $waterPlan.CameraResortCommand $waterBack.TeleportCommand 'water front resort pose did not equal back initial pose'
    $waterWitnessRequest = New-WaterGalleryTransparentWitnessRequest -Plan $waterPlan -Revision 1
    Assert-Equal 'rust-mcbe-transparent-witness-v1' $waterWitnessRequest.schema 'water witness request lost its strict schema'
    Assert-Equal 1 ([uint64]$waterWitnessRequest.revision) 'water witness request lost its revision'
    Assert-Equal 0 ([int]$waterWitnessRequest.dimension) 'water witness request lost its dimension'
    Assert-Equal '5,4,12;6,4,12;7,4,12' (@($waterWitnessRequest.sub_chunks | ForEach-Object { "$($_.x),$($_.y),$($_.z)" }) -join ';') 'water witness request did not derive the exact unique liquid-bearing subchunks'

    $modelWitnessRequest = New-ModelGalleryWitnessRequest -Plan $slabStairPlans[0] -Revision 1
    Assert-Equal 'rust-mcbe-model-witness-v1' $modelWitnessRequest.schema 'model witness request lost its strict schema'
    Assert-Equal 1 ([uint64]$modelWitnessRequest.revision) 'model witness request lost its revision'
    Assert-Equal 0 ([int]$modelWitnessRequest.dimension) 'model witness request lost its dimension'
    Assert-True (@($modelWitnessRequest.sub_chunks).Count -gt 0) 'model witness request did not derive unique central-witness keys'
    Assert-Equal @($modelWitnessRequest.sub_chunks).Count @($modelWitnessRequest.sub_chunks | Sort-Object x, y, z -Unique).Count 'model witness request retained duplicate keys'
    Assert-True ([string]$modelWitnessRequest.request_sha256 -cmatch '^[0-9a-f]{64}$') 'model witness request lost its deterministic hash'
    Assert-Equal 'schema,revision,dimension,request_sha256,sub_chunks' (@($modelWitnessRequest.PSObject.Properties.Name) -join ',') 'slab/stair model witness request shape changed'
    $vineModelWitnessRequest = New-ModelGalleryWitnessRequest -Plan $vinePlans[0] -Revision 1
    Assert-Equal 'rust-mcbe-model-witness-v1' $vineModelWitnessRequest.schema 'vine model witness request lost its strict schema'
    Assert-Equal 'schema,revision,dimension,request_sha256,sub_chunks' (@($vineModelWitnessRequest.PSObject.Properties.Name) -join ',') 'vine model witness request shape is not exact'
    Assert-Equal 1 ([uint64]$vineModelWitnessRequest.revision) 'vine model witness request lost its revision'
    Assert-Equal 0 ([int]$vineModelWitnessRequest.dimension) 'vine model witness request lost its dimension'
    Assert-True (@($vineModelWitnessRequest.sub_chunks).Count -gt 0) 'vine model witness request did not derive unique central-witness keys'
    Assert-Equal @($vineModelWitnessRequest.sub_chunks).Count @($vineModelWitnessRequest.sub_chunks | Sort-Object x, y, z -Unique).Count 'vine model witness request retained duplicate keys'
    Assert-True ([string]$vineModelWitnessRequest.request_sha256 -cmatch '^[0-9a-f]{64}$') 'vine model witness request lost its deterministic hash'
    Assert-ThrowsLike {
        $wrongKind = $vinePlans[0] | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $wrongKind.Manifest.fixture_kind = 'vinegallery'
        New-ModelGalleryWitnessRequest -Plan $wrongKind -Revision 1
    } '*model witness request requires a recognized exact model gallery*' 'generic model witness request accepted noncanonical fixture-kind casing'

    $galleryAnchor = ConvertFrom-GalleryAnchorReadyMarker -Line 'RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=false clean=true'
    Assert-Equal '14,71,-6' (@($galleryAnchor.coordinate) -join ',') 'gallery anchor parser lost its exact mutation coordinate'
    Assert-Equal $false ([bool]$galleryAnchor.visible) 'gallery anchor parser lost the observed cave-visibility state'
    Assert-ThrowsLike {
        ConvertFrom-GalleryAnchorReadyMarker -Line 'RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=true clean=false'
    } '*invalid gallery anchor ready marker*' 'gallery anchor parser accepted an unclean target'
    Assert-ThrowsLike {
        ConvertFrom-GalleryAnchorReadyMarker -Line 'RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=true clean=true extra=true'
    } '*invalid gallery anchor ready marker*' 'gallery anchor parser accepted an unknown field'
    $cameraCommitted = ConvertFrom-CameraCommittedMarker -Line 'RUST_MCBE_CAMERA_COMMITTED sequence=19 position=27.00000,87.62000,43.00000 yaw=-45.00000 pitch=12.50000'
    Assert-Equal 19 ([uint64]$cameraCommitted.sequence) 'camera-commit parser lost sequence'
    Assert-Equal '27,87.62,43' (@($cameraCommitted.position) -join ',') 'camera-commit parser lost position'
    Assert-ThrowsLike {
        ConvertFrom-CameraCommittedMarker -Line 'RUST_MCBE_CAMERA_COMMITTED sequence=0 position=27,87.62,43 yaw=-45 pitch=12.5'
    } '*invalid camera committed marker*' 'camera-commit parser accepted sequence zero'
    $centeredCamera = ConvertFrom-CameraCommittedMarker -Line 'RUST_MCBE_CAMERA_COMMITTED sequence=19 position=27.50000,87.62001,43.50000 yaw=-45.00000 pitch=12.50000'
    $expectedCenteredCamera = Assert-ModelGalleryCommittedCamera -Committed $centeredCamera -Target ([pscustomobject]@{ x = 27; y = 86; z = 43 })
    Assert-Equal '27.5,87.62001,43.5' (@($expectedCenteredCamera.x, $expectedCenteredCamera.y, $expectedCenteredCamera.z) -join ',') 'camera assertion lost Bedrock horizontal centering'
    Assert-ThrowsLike {
        Assert-ModelGalleryCommittedCamera -Committed $cameraCommitted -Target ([pscustomobject]@{ x = 27; y = 86; z = 43 })
    } '*did not match the model gallery target*' 'camera assertion accepted uncentered horizontal coordinates'
    $appLaunchIndex = $source.IndexOf('$appHandle = Start-LoggedProcess -Executable $AppExecutable', [StringComparison]::Ordinal)
    $galleryAnchorBranchIndex = $source.IndexOf('if ($isModelWitnessGallery) {', $appLaunchIndex, [StringComparison]::Ordinal)
    $galleryAnchorWaitIndex = $source.IndexOf("-Marker 'RUST_MCBE_GALLERY_ANCHOR_READY '", $galleryAnchorBranchIndex, [StringComparison]::Ordinal)
    $normalStartupCoordinateIndex = $source.IndexOf('$coordinateMarker = Wait-ProcessOutputMarker', $galleryAnchorWaitIndex, [StringComparison]::Ordinal)
    $worldReadyWaitIndex = $source.IndexOf("-Marker 'RUST_MCBE_WORLD_READY '", $normalStartupCoordinateIndex, [StringComparison]::Ordinal)
    Assert-True ($appLaunchIndex -ge 0 -and $galleryAnchorBranchIndex -gt $appLaunchIndex -and $galleryAnchorWaitIndex -gt $galleryAnchorBranchIndex) 'generic model-gallery startup does not wait for its gallery-only early anchor'
    Assert-True ($normalStartupCoordinateIndex -gt $galleryAnchorWaitIndex -and $worldReadyWaitIndex -gt $normalStartupCoordinateIndex) 'normal/perf startup no longer retains its strict WorldReady wait'

    $publishVisualFixtureIndex = $source.IndexOf('function Publish-VisualFixture {', [StringComparison]::Ordinal)
    $modelGalleryClassificationIndex = $source.IndexOf('$isModelWitnessGallery =', $publishVisualFixtureIndex, [StringComparison]::Ordinal)
    Assert-True ($source.IndexOf("@('SlabStairGallery', 'VineGallery') -ccontains", $modelGalleryClassificationIndex, [StringComparison]::Ordinal) -gt $modelGalleryClassificationIndex) 'model-witness routing is not generic, exact, and case-sensitive for slab/stair plus vine'
    $modelGalleryPreTeleportBranchIndex = $source.IndexOf('if ($isV2 -and $isModelWitnessGallery) {', $modelGalleryClassificationIndex, [StringComparison]::Ordinal)
    $modelGalleryPreTeleportIndex = $source.IndexOf('Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand', $modelGalleryPreTeleportBranchIndex, [StringComparison]::Ordinal)
    $modelGalleryCameraCommitIndex = $source.IndexOf("-Marker 'RUST_MCBE_CAMERA_COMMITTED '", $modelGalleryPreTeleportIndex, [StringComparison]::Ordinal)
    Assert-True ($source.IndexOf('control_sequence = [uint64]$modelCameraCommit.sequence', $modelGalleryCameraCommitIndex, [StringComparison]::Ordinal) -gt $modelGalleryCameraCommitIndex) 'camera commit overwrites acceptance event sequence'
    $modelGalleryFixtureCompletionIndex = $source.IndexOf('$null = Complete-BdsFixtureCommandBatch', $modelGalleryPreTeleportBranchIndex, [StringComparison]::Ordinal)
    Assert-True `
        ($modelGalleryClassificationIndex -gt $publishVisualFixtureIndex -and
            $modelGalleryPreTeleportBranchIndex -gt $modelGalleryClassificationIndex -and
            $modelGalleryPreTeleportIndex -gt $modelGalleryPreTeleportBranchIndex -and
            $modelGalleryCameraCommitIndex -gt $modelGalleryPreTeleportIndex -and
            $modelGalleryFixtureCompletionIndex -gt $modelGalleryCameraCommitIndex) `
        'model gallery camera commit is not fenced ahead of the fixture update flood'
    Assert-True ($source.IndexOf('New-ModelGalleryWitnessRequest -Plan $Plan -Revision 1', $modelGalleryClassificationIndex, [StringComparison]::Ordinal) -gt $modelGalleryClassificationIndex) 'model gallery publication still routes through a slab/stair-only witness builder'

    $modelMarker = ConvertFrom-ModelWitnessCompleteMarker -Line "RUST_MCBE_MODEL_WITNESS_COMPLETE revision=1 request_sha256=$($modelWitnessRequest.request_sha256) sequence=40 view_generation=3 key_count=$(@($modelWitnessRequest.sub_chunks).Count) model_ref_count=43 manifest_count=$(@($modelWitnessRequest.sub_chunks).Count) manifest_sha256=$('a' * 64) missing=0 stale=0 wrong_stream=0 zero_ref=0 draw_mismatch=0 consecutive=1"
    Assert-Equal 1 ([int]$modelMarker.consecutive) 'model marker parser lost consecutive count'
    $modelMarker2 = $modelMarker.PSObject.Copy()
    $modelMarker2.sequence = [uint64]41
    $modelMarker2.consecutive = 2
    $null = Assert-StableModelWitnessEvidence -Request $modelWitnessRequest -First $modelMarker -Second $modelMarker2
    Assert-ThrowsLike {
        $bad = $modelMarker2.PSObject.Copy(); $bad.sequence = [uint64]42
        Assert-StableModelWitnessEvidence -Request $modelWitnessRequest -First $modelMarker -Second $bad
    } '*model witness*adjacent*' 'non-adjacent model witness evidence was accepted'
    $vineModelMarker = ConvertFrom-ModelWitnessCompleteMarker -Line "RUST_MCBE_MODEL_WITNESS_COMPLETE revision=1 request_sha256=$($vineModelWitnessRequest.request_sha256) sequence=487 view_generation=4 key_count=$(@($vineModelWitnessRequest.sub_chunks).Count) model_ref_count=93 manifest_count=$(@($vineModelWitnessRequest.sub_chunks).Count) manifest_sha256=$('b' * 64) missing=0 stale=0 wrong_stream=0 zero_ref=0 draw_mismatch=0 consecutive=1"
    $vineModelMarker2 = $vineModelMarker.PSObject.Copy()
    $vineModelMarker2.sequence = [uint64]488
    $vineModelMarker2.consecutive = 2
    $null = Assert-StableModelWitnessEvidence -Request $vineModelWitnessRequest -First $vineModelMarker -Second $vineModelMarker2
    Assert-ThrowsLike {
        $badSecond = $vineModelMarker2.PSObject.Copy(); $badSecond.model_ref_count = [uint64]94
        Assert-StableModelWitnessEvidence -Request $vineModelWitnessRequest -First $vineModelMarker -Second $badSecond
    } '*model witness*adjacent*' 'vine model witness accepted an unstable model reference count'
    Assert-ThrowsLike {
        $badSecond = $vineModelMarker2.PSObject.Copy(); $badSecond.request_sha256 = 'c' * 64
        Assert-StableModelWitnessEvidence -Request $vineModelWitnessRequest -First $vineModelMarker -Second $badSecond
    } '*model witness*adjacent*' 'vine model witness accepted the wrong request hash'
    Assert-ThrowsLike {
        $badFirst = $vineModelMarker.PSObject.Copy(); $badFirst.draw_mismatch = [uint64]1
        Assert-StableModelWitnessEvidence -Request $vineModelWitnessRequest -First $badFirst -Second $vineModelMarker2
    } '*model witness*adjacent*' 'vine model witness accepted dirty draw-mismatch evidence'

    $tamperedAquaticAssets = Join-Path $TempRoot 'tampered aquatic assets.mcbea'
    [IO.File]::WriteAllBytes($tamperedAquaticAssets, [IO.File]::ReadAllBytes($AquaticAssets))
    $firstAquaticId = [int]$aquaticPlan.CoverageEntries[0].sequential_id
    $tamperedAquaticBytes = [IO.File]::ReadAllBytes($tamperedAquaticAssets)
    $tamperedAquaticBytes[200 + 40 * $firstAquaticId + 25] = 0
    [IO.File]::WriteAllBytes($tamperedAquaticAssets, $tamperedAquaticBytes)
    Assert-ThrowsLike {
        Get-AquaticCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $tamperedAquaticAssets
    } '*diagnostic*' 'aquatic coverage evidence accepted a diagnostic target visual'
    $aquaticMetadataIdentityIndex = $source.IndexOf("`$metadata['aquatic_gallery']", [StringComparison]::Ordinal)
    Assert-True ($aquaticMetadataIdentityIndex -ge 0 -and $visualPublicationIndex -gt $aquaticMetadataIdentityIndex) 'aquatic arguments and artifact identity were not recorded before visual fixture publication/capture'

    $safeGeneratedRoot = Join-Path $TempRoot 'generated destinations'
    Assert-PrebuiltClientPathSafe `
        -ClientExecutable $PrebuiltClient `
        -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
        -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
        -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
        -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Split-Path -Parent $PrebuiltClient) `
            -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
            -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
            -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    } '*overlaps stable BDS runtime*' 'prebuilt client inside the generated BDS runtime was accepted'
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
            -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
            -CoreExecutable $PrebuiltClient `
            -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    } '*aliases generated core executable*' 'prebuilt client aliasing the core output was accepted'
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
            -RunDirectory (Split-Path -Parent $PrebuiltClient) `
            -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
            -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    } '*overlaps acceptance run output*' 'prebuilt client inside the acceptance output directory was accepted'
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
            -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
            -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
            -MetricsOut $PrebuiltClient
    } '*aliases requested metrics output*' 'prebuilt client aliasing MetricsOut was accepted'
    $prebuiltGuardHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash.ToLowerInvariant()
    Assert-FileHashUnchanged -Path $PrebuiltClient -ExpectedSha256 $prebuiltGuardHash -Label 'test prebuilt client'

    $teleportMarkerLine = 'RUST_MCBE_TELEPORT_SETTLED target=0:65:65:16 committed=0:65:65:16 ms=1500.0000 view_generation=7 transparent_sort_generation=11 render_ready_ms=1200.0000 publisher_ms=100.0000 first_level_ms=200.0000 last_level_ms=600.0000 level_events=1089 first_sub_ms=250.0000 last_sub_ms=900.0000 sub_events=1089 first_frame_sequence=41 stable_frame_sequence=42 first_present_ms=1300.0000 first_gpu_ms=1350.0000 stable_present_ms=1400.0000 stable_gpu_ms=1500.0000 expected_manifest_count=4 expected_manifest_hash=1111222233334444 first_presented_manifest_count=4 first_presented_manifest_hash=1111222233334444 stable_presented_manifest_count=4 stable_presented_manifest_hash=1111222233334444 expected=1089 loaded_target=1089 missing_target=0 foreign_loaded=0 foreign_requested=0 foreign_resident=0 source_leftover=0 resident_count=3 resident_hash=aaaabbbbccccdddd known_air_count=1 known_air_hash=eeeeffff00001111 missing_target_instances=0 unexpected_target_instances=0 source_instances=0 foreign_instances=0 stale_generation_instances=0 orphan_allocations=0 frame_count=90'
    $forcedMarkerLine = 'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED target=0:65:65:16 committed=0:65:65:16 ms=1500.0000 view_generation=8 transparent_sort_generation=12 render_ready_ms=0.0000 first_frame_sequence=43 stable_frame_sequence=44 first_present_ms=1200.0000 first_gpu_ms=1300.0000 stable_present_ms=1400.0000 stable_gpu_ms=1500.0000 expected_manifest_count=4 expected_manifest_hash=5555666677778888 first_presented_manifest_count=4 first_presented_manifest_hash=5555666677778888 stable_presented_manifest_count=4 stable_presented_manifest_hash=5555666677778888 expected=1089 loaded_target=1089 missing_target=0 foreign_loaded=0 foreign_requested=0 foreign_resident=0 source_leftover=0 resident_count=3 resident_hash=aaaabbbbccccdddd known_air_count=1 known_air_hash=eeeeffff00001111 missing_target_instances=0 unexpected_target_instances=0 source_instances=0 foreign_instances=0 stale_generation_instances=0 orphan_allocations=0 frame_count=90'
