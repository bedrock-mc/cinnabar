    $teleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine `
        -Kind Teleport
    $forcedMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine `
        -Kind ForcedRemesh
    Assert-Equal '0:65:65:16' $teleportMarker.target 'target-prefixed teleport marker lost its cohort'
    Assert-Equal 1500.0 $teleportMarker.ms 'target-prefixed teleport marker did not parse milliseconds'
    Assert-Equal '0:65:65:16' $forcedMarker.target 'target-prefixed forced-remesh marker lost its cohort'
    Assert-Equal 1500.0 $forcedMarker.ms 'target-prefixed forced-remesh marker did not parse milliseconds'
    Assert-Equal 11 $teleportMarker.transparent_sort_generation 'teleport marker lost presented transparent sort generation'
    Assert-Equal 12 $forcedMarker.transparent_sort_generation 'forced-remesh marker lost presented transparent sort generation'
    Assert-ThrowsLike {
        ConvertFrom-FullViewSettleMarker `
            -Line $teleportMarkerLine.Replace(' transparent_sort_generation=11', '') `
            -Kind Teleport
    } 'Teleport settle marker is missing transparent_sort_generation*' 'full-view marker accepted missing transparent presentation evidence'

    $worldReadyLine = 'RUST_MCBE_WORLD_READY source_tag=v1 blob_sha256=abc'
    $worldReadyTrigger = New-SteadyResourceTriggerEvidence `
        -Kind WorldReady `
        -WorldReadyMarker $worldReadyLine
    Assert-Equal 'WorldReady' $worldReadyTrigger.kind 'world-ready trigger changed kind'
    Assert-True ([string]$worldReadyTrigger.marker_sha256 -match '^[0-9a-f]{64}$') 'world-ready trigger omitted marker hash'
    $visualTrigger = New-SteadyResourceTriggerEvidence `
        -Kind VisualFixtureReady `
        -FixturePublication ([pscustomobject]@{
            ManifestSha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            LayoutHash = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
            Pose = 'LeafGalleryFront'
        })
    Assert-Equal 'VisualFixtureReady' $visualTrigger.kind 'visual trigger changed kind'
    Assert-Equal 'LeafGalleryFront' $visualTrigger.pose 'visual trigger lost pose'
    Assert-Equal ('a' * 64) $visualTrigger.manifest_sha256 'visual trigger lost manifest hash'
    Assert-Equal ('b' * 64) $visualTrigger.fixture_layout_hash 'visual trigger lost layout hash'
    $fullViewTrigger = New-SteadyResourceTriggerEvidence `
        -Kind FullViewPresented `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    Assert-Equal 'FullViewPresented' $fullViewTrigger.kind 'full-view trigger changed kind'
    Assert-ThrowsLike {
        New-SteadyResourceTriggerEvidence -Kind FullViewPresented -TeleportMarker $teleportMarker
    } '*ForcedRemeshMarker*' 'full-view trigger accepted incomplete binding evidence'

    $mutationCoordinate = @(101, 64, -37)
    $frontPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Front
    $frontPlanAgain = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Front
    $backPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Back
    $teleportPlan = New-FullViewTeleportPlan -MutationCoordinate $mutationCoordinate
    $leafFrontPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose LeafGalleryFront
    $leafFrontPlanAgain = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose LeafGalleryFront
    $leafBackPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose LeafGalleryBack
    $baselineForestPlan = New-LeafForestPlan -MutationCoordinate $mutationCoordinate -Mode Baseline
    $fullViewForestPlan = New-FullViewTeleportPlan -MutationCoordinate $mutationCoordinate -LeafForest

    $stablePrefix = [Text.Encoding]::UTF8.GetBytes("first line`n")
    $sparseLogBytes = [byte[]]::new($stablePrefix.Length + 8)
    [Array]::Copy($stablePrefix, $sparseLogBytes, $stablePrefix.Length)
    $sparseLogBytes[$stablePrefix.Length + 4] = [byte][char]'x'
    Assert-Equal `
        $stablePrefix.Length `
        (Get-ContiguousProcessLogByteCount -Buffer $sparseLogBytes -Count $sparseLogBytes.Length) `
        'process-log reader advanced into an uncommitted NUL gap'
    Assert-Equal `
        $stablePrefix.Length `
        (Get-ContiguousProcessLogByteCount -Buffer $stablePrefix -Count $stablePrefix.Length) `
        'process-log reader truncated a fully committed byte range'

    $zeroErrorEvidence = Assert-BdsFixtureCommandResults `
        -Commands @('fill 0 0 0 0 0 0 minecraft:air') `
        -Lines @('[2026-07-11 12:00:00:000 ERROR] 0 blocks filled')
    Assert-Equal 0 $zeroErrorEvidence.results[0].changed_count 'BDS zero-change fill error did not retain an exact zero result'

    foreach ($leafPlan in @($leafFrontPlan, $leafBackPlan, $baselineForestPlan, $fullViewForestPlan)) {
        Assert-Equal 'rust-mcbe-visual-fixture-v2' $leafPlan.Manifest.schema 'leaf plan used the wrong manifest schema'
        Assert-True ([string]$leafPlan.Manifest.fixture_layout_hash -match '^[0-9a-f]{64}$') 'leaf plan omitted a canonical SHA-256 layout hash'
        Assert-True ($leafPlan.Commands.Count -le 64) "leaf plan exceeded the 64-command bound: $($leafPlan.Commands.Count)"
        Assert-True ($leafPlan.Commands[-2] -ceq 'list') 'leaf plan fence was not immediately before teleport'
        Assert-True ($leafPlan.Commands[-1] -ceq $leafPlan.TeleportCommand) 'leaf plan teleport was not the final planned command'
        foreach ($volume in @($leafPlan.Manifest.fill_volumes)) {
            Assert-True ([int]$volume -le 32768) "leaf plan fill exceeded the BDS bound: $volume"
        }
        $leafCommands = @($leafPlan.FixtureCommands | Where-Object { $_ -match 'minecraft:.*leaves' })
        Assert-True ($leafCommands.Count -gt 0) 'leaf plan emitted no leaf command'
        foreach ($leafCommand in $leafCommands) {
            Assert-True ($leafCommand.Contains('"persistent_bit"=true')) "leaf command omitted persistent_bit=true: $leafCommand"
            Assert-True ($leafCommand.Contains('"update_bit"=false')) "leaf command omitted update_bit=false: $leafCommand"
        }
    }

    Assert-Equal ($leafFrontPlan.Commands -join "`n") ($leafFrontPlanAgain.Commands -join "`n") 'leaf-front commands were not deterministic'
    Assert-Equal $leafFrontPlan.Manifest.fixture_layout_hash $leafFrontPlanAgain.Manifest.fixture_layout_hash 'leaf-front layout hash was not deterministic'
    Assert-Equal $leafFrontPlan.Manifest.fixture_layout_hash $leafBackPlan.Manifest.fixture_layout_hash 'leaf gallery poses did not share one canonical layout'
    Assert-True ($leafFrontPlan.TeleportCommand -cne $leafBackPlan.TeleportCommand) 'leaf gallery front/back cameras were identical'
    $selfColored = @('minecraft:cherry_leaves', 'minecraft:azalea_leaves', 'minecraft:azalea_leaves_flowered')
    $tintDeferred = @('minecraft:oak_leaves', 'minecraft:birch_leaves', 'minecraft:spruce_leaves')
    Assert-Equal ($selfColored -join ',') (@($leafFrontPlan.Manifest.self_colored) -join ',') 'self-colored leaf set changed'
    Assert-Equal ($tintDeferred -join ',') (@($leafFrontPlan.Manifest.tint_deferred) -join ',') 'tint-deferred leaf set changed'
    Assert-Equal 6 @($leafFrontPlan.Manifest.blocks).Count 'leaf gallery did not contain six labeled 2x2x2 cubes'
    foreach ($block in @($leafFrontPlan.Manifest.blocks)) {
        Assert-Equal '2,2,2' (@($block.size) -join ',') "leaf cube $($block.label) was not 2x2x2"
        Assert-True ([bool]$block.persistent_bit) "leaf cube $($block.label) was not persistent"
        Assert-True (-not [bool]$block.update_bit) "leaf cube $($block.label) enabled update_bit"
    }
    Assert-True (@($leafFrontPlan.Manifest.leaf_adjacency).Count -gt 0) 'leaf gallery omitted leaf-to-leaf adjacency evidence'
    Assert-True (@($leafFrontPlan.Manifest.opaque_backing).Count -gt 0) 'leaf gallery omitted opaque backing touching leaves'
    Assert-Equal 'near,far' (@($leafFrontPlan.Manifest.panels | ForEach-Object { $_.distance }) -join ',') 'leaf gallery omitted deterministic near/far panels'

    Assert-Equal $baselineForestPlan.Manifest.fixture_layout_hash $fullViewForestPlan.Manifest.fixture_layout_hash 'baseline and full-view forests changed canonical layout'
    Assert-True (@($baselineForestPlan.Manifest.canopies).Count -ge 4) 'forest did not contain multiple bounded canopies'
    foreach ($forestPlan in @($baselineForestPlan, $fullViewForestPlan)) {
        Assert-Equal ($selfColored -join ',') (@($forestPlan.Manifest.self_colored) -join ',') 'forest self-colored leaf set changed'
        Assert-Equal ($tintDeferred -join ',') (@($forestPlan.Manifest.tint_deferred) -join ',') 'forest tint-deferred leaf set changed'
        $forestSelfColored = @($forestPlan.Manifest.canopies | Where-Object { $_.category -ceq 'self_colored' } | ForEach-Object { $_.block } | Sort-Object -Unique)
        $forestTintDeferred = @($forestPlan.Manifest.canopies | Where-Object { $_.category -ceq 'tint_deferred' } | ForEach-Object { $_.block } | Sort-Object -Unique)
        Assert-Equal (($selfColored | Sort-Object) -join ',') ($forestSelfColored -join ',') 'forest canopy categories lost a self-colored identifier'
        Assert-Equal (($tintDeferred | Sort-Object) -join ',') ($forestTintDeferred -join ',') 'forest canopy categories lost a tint-deferred identifier'
        Assert-Equal $mutationCoordinate[1] $forestPlan.Manifest.clear.min.y 'forest clear did not own the target ground layer'
        Assert-Equal 31213 $forestPlan.Manifest.clear.volume 'forest clear volume changed from the bounded 49x13x49 scene'
        Assert-True (@($forestPlan.Manifest.fill_volumes) -contains 31213) 'forest fill-volume evidence omitted the exact clear volume'
        Assert-Equal 0 $forestPlan.Manifest.layout.clear_min_offset[1] 'forest canonical layout did not own the ground layer'
        Assert-Equal 'rust_mcbe_leaf_forest' $forestPlan.LoadAreaName 'forest ticking-area name changed'
        Assert-Equal `
            'tickingarea add 1117 64 991 1165 76 1039 rust_mcbe_leaf_forest true' `
            $forestPlan.LoadAreaCommand `
            'forest did not request an exact deterministic preload rectangle around its clear bounds'
        Assert-Equal 'marked for preload.' $forestPlan.LoadAreaMarker 'forest waited for the wrong preload acknowledgement'
        Assert-Equal 8000 $forestPlan.LoadAreaSettleMilliseconds 'forest preload settle bound changed'
        Assert-Equal 'tickingarea remove rust_mcbe_leaf_forest' $forestPlan.CleanupCommand 'forest cleanup command changed'
        Assert-Equal 'Removed ticking area(s)' $forestPlan.CleanupMarker 'forest cleanup waited for the wrong acknowledgement'
        Assert-Equal $forestPlan.LoadAreaCommand $forestPlan.Commands[0] 'forest preload was not the first planned command'
        Assert-Equal 22 $forestPlan.Manifest.command_count 'forest command count omitted preload/fence/teleport'
        Assert-Equal 'rust_mcbe_leaf_forest' $forestPlan.Manifest.load_area.name 'forest manifest omitted the deterministic ticking-area name'
        Assert-True ([bool]$forestPlan.Manifest.load_area.preload) 'forest manifest did not require ticking-area preload'
        Assert-Equal 8000 $forestPlan.Manifest.load_area.settle_milliseconds 'forest manifest omitted preload settle evidence'
    }
    Assert-True ($null -eq $leafFrontPlan.PSObject.Properties['LoadAreaCommand']) 'near leaf gallery unexpectedly acquired a ticking area'
    $expectedFarCamera = @(($mutationCoordinate[0] + 1040), ($mutationCoordinate[1] + 12), ($mutationCoordinate[2] + 1040))
    $expectedTargetMutation = @(($mutationCoordinate[0] + 1040), $mutationCoordinate[1], ($mutationCoordinate[2] + 1052))
    Assert-Equal ($expectedFarCamera -join ',') (@($baselineForestPlan.Target.x, $baselineForestPlan.Target.y, $baselineForestPlan.Target.z) -join ',') 'baseline forest did not use the identical far camera/cohort'
    Assert-Equal ($expectedFarCamera -join ',') (@($fullViewForestPlan.Target.x, $fullViewForestPlan.Target.y, $fullViewForestPlan.Target.z) -join ',') 'far camera changed from the fixed 65-chunk binding target'
    Assert-Equal ($expectedTargetMutation -join ',') (@($baselineForestPlan.TargetMutation.x, $baselineForestPlan.TargetMutation.y, $baselineForestPlan.TargetMutation.z) -join ',') 'baseline forest did not use the identical far mutation coordinate'
    Assert-Equal ($expectedTargetMutation -join ',') (@($fullViewForestPlan.TargetMutation.x, $fullViewForestPlan.TargetMutation.y, $fullViewForestPlan.TargetMutation.z) -join ',') 'far target mutation changed from the no-CLI contract'
    Assert-Equal ($baselineForestPlan.Commands -join "`n") ($fullViewForestPlan.Commands -join "`n") 'baseline and full-view forests did not publish identical scene commands'
    Assert-Equal 65 $baselineForestPlan.Manifest.offset_chunks 'baseline forest did not publish the same far offset'
    $redundantTargetSupportCommand = "setblock $($expectedTargetMutation[0]) $($expectedTargetMutation[1] - 1) $($expectedTargetMutation[2]) minecraft:stone"
    Assert-True (-not ($fullViewForestPlan.FixtureCommands -contains $redundantTargetSupportCommand)) 'forest redundantly replaced target support already covered by its stone platform'
    $initialTargetCommand = "setblock $($expectedTargetMutation[0]) $($expectedTargetMutation[1]) $($expectedTargetMutation[2]) minecraft:diamond_block"
    Assert-True ($fullViewForestPlan.FixtureCommands -contains $initialTargetCommand) 'forest did not initialize target mutation to the opposite block'
    Assert-True (-not ($fullViewForestPlan.FixtureCommands -contains $initialTargetCommand.Replace('diamond_block', 'gold_block'))) 'forest initialized target to the first post-ARM block, making it a no-op'
    Assert-Equal 'minecraft:gold_block,minecraft:diamond_block' (@($fullViewForestPlan.Manifest.mutation_blocks) -join ',') 'target mutation alternation changed'
    Assert-Equal ($mutationCoordinate -join ',') (@($fullViewForestPlan.Manifest.source_mutation.x, $fullViewForestPlan.Manifest.source_mutation.y, $fullViewForestPlan.Manifest.source_mutation.z) -join ',') 'forest manifest lost source mutation identity'
    Assert-Equal ($expectedTargetMutation -join ',') (@($fullViewForestPlan.Manifest.target_mutation.x, $fullViewForestPlan.Manifest.target_mutation.y, $fullViewForestPlan.Manifest.target_mutation.z) -join ',') 'forest manifest lost target mutation identity'

    $preloadResult = Assert-BdsTickingAreaPreloadResult `
        -Line 'NO LOG FILE! - [2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.' `
        -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
        -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    Assert-Equal '1104,976,1167,1039' (@($preloadResult.min_x, $preloadResult.min_z, $preloadResult.max_x, $preloadResult.max_z) -join ',') 'preload acknowledgement lost snapped X/Z bounds'
    Assert-ThrowsLike {
        Assert-BdsTickingAreaPreloadResult `
            -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1120, 0, 992 to 1151, 0, 1023 marked for preload.' `
            -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
            -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    } 'ticking-area acknowledgement did not match exact chunk-snapped fixture bounds:*' 'forest accepted a snapped ticking area that did not cover its clear bounds'
    Assert-ThrowsLike {
        Assert-BdsTickingAreaPreloadResult `
            -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1088, 0, 960 to 1183, 0, 1055 marked for preload.' `
            -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
            -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    } 'ticking-area acknowledgement did not match exact chunk-snapped fixture bounds:*' 'forest accepted an overbroad stale ticking area that merely covered its clear bounds'
    Assert-ThrowsLike {
        Assert-BdsTickingAreaPreloadResult `
            -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039.' `
            -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
            -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    } 'invalid ticking-area preload acknowledgement:*' 'forest accepted a non-preloaded ticking area acknowledgement'

    foreach ($resultPlan in @($leafFrontPlan, $leafBackPlan, $baselineForestPlan, $fullViewForestPlan)) {
        $resultLines = New-TestBdsFixtureResultLines -Commands $resultPlan.FixtureCommands
        $resultEvidence = Assert-BdsFixtureCommandResults -Commands $resultPlan.FixtureCommands -Lines $resultLines
        Assert-Equal $resultPlan.FixtureCommands.Count $resultEvidence.result_count 'schema-v2 fixture result evidence lost a command result'
    }
    $zeroFillLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands)
    $zeroFillIndex = [Array]::FindIndex([string[]]$leafFrontPlan.FixtureCommands, [Predicate[string]]{ param($command) $command.StartsWith('fill ', [StringComparison]::Ordinal) })
    $zeroFillLines[$zeroFillIndex] = '[2026-07-11 12:00:00:000 INFO] 0 blocks filled'
    $null = Assert-BdsFixtureCommandResults -Commands $leafFrontPlan.FixtureCommands -Lines $zeroFillLines
    $outsideWorldLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $outsideWorldLines[0] = '[2026-07-11 12:00:00:000 ERROR] Cannot place blocks outside of the world'
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $fullViewForestPlan.FixtureCommands -Lines $outsideWorldLines
    } 'BDS fixture command failed:*outside of the world*' 'schema-v2 fixture accepted the live-observed outside-world failure'
    $missingResultLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands | Select-Object -Skip 1)
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $leafFrontPlan.FixtureCommands -Lines $missingResultLines
    } 'BDS fixture result count mismatch:*' 'schema-v2 fixture accepted a missing command result'
    $extraResultLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands) + @('[2026-07-11 12:00:00:000 INFO] Block placed')
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $leafFrontPlan.FixtureCommands -Lines $extraResultLines
    } 'BDS fixture result count mismatch:*' 'schema-v2 fixture accepted an extra command result'
    $outOfOrderLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $outOfOrderLines[0] = '[2026-07-11 12:00:00:000 INFO] Block placed'
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $fullViewForestPlan.FixtureCommands -Lines $outOfOrderLines
    } 'BDS fixture result did not match command 1:*' 'schema-v2 fixture accepted an out-of-order result type'
    $tooManyFilledLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $tooManyFilledLines[0] = '[2026-07-11 12:00:00:000 INFO] 31214 blocks filled'
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $fullViewForestPlan.FixtureCommands -Lines $tooManyFilledLines
    } 'BDS fill result exceeded declared command volume:*' 'schema-v2 fixture accepted an impossible fill count'

    $armedMarker = ConvertFrom-TargetMutationArmedMarker -Line 'RUST_MCBE_TARGET_MUTATION_ARMED source=101,64,-37 target=1141,64,1015 view_generation=9'
    Assert-Equal '101,64,-37' (@($armedMarker.source) -join ',') 'target-mutation marker lost source coordinate'
    Assert-Equal '1141,64,1015' (@($armedMarker.target) -join ',') 'target-mutation marker lost target coordinate'
    Assert-Equal 9 $armedMarker.view_generation 'target-mutation marker lost view generation'
    Assert-ThrowsLike {
        ConvertFrom-TargetMutationArmedMarker -Line 'RUST_MCBE_TARGET_MUTATION_ARMED source=101,64,-37 target=1141,64,1015'
    } 'invalid target mutation armed marker:*' 'target-mutation marker accepted a missing generation'
    $movePlayerIngress = ConvertFrom-MovePlayerIngressMarker -Line 'RUST_MCBE_MOVE_PLAYER_INGRESS sequence=27 position=1141.5,76.25,1003.5'
    Assert-Equal 27 $movePlayerIngress.sequence 'MovePlayer ingress marker lost its sequence'
    Assert-Equal 1141.5 ([double]$movePlayerIngress.position[0]) 'MovePlayer ingress marker lost decimal X'
    Assert-Equal 76.25 ([double]$movePlayerIngress.position[1]) 'MovePlayer ingress marker lost decimal Y'
    Assert-Equal 1003.5 ([double]$movePlayerIngress.position[2]) 'MovePlayer ingress marker lost decimal Z'
    Assert-ThrowsLike {
        ConvertFrom-MovePlayerIngressMarker -Line 'RUST_MCBE_MOVE_PLAYER_INGRESS sequence=0 position=1141.5,76.25,1003.5'
    } 'invalid MovePlayer ingress marker:*' 'MovePlayer ingress marker accepted sequence zero'

    Assert-Equal ($frontPlan.Commands -join "`n") ($frontPlanAgain.Commands -join "`n") 'front fixture commands were not deterministic'
    Assert-True ($frontPlan.TeleportCommand -cne $backPlan.TeleportCommand) 'front and back fixture teleports were identical'
    $expectedGalleryCenter = @($mutationCoordinate[0], ($mutationCoordinate[1] + 3), ($mutationCoordinate[2] + 4))
    $expectedFrontCamera = @($mutationCoordinate[0], ($mutationCoordinate[1] + 12), ($mutationCoordinate[2] - 24))
    $expectedBackCamera = @($mutationCoordinate[0], ($mutationCoordinate[1] + 10), ($mutationCoordinate[2] + 32))
    $frontCamera = $frontPlan.Manifest.camera.position
    $backCamera = $backPlan.Manifest.camera.position
    Assert-Equal `
        ($expectedFrontCamera -join ',') `
        (@($frontCamera.x, $frontCamera.y, $frontCamera.z) -join ',') `
        'front camera did not use the live-proven framing offset'
    Assert-Equal `
        ($expectedBackCamera -join ',') `
        (@($backCamera.x, $backCamera.y, $backCamera.z) -join ',') `
        'back camera did not use the live-proven framing offset'
    Assert-Equal 28 ([Math]::Abs([int]$frontCamera.z - [int]$frontPlan.Manifest.gallery_center.z)) 'front camera was not 28 Z blocks from gallery center'
    Assert-Equal 28 ([Math]::Abs([int]$backCamera.z - [int]$backPlan.Manifest.gallery_center.z)) 'back camera was not 28 Z blocks from gallery center'
    Assert-Equal `
        "tp @a[name=RustMCBE] $($expectedFrontCamera -join ' ') facing $($expectedGalleryCenter -join ' ')" `
        $frontPlan.TeleportCommand `
        'front teleport did not preserve the exact widened pose and gallery target'
    Assert-Equal `
        "tp @a[name=RustMCBE] $($expectedBackCamera -join ' ') facing $($expectedGalleryCenter -join ' ')" `
        $backPlan.TeleportCommand `
        'back teleport did not preserve the exact widened pose and gallery target'
    Assert-Equal 'list' $frontPlan.FenceCommand 'front fixture did not use the observable BDS list fence'
    Assert-Equal 'players online:' $frontPlan.FenceMarker 'front fixture waited for the wrong BDS list output'
    Assert-True ($frontPlan.Commands[-2] -ceq 'list') 'front processing fence was not immediately before teleport'
    Assert-True ($frontPlan.Commands[-1] -ceq $frontPlan.TeleportCommand) 'front teleport was not the final fixture command'
    Assert-Equal 'list' $backPlan.FenceCommand 'back fixture did not use the observable BDS list fence'
    Assert-Equal 'players online:' $backPlan.FenceMarker 'back fixture waited for the wrong BDS list output'
    Assert-True ($backPlan.Commands[-2] -ceq 'list') 'back processing fence was not immediately before teleport'
    Assert-True ($backPlan.Commands[-1] -ceq $backPlan.TeleportCommand) 'back teleport was not the final fixture command'
    Assert-True ($frontPlan.TeleportCommand.Contains('@a[name=RustMCBE]')) 'fixture teleport did not target the stable offline player name'
    $teleportDeltaChunks = [Math]::Abs([int]$teleportPlan.Target.x - $mutationCoordinate[0]) / 16
    Assert-True ($teleportDeltaChunks -gt 64) 'full-view teleport did not exceed two radius-16 view diameters'
    Assert-Equal 'list' $teleportPlan.FenceCommand 'full-view teleport did not use the observable BDS fence'
    Assert-True ($teleportPlan.TeleportCommand.Contains('@a[name=RustMCBE]')) 'full-view teleport did not target the stable offline player name'

    $clear = $frontPlan.Manifest.clear
    $clearVolume = ([int]$clear.max.x - [int]$clear.min.x + 1) *
        ([int]$clear.max.y - [int]$clear.min.y + 1) *
        ([int]$clear.max.z - [int]$clear.min.z + 1)
    Assert-True ($clearVolume -le 32768) "fixture clear volume exceeded BDS fill limit: $clearVolume"
    Assert-True (([int]$clear.min.y) -gt $mutationCoordinate[1]) 'fixture clear volume did not preserve the mutation surface block'

    $sandMinX = $mutationCoordinate[0] + 14
    $sandMaxX = $mutationCoordinate[0] + 15
    $sandMinZ = $mutationCoordinate[2] + 5
    $sandMaxZ = $mutationCoordinate[2] + 6
    $expectedSandSupport = "fill $sandMinX $($mutationCoordinate[1]) $sandMinZ $sandMaxX $($mutationCoordinate[1]) $sandMaxZ minecraft:stone"
    $expectedSandCube = "fill $sandMinX $($mutationCoordinate[1] + 1) $sandMinZ $sandMaxX $($mutationCoordinate[1] + 2) $sandMaxZ minecraft:sand"
    $sandSupportIndexes = @(for ($index = 0; $index -lt $frontPlan.Commands.Count; $index++) {
        if ($frontPlan.Commands[$index] -ceq $expectedSandSupport) {
            $index
        }
    })
    $sandCubeIndexes = @(for ($index = 0; $index -lt $frontPlan.Commands.Count; $index++) {
        if ($frontPlan.Commands[$index] -ceq $expectedSandCube) {
            $index
        }
    })
    Assert-Equal 1 $sandSupportIndexes.Count 'fixture did not contain exactly one deterministic hidden sand support'
    Assert-Equal 1 $sandCubeIndexes.Count 'fixture did not contain exactly one sand cube fill'
    Assert-True ($sandSupportIndexes[0] -lt $sandCubeIndexes[0]) 'fixture built the sand cube before its hidden support'

    $requiredBlocks = @(
        'minecraft:stone',
        'minecraft:dirt',
        'minecraft:grass_block',
        'minecraft:oak_planks',
        'minecraft:coal_ore',
        'minecraft:iron_ore',
        'minecraft:diamond_ore',
        'minecraft:sand',
        'minecraft:glass'
    )
    $fixtureCommands = $frontPlan.Commands -join "`n"
    foreach ($requiredBlock in $requiredBlocks) {
        Assert-True ($fixtureCommands.Contains($requiredBlock)) "fixture commands are missing $requiredBlock"
    }
    foreach ($axis in @('x', 'y', 'z')) {
        Assert-True ($fixtureCommands.Contains("minecraft:oak_log [`"pillar_axis`"=`"$axis`"]")) "fixture commands are missing the Bedrock-state $axis oak-log beam"
    }
    Assert-True ($fixtureCommands.Contains('minecraft:oak_stairs')) 'fixture commands are missing oak stairs'
    Assert-True ($fixtureCommands.Contains('minecraft:glass_pane')) 'fixture commands are missing glass panes'

    $expectedLabels = @('stone', 'dirt', 'grass', 'oak_planks', 'coal_ore', 'iron_ore', 'diamond_ore', 'sand', 'glass')
    $manifestLabels = @($frontPlan.Manifest.blocks | ForEach-Object { $_.label })
    Assert-Equal ($expectedLabels -join ',') ($manifestLabels -join ',') 'fixture manifest labels changed'
    Assert-Equal 'Front' $frontPlan.Manifest.pose 'fixture manifest did not record its pose'
    Assert-Equal 'list' $frontPlan.Manifest.processing_fence.command 'fixture manifest recorded the wrong fence command'
    Assert-Equal 'players online:' $frontPlan.Manifest.processing_fence.stdout_marker 'fixture manifest recorded the wrong fence marker'
    Assert-Equal ($mutationCoordinate -join ',') (@($frontPlan.Manifest.mutation.x, $frontPlan.Manifest.mutation.y, $frontPlan.Manifest.mutation.z) -join ',') 'fixture manifest did not derive from the mutation coordinate'
    Assert-True ($null -ne $frontPlan.Manifest.camera) 'fixture manifest omitted expected camera coordinates'
    Assert-True ($null -ne $frontPlan.Manifest.gallery_center) 'fixture manifest omitted expected gallery coordinates'

    foreach ($fixtureCommand in $frontPlan.Commands) {
        Assert-True (($fixtureCommand -ceq 'list') -or ($fixtureCommand -match '^(fill|setblock|tp) ')) "fixture contains an unexpected server command: $fixtureCommand"
        Assert-True (-not $fixtureCommand.Contains($BdsDir)) 'fixture command targeted the source BDS directory'
        Assert-True (-not $fixtureCommand.Contains("`r") -and -not $fixtureCommand.Contains("`n")) 'fixture command contains an injected newline'
    }

    $fixtureInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $fixtureHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $fixtureInput }
    }
    $script:ObservedFixtureFence = $null
    $fixtureRunDirectory = Join-Path $TempRoot 'fixture run'
    New-Item -ItemType Directory -Path $fixtureRunDirectory | Out-Null
    $fixturePublication = Publish-VisualFixture `
        -Handle $fixtureHandle `
        -Plan $frontPlan `
        -RunDirectory $fixtureRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedFixtureFence = $Marker
            return $Marker
        }
    $fixtureLogPath = Join-Path $fixtureRunDirectory 'bds.console.log'
    $fixtureReadyPath = Join-Path $fixtureRunDirectory 'visual-fixture-ready.json'
    Assert-True (Test-Path -LiteralPath $fixtureReadyPath -PathType Leaf) 'fixture ready artifact was not published'
    Assert-Equal $fixtureReadyPath $fixturePublication.Path 'fixture publication returned the wrong path'
    Assert-True ([string]$fixturePublication.ManifestSha256 -match '^[0-9a-f]{64}$') 'fixture publication omitted its file hash'
    Assert-Equal ($frontPlan.Commands -join "`n") ((Get-Content -LiteralPath $fixtureLogPath) -join "`n") 'fixture console log did not record every command in order'
    Assert-Equal ($frontPlan.Commands -join [Environment]::NewLine) $fixtureInput.ToString().TrimEnd("`r", "`n") 'fixture commands were not sent through the owned standard input in order'
    Assert-Equal $frontPlan.FenceMarker $script:ObservedFixtureFence 'fixture publisher did not wait for the processing fence'
    $fixtureReady = Get-Content -Raw -LiteralPath $fixtureReadyPath | ConvertFrom-Json
    Assert-Equal 'Front' $fixtureReady.pose 'fixture ready artifact recorded the wrong pose'
    Assert-Equal 'list' $fixtureReady.processing_fence.command 'fixture ready artifact recorded the wrong fence command'
    Assert-Equal 'players online:' $fixtureReady.processing_fence.stdout_marker 'fixture ready artifact recorded the wrong fence marker'
    Assert-Equal 3000 $fixtureReady.settle_milliseconds 'fixture ready artifact did not record the production settle duration'
    Assert-Equal $frontPlan.TeleportCommand $fixtureReady.teleport_command 'fixture ready artifact recorded the wrong teleport'

    $waterInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $waterHandle = [pscustomobject]@{ Process = [pscustomobject]@{ StandardInput = $waterInput } }
    $script:WaterFenceCount = 0
    $script:WaterSortMarkerCount = 0
    $waterAppStdout = Join-Path $TempRoot 'water app stdout.log'
    [IO.File]::WriteAllText(
        $waterAppStdout,
        "RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=3 ref_count=12`n",
        [Text.UTF8Encoding]::new($false)
    )
    $waterAppHandle = [pscustomobject]@{
        StdoutPath = $waterAppStdout
        StdoutMarkerCursor = [pscustomobject]@{ Offset = [long]0; PartialLine = ''; LineNumber = [uint64]0 }
        Process = [pscustomobject]@{}
    }
    $waterRunDirectory = Join-Path $TempRoot 'water gallery run'
    New-Item -ItemType Directory -Path $waterRunDirectory | Out-Null
    $waterPublication = Publish-VisualFixture `
        -Handle $waterHandle `
        -Plan $waterPlan `
        -RunDirectory $waterRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:WaterFenceCount++
            if ($script:WaterFenceCount -eq 1) {
                $commandResults = @($waterPlan.FixtureCommands | ForEach-Object {
                    if ($_ -match '^fill ') { '[2026-07-11 00:00:00 INFO] 1 blocks filled' }
                    else { '[2026-07-11 00:00:00 INFO] Block placed' }
                })
                return New-TestBdsMarkerEvidence -Line '[INFO] There are 1/10 players online:' -SkippedLines $commandResults
            }
            if ($script:WaterFenceCount -eq 2) {
                return New-TestBdsMarkerEvidence `
                    -Line '[INFO] There are 1/10 players online:' `
                    -SkippedLines @('[INFO] Teleported RustMCBE to 100.000000, 74.000000, 176.000000')
            }
            return New-TestBdsMarkerEvidence `
                -Line '[INFO] There are 1/10 players online:' `
                -SkippedLines @('[INFO] Teleported RustMCBE to 100.000000, 73.000000, 224.000000')
        } `
        -AppHandle $waterAppHandle `
        -WaitForAppMarker {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:WaterSortMarkerCount++
            $lines = if ($Marker -ceq 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE ') {
                if ($script:WaterSortMarkerCount -eq 3) {
                    @('RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=1 sequence=21 generation=194 key_count=3 consecutive=1')
                }
                else {
                    @('RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=1 sequence=22 generation=194 key_count=3 consecutive=2')
                }
            }
            elseif ($script:WaterSortMarkerCount -eq 1) {
                @(
                    'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=7 ref_count=42',
                    'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=8 ref_count=42'
                )
            }
            else {
                @('RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=9 ref_count=42')
            }
            [IO.File]::AppendAllText($Handle.StdoutPath, (($lines -join "`n") + "`n"), [Text.UTF8Encoding]::new($false))
            return Wait-ProcessOutputMarker -Handle $Handle -Marker $Marker -TimeoutSeconds $TimeoutSeconds -PassThruEvidence
        }
    Assert-Equal $waterPlan.Manifest.fixture_layout_hash $waterPublication.LayoutHash 'water publication lost layout hash'
    Assert-Equal ($waterPlan.Commands -join [Environment]::NewLine) $waterInput.ToString().TrimEnd("`r", "`n") 'water gallery did not execute its fixed initial and moving-camera resort poses in order'
    Assert-Equal 3 $script:WaterFenceCount 'water gallery did not fence fixture construction and both camera poses'
    Assert-Equal 4 $script:WaterSortMarkerCount 'water gallery did not fence both committed transparent sorts and two GPU-presented exact-key witnesses'
    Assert-True (Test-Path -LiteralPath (Join-Path $waterRunDirectory 'transparent-witness-request.json') -PathType Leaf) 'water gallery did not atomically publish its exact witness request'
    $publishedWater = Get-Content -Raw -LiteralPath $waterPublication.Path | ConvertFrom-Json
    Assert-Equal $waterPlan.CameraResortCommand $publishedWater.camera_resort_command 'published water manifest lost moving-camera evidence'
    $waterEvents = @(Get-Content -LiteralPath (Join-Path $waterRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'fixture_commands_validated,initial_camera_fence_observed,initial_transparent_sort_committed,camera_resort_issued,camera_resort_fence_observed,resort_transparent_sort_committed,transparent_witness_complete,transparent_witness_complete' (@($waterEvents.event) -join ',') 'water gallery did not retain validated commands and causal exact-key GPU-presentation evidence'
    Assert-Equal '7,9' (@($waterEvents | Where-Object { $_.event -match 'transparent_sort_committed$' } | ForEach-Object { $_.generation }) -join ',') 'buffered pre-pose sort markers satisfied causal water-gallery evidence'
    Assert-ThrowsLike {
        Assert-BdsCameraResortResult -Evidence (New-TestBdsMarkerEvidence `
            -Line '[INFO] There are 1/10 players online:' `
            -SkippedLines @('[ERROR] No targets matched selector'))
    } 'BDS camera resort command failed:*' 'water gallery accepted a rejected camera resort before its fence'

    $sortMarker = ConvertFrom-TransparentSortCommittedMarker -Line 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=17 ref_count=99'
    Assert-Equal 17 $sortMarker.generation 'transparent sort marker parser lost generation'
    Assert-Equal 99 $sortMarker.ref_count 'transparent sort marker parser lost ref count'
    Assert-ThrowsLike {
        ConvertFrom-TransparentSortCommittedMarker -Line 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=0 ref_count=99'
    } 'invalid transparent sort committed marker:*' 'transparent sort marker accepted generation zero'
    $witnessMarker = ConvertFrom-TransparentWitnessCompleteMarker -Line 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=3 sequence=17 generation=194 key_count=3 consecutive=2'
    Assert-Equal 3 ([uint64]$witnessMarker.revision) 'transparent witness marker parser lost revision'
    Assert-Equal 2 ([int]$witnessMarker.consecutive) 'transparent witness marker parser lost consecutive proof count'
    Assert-ThrowsLike {
        ConvertFrom-TransparentWitnessCompleteMarker -Line 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=3 sequence=17 generation=194 key_count=0 consecutive=2'
    } 'invalid transparent witness complete marker:*' 'transparent witness marker accepted zero keys'
    $null = Assert-StableTransparentWitnessEvidence `
        -Request ([pscustomobject]@{ revision = [uint64]3; sub_chunks = @([pscustomobject]@{ x = 1; y = 2; z = 3 }) }) `
        -First ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]21; generation = [uint64]194; key_count = [uint64]1; consecutive = 1 }) `
        -Second ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]22; generation = [uint64]194; key_count = [uint64]1; consecutive = 2 })
    Assert-ThrowsLike {
        Assert-StableTransparentWitnessEvidence `
            -Request ([pscustomobject]@{ revision = [uint64]3; sub_chunks = @([pscustomobject]@{ x = 1; y = 2; z = 3 }) }) `
            -First ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]21; generation = [uint64]193; key_count = [uint64]1; consecutive = 1 }) `
            -Second ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]22; generation = [uint64]194; key_count = [uint64]1; consecutive = 1 })
    } 'transparent witness did not complete twice consecutively:*' 'a gen193 missing-key frame followed by only one gen194 complete frame satisfied readiness'
    $null = Assert-NewerTransparentSortCommit `
        -Initial ([pscustomobject]@{ generation = [uint64]7 }) `
        -InitialLineNumber 101 `
        -Resort ([pscustomobject]@{ generation = [uint64]8 }) `
        -ResortLineNumber 102
    Assert-ThrowsLike {
        Assert-NewerTransparentSortCommit `
            -Initial ([pscustomobject]@{ generation = [uint64]7 }) `
            -InitialLineNumber 101 `
            -Resort ([pscustomobject]@{ generation = [uint64]7 }) `
            -ResortLineNumber 102
    } 'camera resort did not commit a newer transparent sort:*' 'camera resort accepted the initial sort generation again'

    $forestInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $forestHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $forestInput }
    }
    $forestResultLines = New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands
    $script:ObservedForestFence = $null
    $script:ObservedForestLoadAreaMarker = $null
    $forestRunDirectory = Join-Path $TempRoot 'forest full view run'
    New-Item -ItemType Directory -Path $forestRunDirectory | Out-Null
    $forestPublication = Publish-FullViewTeleport `
        -Handle $forestHandle `
        -Plan $fullViewForestPlan `
        -RunDirectory $forestRunDirectory `
        -PreloadSettleMilliseconds 0 `
        -WaitForLoadArea {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedForestLoadAreaMarker = $Marker
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
        } `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedForestFence = $Marker
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                -SkippedLines $forestResultLines
        }
    Assert-Equal $fullViewForestPlan.LoadAreaMarker $script:ObservedForestLoadAreaMarker 'forest publisher did not observe the preload acknowledgement'
    Assert-Equal $fullViewForestPlan.FenceMarker $script:ObservedForestFence 'forest publisher did not observe the list fence'
    Assert-Equal $fullViewForestPlan.LoadAreaName $forestHandle.ActiveTickingArea.Name 'forest publisher did not retain active ticking-area cleanup state'
    Assert-True (Test-Path -LiteralPath $forestPublication.Path -PathType Leaf) 'forest publisher did not atomically publish its manifest'
    Assert-Equal $fullViewForestPlan.Manifest.fixture_layout_hash $forestPublication.LayoutHash 'forest publication lost layout hash'
    $forestManifestHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $forestPublication.Path).Hash.ToLowerInvariant()
    Assert-Equal $forestManifestHash $forestPublication.ManifestSha256 'forest publication hash did not match bytes'
    Assert-PublishedTargetMutation -Path $forestPublication.Path -Expected $fullViewForestPlan.TargetMutation
    $tamperedForestManifestPath = Join-Path $forestRunDirectory 'tampered-visual-fixture-ready.json'
    $tamperedForestManifest = Get-Content -Raw -LiteralPath $forestPublication.Path | ConvertFrom-Json
    $tamperedForestManifest.target_mutation.x = [int]$tamperedForestManifest.target_mutation.x + 1
    [IO.File]::WriteAllText(
        $tamperedForestManifestPath,
        ($tamperedForestManifest | ConvertTo-Json -Depth 16),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike {
        Assert-PublishedTargetMutation -Path $tamperedForestManifestPath -Expected $fullViewForestPlan.TargetMutation
    } 'published target mutation did not match plan*' 'far publisher accepted a serialized target mutation mismatch'
    Assert-Equal ($fullViewForestPlan.Commands -join "`n") ((Get-Content -LiteralPath (Join-Path $forestRunDirectory 'bds.console.log')) -join "`n") 'forest commands/fence/teleport were not sent in exact order'
    Assert-Equal ($fullViewForestPlan.Commands -join [Environment]::NewLine) $forestInput.ToString().TrimEnd("`r", "`n") 'forest owned-stdin order changed'
    $forestEvents = @(Get-Content -LiteralPath (Join-Path $forestRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal `
        'load_area_ready,fixture_commands_completed,processing_fence_observed,visual_fixture_ready,teleport_issued' `
        (@($forestEvents | ForEach-Object { $_.event }) -join ',') `
        'forest evidence event order changed'
    $forestReadyEvent = @($forestEvents | Where-Object { $_.event -ceq 'visual_fixture_ready' })[0]
    $forestTeleportEvent = @($forestEvents | Where-Object { $_.event -ceq 'teleport_issued' })[0]
    Assert-True ([int]$forestReadyEvent.sequence -lt [int]$forestTeleportEvent.sequence) 'forest teleport preceded atomic manifest readiness'
    Assert-Equal `
        (@($fullViewForestPlan.TargetMutation.x, $fullViewForestPlan.TargetMutation.y, $fullViewForestPlan.TargetMutation.z) -join ',') `
        ([string]$forestReadyEvent.target_mutation) `
        'forest ready event lost target mutation coordinate'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $forestRunDirectory -Filter '*.partial-*' -File).Count 'forest publication leaked a partial manifest'

    $baselineInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $baselineHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $baselineInput }
    }
    $baselineRunDirectory = Join-Path $TempRoot 'forest baseline run'
    New-Item -ItemType Directory -Path $baselineRunDirectory | Out-Null
    $baselineSourceCommand = Publish-BaselineSourceMutation `
        -Handle $baselineHandle `
        -Coordinate $mutationCoordinate `
        -RunDirectory $baselineRunDirectory
    $null = Start-BdsFixtureLoadArea `
        -Handle $baselineHandle `
        -Plan $baselineForestPlan `
        -RunDirectory $baselineRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForLoadArea {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
        }
    $script:BaselineReuseWaitInvoked = $false
    $null = Publish-VisualFixture `
        -Handle $baselineHandle `
        -Plan $baselineForestPlan `
        -RunDirectory $baselineRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForLoadArea {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:BaselineReuseWaitInvoked = $true
            throw 'publisher attempted to wait for an already-ready exact ticking area'
        } `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                -SkippedLines (New-TestBdsFixtureResultLines -Commands $baselineForestPlan.FixtureCommands)
        }
    Assert-Equal 'setblock 101 64 -37 minecraft:gold_block' $baselineSourceCommand 'baseline source mutation prelude changed'
    Assert-True (-not $script:BaselineReuseWaitInvoked) 'baseline publisher did not reuse its ready ticking area'
    $expectedBaselineConsole = @($baselineSourceCommand) + @($baselineForestPlan.Commands)
    Assert-Equal ($expectedBaselineConsole -join "`n") ((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) -join "`n") 'baseline source mutation did not precede the far forest fence/teleport'
    Assert-Equal 1 @((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) | Where-Object { $_ -ceq $baselineForestPlan.LoadAreaCommand }).Count 'baseline exact-plan reuse issued the ticking-area add more than once'
    $baselineEvents = @(Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal `
        'source_mutation_command,load_area_ready,load_area_reused,fixture_commands_completed,processing_fence_observed,visual_fixture_ready,teleport_issued' `
        (@($baselineEvents | ForEach-Object { $_.event }) -join ',') `
        'baseline event evidence did not order source mutation before the far forest'
    $mismatchedLoadAreaPlan = New-LeafForestPlan -MutationCoordinate $mutationCoordinate -Mode Baseline
    $mismatchedLoadAreaPlan.LoadAreaCommand = $mismatchedLoadAreaPlan.LoadAreaCommand.Replace(' true', ' false')
    Assert-ThrowsLike {
        Start-BdsFixtureLoadArea `
            -Handle $baselineHandle `
            -Plan $mismatchedLoadAreaPlan `
            -RunDirectory $baselineRunDirectory `
            -SettleMilliseconds 0 `
            -WaitForLoadArea { throw 'mismatched plan attempted a second BDS command' }
    } 'BDS handle already owns a different exact ticking-area plan:*' 'baseline reused an active ticking area for a non-identical plan'
    Assert-Equal 1 @((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) | Where-Object { $_ -ceq $baselineForestPlan.LoadAreaCommand }).Count 'mismatched-plan rejection issued a second ticking-area add'

    $failedForestInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $failedForestHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $failedForestInput }
    }
    $failedForestRunDirectory = Join-Path $TempRoot 'forest rejected results run'
    New-Item -ItemType Directory -Path $failedForestRunDirectory | Out-Null
    $failedForestLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $failedForestLines[0] = '[2026-07-11 12:00:00:000 ERROR] Cannot place blocks outside of the world'
    Assert-ThrowsLike {
        Publish-FullViewTeleport `
            -Handle $failedForestHandle `
            -Plan $fullViewForestPlan `
            -RunDirectory $failedForestRunDirectory `
            -PreloadSettleMilliseconds 0 `
            -WaitForLoadArea {
                param($Handle, $Marker, $TimeoutSeconds)
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
            } `
            -WaitForFence {
                param($Handle, $Marker, $TimeoutSeconds)
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                    -SkippedLines $failedForestLines
            }
    } 'BDS fixture command failed:*outside of the world*' 'forest publisher did not fail closed on the live-observed outside-world result'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $failedForestRunDirectory 'visual-fixture-ready.json'))) 'failed forest published a fixture manifest'
    Assert-True (Test-Path -LiteralPath (Join-Path $failedForestRunDirectory 'fixture-command-stdout.json') -PathType Leaf) 'failed forest did not preserve its exact live stdout interval'
    Assert-True (-not $failedForestInput.ToString().Contains($fullViewForestPlan.TeleportCommand)) 'failed forest teleported after a rejected fixture command'
    Assert-Equal $fullViewForestPlan.LoadAreaName $failedForestHandle.ActiveTickingArea.Name 'failed forest lost cleanup ownership for its active ticking area'
    $failedForestEvents = @(Get-Content -LiteralPath (Join-Path $failedForestRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'load_area_ready' (@($failedForestEvents | ForEach-Object { $_.event }) -join ',') 'failed forest claimed fixture command completion or publication'

    $leafGalleryInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $leafGalleryHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $leafGalleryInput }
    }
    $leafGalleryRunDirectory = Join-Path $TempRoot 'leaf gallery validated run'
    New-Item -ItemType Directory -Path $leafGalleryRunDirectory | Out-Null
    $leafGalleryPublication = Publish-VisualFixture `
        -Handle $leafGalleryHandle `
        -Plan $leafFrontPlan `
        -RunDirectory $leafGalleryRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                -SkippedLines (New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands)
        }
    Assert-True (Test-Path -LiteralPath $leafGalleryPublication.Path -PathType Leaf) 'leaf gallery did not publish after every command result succeeded'
    $leafGalleryStdoutEvidence = Get-Content -Raw -LiteralPath (Join-Path $leafGalleryRunDirectory 'fixture-command-stdout.json') | ConvertFrom-Json
    Assert-Equal $leafFrontPlan.FixtureCommands.Count @($leafGalleryStdoutEvidence.skipped_lines).Count 'leaf gallery stdout artifact lost an exact result line'
    Assert-Equal 'players online:' $leafGalleryStdoutEvidence.marker 'leaf gallery stdout artifact lost its fence marker'
    Assert-Equal ($leafFrontPlan.Commands -join [Environment]::NewLine) $leafGalleryInput.ToString().TrimEnd("`r", "`n") 'leaf gallery result validation changed command/fence/teleport order'

    $failedGalleryInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $failedGalleryHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $failedGalleryInput }
    }
    $failedGalleryRunDirectory = Join-Path $TempRoot 'leaf gallery rejected results run'
    New-Item -ItemType Directory -Path $failedGalleryRunDirectory | Out-Null
    $failedGalleryLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands)
    $failedGalleryLines[0] = '[2026-07-11 12:00:00:000 ERROR] No blocks filled'
    Assert-ThrowsLike {
        Publish-VisualFixture `
            -Handle $failedGalleryHandle `
            -Plan $leafFrontPlan `
            -RunDirectory $failedGalleryRunDirectory `
            -SettleMilliseconds 0 `
            -WaitForFence {
                param($Handle, $Marker, $TimeoutSeconds)
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                    -SkippedLines $failedGalleryLines
            }
    } 'BDS fixture command failed:*No blocks filled*' 'leaf gallery publisher did not fail closed on an ERROR result'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $failedGalleryRunDirectory 'visual-fixture-ready.json'))) 'failed leaf gallery published a fixture manifest'
    Assert-True (-not $failedGalleryInput.ToString().Contains($leafFrontPlan.TeleportCommand)) 'failed leaf gallery teleported after a rejected fixture command'

    $cleanupResult = Remove-BdsTickingArea `
        -Handle $forestHandle `
        -RunDirectory $forestRunDirectory `
        -WaitForAck {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line 'NO LOG FILE! - [2026-07-11 12:00:00:000 INFO] Removed ticking area(s)'
        }
    Assert-Equal $fullViewForestPlan.CleanupCommand $cleanupResult.command 'ticking-area cleanup issued the wrong command'
    $activeTickingAreaProperty = $forestHandle.PSObject.Properties['ActiveTickingArea']
    Assert-True ($null -eq $activeTickingAreaProperty -or $null -eq $activeTickingAreaProperty.Value) 'acknowledged ticking-area cleanup left active ownership state'
    Assert-Equal $fullViewForestPlan.CleanupCommand ((Get-Content -LiteralPath (Join-Path $forestRunDirectory 'bds.console.log'))[-1]) 'cleanup command was not persisted in the BDS console log'
    $cleanupEvents = @(Get-Content -LiteralPath (Join-Path $forestRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'load_area_removed' $cleanupEvents[-1].event 'cleanup acknowledgement was not recorded as the final forest event'

    $failedCleanupInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $failedCleanupHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $failedCleanupInput }
    }
    $failedCleanupRunDirectory = Join-Path $TempRoot 'ticking area cleanup failure run'
    New-Item -ItemType Directory -Path $failedCleanupRunDirectory | Out-Null
    $null = Start-BdsFixtureLoadArea `
        -Handle $failedCleanupHandle `
        -Plan $fullViewForestPlan `
        -RunDirectory $failedCleanupRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForLoadArea {
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
        }
    Assert-ThrowsLike {
        Remove-BdsTickingArea `
            -Handle $failedCleanupHandle `
            -RunDirectory $failedCleanupRunDirectory `
            -WaitForAck {
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] Removed ticking areas'
            }
    } 'invalid ticking-area cleanup acknowledgement:*' 'cleanup accepted a non-exact acknowledgement'
    Assert-Equal $fullViewForestPlan.LoadAreaName $failedCleanupHandle.ActiveTickingArea.Name 'failed cleanup discarded active ticking-area ownership state'
    $failedCleanupEvents = @(Get-Content -LiteralPath (Join-Path $failedCleanupRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'load_area_ready' (@($failedCleanupEvents | ForEach-Object { $_.event }) -join ',') 'failed cleanup falsely recorded load_area_removed'
    Assert-Equal $fullViewForestPlan.CleanupCommand ($failedCleanupInput.ToString().TrimEnd("`r", "`n").Split([Environment]::NewLine)[-1]) 'failed cleanup did not issue the owned exact removal command'

    $serverPropertiesPath = Join-Path $TempRoot 'server.properties'
    [IO.File]::WriteAllLines(
        $serverPropertiesPath,
        @(
            'server-port=19132'
            'server-portv6=19133'
            'online-mode=true'
            'allow-list=true'
            'enable-lan-visibility=true'
            'gamemode=survival'
            'force-gamemode=false'
            'allow-cheats=false'
            'view-distance=32'
            'player-idle-timeout=30'
            'default-player-permission-level=member'
            'client-side-chunk-generation-enabled=true'
            'server-name=fixture'
            'level-name=Bedrock level'
            'level-seed=unchanged-seed'
        ),
        [Text.UTF8Encoding]::new($false)
    )
    Set-ServerProperties -Path $serverPropertiesPath -Port 20000 -PortV6 20001
    $serverProperties = @([IO.File]::ReadAllLines($serverPropertiesPath))
    foreach ($expectedProperty in @(
        'server-port=20000',
        'server-portv6=20001',
        'online-mode=false',
        'allow-list=false',
        'enable-lan-visibility=false',
        'gamemode=creative',
        'force-gamemode=true',
        'allow-cheats=true',
        'view-distance=16',
        'player-idle-timeout=0',
        'default-player-permission-level=operator',
        'client-side-chunk-generation-enabled=false',
        'server-name=fixture',
        'level-name=Bedrock level',
        'level-seed=unchanged-seed'
    )) {
        Assert-True ($serverProperties -contains $expectedProperty) "missing rewritten property: $expectedProperty"
    }
    $duplicateAcceptancePropertyPath = Join-Path $TempRoot 'duplicate-acceptance-property.properties'
    [IO.File]::WriteAllLines(
        $duplicateAcceptancePropertyPath,
        @($serverProperties) + 'client-side-chunk-generation-enabled=true',
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike {
        Set-ServerProperties -Path $duplicateAcceptancePropertyPath -Port 20002 -PortV6 20003
    } 'server.properties must contain exactly one client-side-chunk-generation-enabled entry' 'duplicate client-side terrain generation setting was silently accepted'

    $worldIdentitySource = Join-Path $TempRoot 'world identity source'
    $worldIdentitySourceReverse = Join-Path $TempRoot 'world identity source reverse'
    foreach ($identityRoot in @($worldIdentitySource, $worldIdentitySourceReverse)) {
        $identityWorld = Join-Path $identityRoot 'worlds\Bedrock level'
        New-Item -ItemType Directory -Path (Join-Path $identityWorld 'db') -Force | Out-Null
        [IO.File]::WriteAllLines(
            (Join-Path $identityRoot 'server.properties'),
