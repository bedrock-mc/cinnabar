    New-Item -ItemType Directory -Path $BdsDir -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $BdsDir 'bedrock_server.exe') -Value 'fixture' -NoNewline
    Set-Content -LiteralPath $Assets -Value 'assets fixture' -NoNewline
    New-TestCrossCropAssets -RegistryPath $BlockRegistry -Path $CrossCropAssets
    New-TestAquaticAssets -RegistryPath $BlockRegistry -Path $AquaticAssets
    New-TestSlabStairAssets -RegistryPath $BlockRegistry -Path $SlabStairAssets
    New-Item -ItemType Directory -Path (Split-Path -Parent $PrebuiltClient) -Force | Out-Null
    Set-Content -LiteralPath $PrebuiltClient -Value 'pinned opaque client fixture' -NoNewline
    $prebuiltHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) "pre-existing dry-run artifact prevents an immutability assertion: $DryRunDirectory"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $BdsDir 'worlds'))) 'generic dry-run fixture unexpectedly contains a pre-created BDS world'

    $success = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($success.ExitCode -eq 0) "dry-run failed: $($success.Output -join [Environment]::NewLine)"
    $commands = @($success.Output | Where-Object { $_ -match '^(BDS|CORE|APP)_COMMAND=' })
    Assert-True ($commands.Count -eq 3) "expected exactly three commands, got $($commands.Count)"
    Assert-True ($commands[0] -match '^BDS_COMMAND=') 'BDS command was not first'
    Assert-True ($commands[1] -match '^CORE_COMMAND=') 'core command was not second'
    Assert-True ($commands[2] -match '^APP_COMMAND=') 'app command was not third'
    Assert-True ($success.Output.Count -eq 6) 'default dry-run output changed'
    Assert-True ($success.Output -contains 'BUILD_PROFILE=release') 'default dry-run did not identify the release profile'
    Assert-True ($success.Output -contains 'REQUESTED_PRESENT_MODE=Fifo') 'default dry-run did not request FIFO'
    Assert-True ($success.Output -contains 'EFFECTIVE_PRESENT_MODE=UNPROVEN') 'default dry-run relabeled requested FIFO as effective without surface proof'
    $expectedRuntimeDirectory = Join-Path (Join-Path $ProjectRoot '.local\bds-runtime') (Split-Path -Leaf $BdsDir)
    $expectedSocketDirectory = Join-Path $DryRunDirectory 'socket'
    $expectedCanonicalMetrics = Join-Path $DryRunDirectory 'app-metrics.json'
    $expectedCommands = @(
        ('BDS_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $expectedRuntimeDirectory 'bedrock_server.exe') `
            -Arguments @()))
        ('CORE_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $ProjectRoot 'target\release\bedrock-core.exe') `
            -Arguments @('-socket-dir', $expectedSocketDirectory, '-upstream', '127.0.0.1:19132')))
        ('APP_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $ProjectRoot 'target\release\bedrock-client.exe') `
            -Arguments @(
                '--socket-dir', $expectedSocketDirectory,
                '--acceptance-seconds', '900',
                '--metrics-out', $expectedCanonicalMetrics,
                '--auto-fly'
            )))
    )
    Assert-Equal `
        ($expectedCommands -join [Environment]::NewLine) `
        ($commands -join [Environment]::NewLine) `
        'default dry-run commands changed'
    foreach ($flag in @('--socket-dir', '--acceptance-seconds 900', '--metrics-out', '--auto-fly')) {
        Assert-True ($commands[2].Contains($flag)) "app command is missing $flag"
    }
    Assert-True (-not $commands[2].Contains('--no-vsync')) 'default acceptance bypassed FIFO'
    Assert-True (-not $commands[2].Contains('--assets')) 'default app command unexpectedly gained --assets'
    Assert-True (-not ($success.Output -match '^VISUAL_FIXTURE_POSE=')) 'default dry-run recorded a fixture pose'
    Assert-True ($commands[0].Contains('"')) 'path containing spaces was not quoted'
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) 'dry-run created its run directory'
    Assert-True (-not (Test-Path -LiteralPath $MetricsOut)) 'dry-run wrote metrics'

    $noVsyncDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun', '-DurationSeconds', '900', '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut, '-NoVsync'
    )
    Assert-True ($noVsyncDryRun.ExitCode -eq 0) "explicit no-vsync dry-run failed: $($noVsyncDryRun.Output -join [Environment]::NewLine)"
    $noVsyncAppCommand = @($noVsyncDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($noVsyncAppCommand[0].Contains('--no-vsync')) 'explicit no-vsync A/B lost its app flag'
    Assert-True ($noVsyncDryRun.Output -contains 'REQUESTED_PRESENT_MODE=Immediate') 'explicit no-vsync request was not recorded'
    Assert-True ($noVsyncDryRun.Output -contains 'EFFECTIVE_PRESENT_MODE=UNPROVEN') 'explicit no-vsync dry-run relabeled the request as effective'

    $sharedRuntimeDirectory = Join-Path $TempRoot 'approved shared BDS runtime'
    $sharedRuntimeDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-BdsRuntimeDirectory', $sharedRuntimeDirectory,
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($sharedRuntimeDryRun.ExitCode -eq 0) "shared-runtime dry-run failed: $($sharedRuntimeDryRun.Output -join [Environment]::NewLine)"
    $sharedRuntimeCommand = @($sharedRuntimeDryRun.Output | Where-Object { $_ -match '^BDS_COMMAND=' })
    Assert-True ($sharedRuntimeCommand.Count -eq 1) "expected one shared-runtime BDS command, got $($sharedRuntimeCommand.Count)"
    Assert-Equal `
        ('BDS_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $sharedRuntimeDirectory 'bedrock_server.exe') `
            -Arguments @())) `
        $sharedRuntimeCommand[0] `
        'explicit shared BDS runtime directory was ignored'
    Assert-True (-not (Test-Path -LiteralPath $sharedRuntimeDirectory)) 'shared-runtime dry-run created its runtime directory'

    $frontDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-VisualFixturePose', 'Front'
    )
    Assert-True ($frontDryRun.ExitCode -eq 0) "front fixture dry-run failed: $($frontDryRun.Output -join [Environment]::NewLine)"
    $frontAppCommand = @($frontDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($frontAppCommand.Count -eq 1) 'front fixture dry-run did not emit one app command'
    Assert-True ($frontAppCommand[0].Contains("--assets `"$((Resolve-Path -LiteralPath $Assets).Path)`"")) 'front fixture app command did not include the resolved assets path'
    Assert-True (-not $frontAppCommand[0].Contains('--auto-fly')) 'front fixture app command retained --auto-fly'
    Assert-True (-not $frontAppCommand[0].Contains('--no-vsync')) 'front fixture bypassed default FIFO'
    Assert-True ($frontDryRun.Output -contains 'VISUAL_FIXTURE_POSE=Front') 'front fixture dry-run did not record its pose'

    $backDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-VisualFixturePose', 'Back'
    )
    Assert-True ($backDryRun.ExitCode -eq 0) "back fixture dry-run failed: $($backDryRun.Output -join [Environment]::NewLine)"
    Assert-True ($backDryRun.Output -contains 'VISUAL_FIXTURE_POSE=Back') 'back fixture dry-run did not record its pose'

    $teleportDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $CrossCropAssets,
        '-FullViewTeleportGate'
    )
    Assert-True ($teleportDryRun.ExitCode -eq 0) "full-view dry-run failed: $($teleportDryRun.Output -join [Environment]::NewLine)"
    $teleportAppCommand = @($teleportDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $teleportAppCommand.Count 'full-view dry-run did not emit one app command'
    Assert-True ($teleportAppCommand[0].Contains('--full-view-teleport-gate')) 'full-view app command omitted its gate flag'
    Assert-True ($teleportAppCommand[0].Contains('--frame-cap 60')) 'full-view app command omitted the deterministic 60fps cap'
    Assert-True (-not $teleportAppCommand[0].Contains('--auto-fly')) 'full-view app command retained auto-fly'
    Assert-True (-not $teleportAppCommand[0].Contains('--no-vsync')) 'full-view app command bypassed its capped presentation mode'
    Assert-True ($teleportDryRun.Output -contains 'FULL_VIEW_TELEPORT_GATE=1') 'full-view dry-run did not record its mode'

    $leafFrontDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-VisualFixturePose', 'LeafGalleryFront',
        '-SteadyResourceTrigger', 'VisualFixtureReady',
        '-UseVsync'
    )
    Assert-True ($leafFrontDryRun.ExitCode -eq 0) "leaf-front dry-run failed: $($leafFrontDryRun.Output -join [Environment]::NewLine)"
    $leafFrontAppCommand = @($leafFrontDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $leafFrontAppCommand.Count 'leaf-front dry-run did not emit one app command'
    Assert-True (-not $leafFrontAppCommand[0].Contains('--auto-fly')) 'leaf-front mode retained auto-fly'
    Assert-True (-not $leafFrontAppCommand[0].Contains('--no-vsync')) 'leaf-front mode bypassed -UseVsync'
    Assert-True (-not $leafFrontAppCommand[0].Contains('--full-view-teleport-gate')) 'leaf-front mode armed the far tracker'
    Assert-True ($leafFrontDryRun.Output -contains 'VISUAL_FIXTURE_POSE=LeafGalleryFront') 'leaf-front dry-run lost its pose'
    Assert-True ($leafFrontDryRun.Output -contains 'STEADY_RESOURCE_TRIGGER=VisualFixtureReady') 'leaf-front dry-run lost its trigger'
    Assert-True ($leafFrontDryRun.Output -contains 'USE_VSYNC=1') 'leaf-front dry-run lost its vsync mode'

    $crossCropDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $CrossCropAssets,
        '-VisualFixturePose', 'CrossCropGalleryFront',
        '-SteadyResourceTrigger', 'VisualFixtureReady',
        '-UseVsync'
    )
    Assert-True ($crossCropDryRun.ExitCode -eq 0) "cross/crop gallery dry-run failed: $($crossCropDryRun.Output -join [Environment]::NewLine)"
    $assetIdentity = (Get-FileHash -Algorithm SHA256 -LiteralPath $CrossCropAssets).Hash.ToLowerInvariant()
    Assert-True ($crossCropDryRun.Output -contains 'VISUAL_FIXTURE_POSE=CrossCropGalleryFront') 'cross/crop dry-run lost its exact gallery argument'
    Assert-True ($crossCropDryRun.Output -contains "CROSS_CROP_GALLERY_ASSETS_SHA256=$assetIdentity") 'cross/crop dry-run did not record exact artifact identity'
    Assert-Equal 1 @($crossCropDryRun.Output | Where-Object { $_ -match '^CROSS_CROP_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count 'cross/crop dry-run did not record deterministic gallery arguments identity'

    $aquaticDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $AquaticAssets,
        '-VisualFixturePose', 'AquaticGalleryFront',
        '-SteadyResourceTrigger', 'VisualFixtureReady',
        '-UseVsync'
    )
    Assert-True ($aquaticDryRun.ExitCode -eq 0) "aquatic gallery dry-run failed: $($aquaticDryRun.Output -join [Environment]::NewLine)"
    $aquaticAssetIdentity = (Get-FileHash -Algorithm SHA256 -LiteralPath $AquaticAssets).Hash.ToLowerInvariant()
    Assert-True ($aquaticDryRun.Output -contains 'VISUAL_FIXTURE_POSE=AquaticGalleryFront') 'aquatic dry-run lost its exact gallery argument'
    Assert-True ($aquaticDryRun.Output -contains "AQUATIC_GALLERY_ASSETS_SHA256=$aquaticAssetIdentity") 'aquatic dry-run did not record exact artifact identity'
    Assert-Equal 1 @($aquaticDryRun.Output | Where-Object { $_ -match '^AQUATIC_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count 'aquatic dry-run did not record deterministic gallery arguments identity'
    $aquaticAppCommand = @($aquaticDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($aquaticAppCommand[0] -notmatch '--require-transparent-presentation') 'non-water aquatic gallery unexpectedly required transparent presentation settle'

    $waterDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $AquaticAssets,
        '-VisualFixturePose', 'WaterGalleryFront',
        '-UseVsync',
        '-SteadyResourceTrigger', 'VisualFixtureReady'
    )
    Assert-True ($waterDryRun.ExitCode -eq 0) "water gallery dry-run failed: $($waterDryRun.Output -join [Environment]::NewLine)"
    $waterAppCommand = @($waterDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($waterAppCommand[0] -match '--require-transparent-presentation') 'water gallery did not opt into bounded transparent presentation settle'
    Assert-True ($waterAppCommand[0] -match '--transparent-witness-request') 'water gallery did not pass its ignored-local transparent witness request path to the app'

    foreach ($flowerBedPose in @('FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite')) {
        $flowerBedDryRun = Invoke-Acceptance -Arguments @(
            '-DryRun',
            '-DurationSeconds', '60',
            '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut,
            '-Assets', $CrossCropAssets,
            '-VisualFixturePose', $flowerBedPose,
            '-UseVsync',
            '-SteadyResourceTrigger', 'VisualFixtureReady'
        )
        Assert-True ($flowerBedDryRun.ExitCode -eq 0) "$flowerBedPose dry-run failed: $($flowerBedDryRun.Output -join [Environment]::NewLine)"
        Assert-True ($flowerBedDryRun.Output -contains "VISUAL_FIXTURE_POSE=$flowerBedPose") "$flowerBedPose dry-run lost its exact pose"
        Assert-Equal 1 @($flowerBedDryRun.Output | Where-Object { $_ -match '^FLOWERBED_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count "$flowerBedPose dry-run did not emit deterministic arguments identity"
    }

    foreach ($slabStairPose in @('SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite')) {
        $slabStairDryRun = Invoke-Acceptance -Arguments @(
            '-DryRun', '-DurationSeconds', '60', '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut, '-Assets', $SlabStairAssets,
            '-VisualFixturePose', $slabStairPose, '-UseVsync',
            '-SteadyResourceTrigger', 'VisualFixtureReady'
        )
        Assert-True ($slabStairDryRun.ExitCode -eq 0) "$slabStairPose dry-run failed: $($slabStairDryRun.Output -join [Environment]::NewLine)"
        Assert-True ($slabStairDryRun.Output -contains "VISUAL_FIXTURE_POSE=$slabStairPose") "$slabStairPose lost exact pose"
        Assert-True ($slabStairDryRun.Output -contains 'STEADY_RESOURCE_TRIGGER=VisualFixtureReady') "$slabStairPose lost trigger"
        Assert-True ($slabStairDryRun.Output -contains 'USE_VSYNC=1') "$slabStairPose lost vsync"
        Assert-Equal 1 @($slabStairDryRun.Output | Where-Object { $_ -match '^SLAB_STAIR_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count "$slabStairPose lost arguments hash"
        Assert-Equal 1 @($slabStairDryRun.Output | Where-Object { $_ -match '^SLAB_STAIR_GALLERY_ASSETS_SHA256=[0-9a-f]{64}$' }).Count "$slabStairPose lost assets hash"
        $slabStairAppCommand = @($slabStairDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
        Assert-True ($slabStairAppCommand[0] -match '--model-witness-request') "$slabStairPose did not pass its ignored-local model witness request path to the app"
    }

    foreach ($vinePose in @('VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')) {
        $vineDryRun = Invoke-Acceptance -Arguments @(
            '-DryRun', '-DurationSeconds', '60', '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut, '-Assets', $SlabStairAssets,
            '-VisualFixturePose', $vinePose, '-UseVsync',
            '-SteadyResourceTrigger', 'VisualFixtureReady'
        )
        Assert-True ($vineDryRun.ExitCode -eq 0) "$vinePose dry-run failed: $($vineDryRun.Output -join [Environment]::NewLine)"
        Assert-True ($vineDryRun.Output -contains "VISUAL_FIXTURE_POSE=$vinePose") "$vinePose lost exact pose"
        Assert-Equal 1 @($vineDryRun.Output | Where-Object { $_ -match '^VINE_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count "$vinePose lost arguments hash"
        Assert-Equal 1 @($vineDryRun.Output | Where-Object { $_ -match '^VINE_GALLERY_ASSETS_SHA256=[0-9a-f]{64}$' }).Count "$vinePose lost assets hash"
        $vineAppCommand = @($vineDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
        Assert-True ($vineAppCommand[0] -match '--model-witness-request') "$vinePose did not pass its ignored-local model witness request path to the app"
    }

    $baselineDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-LeafForestBaseline',
        '-SteadyResourceTrigger', 'WorldReady',
        '-ClientExecutable', $PrebuiltClient,
        '-SkipClientBuild',
        '-UseVsync'
    )
    Assert-True ($baselineDryRun.ExitCode -eq 0) "leaf-forest baseline dry-run failed: $($baselineDryRun.Output -join [Environment]::NewLine)"
    $baselineAppCommand = @($baselineDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $baselineAppCommand.Count 'baseline dry-run did not emit one app command'
    Assert-True ($baselineAppCommand[0].StartsWith('APP_COMMAND=' + (ConvertTo-TestCommandArgument ((Resolve-Path -LiteralPath $PrebuiltClient).Path)))) 'baseline did not select the exact prebuilt executable'
    Assert-True (-not $baselineAppCommand[0].Contains('--auto-fly')) 'baseline retained auto-fly'
    Assert-True (-not $baselineAppCommand[0].Contains('--no-vsync')) 'baseline bypassed -UseVsync'
    Assert-True (-not $baselineAppCommand[0].Contains('--full-view-teleport-gate')) 'baseline armed the far tracker'
    foreach ($marker in @(
        'LEAF_FOREST_BASELINE=1',
        'STEADY_RESOURCE_TRIGGER=WorldReady',
        'SKIP_CLIENT_BUILD=1',
        'USE_VSYNC=1'
    )) {
        Assert-True ($baselineDryRun.Output -contains $marker) "baseline dry-run omitted $marker"
    }
    Assert-Equal $prebuiltHashBefore (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash 'dry-run overwrote the explicit prebuilt client'

    $leafForestFullViewDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-LeafForestFullView',
        '-FullViewTeleportGate',
        '-SteadyResourceTrigger', 'FullViewPresented'
    )
    Assert-True ($leafForestFullViewDryRun.ExitCode -eq 0) "leaf-forest full-view dry-run failed: $($leafForestFullViewDryRun.Output -join [Environment]::NewLine)"
    $leafForestFullViewAppCommand = @($leafForestFullViewDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $leafForestFullViewAppCommand.Count 'leaf-forest full-view emitted the wrong app-command count'
    Assert-True ($leafForestFullViewAppCommand[0].Contains('--full-view-teleport-gate --frame-cap 60')) 'leaf-forest full-view lost the binding capped mode'
    Assert-True (-not $leafForestFullViewAppCommand[0].Contains('--no-vsync')) 'leaf-forest full-view added no-vsync'
    Assert-True ($leafForestFullViewDryRun.Output -contains 'LEAF_FOREST_FULL_VIEW=1') 'leaf-forest full-view lost its mode marker'
    Assert-True ($leafForestFullViewDryRun.Output -contains 'STEADY_RESOURCE_TRIGGER=FullViewPresented') 'leaf-forest full-view lost its trigger marker'

    $invalidLeafModes = @(
        @('-LeafForestBaseline', '-LeafForestFullView', '-FullViewTeleportGate', '-SteadyResourceTrigger', 'WorldReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild', '-UseVsync'),
        @('-LeafForestBaseline', '-SteadyResourceTrigger', 'WorldReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild'),
        @('-LeafForestBaseline', '-SteadyResourceTrigger', 'VisualFixtureReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild', '-UseVsync'),
        @('-LeafForestFullView', '-SteadyResourceTrigger', 'FullViewPresented'),
        @('-LeafForestFullView', '-FullViewTeleportGate', '-SteadyResourceTrigger', 'WorldReady'),
        @('-VisualFixturePose', 'LeafGalleryBack', '-SteadyResourceTrigger', 'VisualFixtureReady'),
        @('-VisualFixturePose', 'LeafGalleryBack', '-SteadyResourceTrigger', 'WorldReady', '-UseVsync'),
        @('-VisualFixturePose', 'LeafGalleryFront', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync'),
        @('-VisualFixturePose', 'leafgalleryfront', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $Assets),
        @('-VisualFixturePose', 'slabstairgallerytop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $SlabStairAssets),
        @('-VisualFixturePose', 'SlabStairGalleryTop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync'),
        @('-VisualFixturePose', 'SlabStairGalleryTop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $Assets),
        @('-VisualFixturePose', 'vinegallerytop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $SlabStairAssets),
        @('-VisualFixturePose', 'VineGalleryTop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync'),
        @('-VisualFixturePose', 'VineGalleryTop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $Assets),
        @('-SteadyResourceTrigger', 'worldready'),
        @('-LeafForestBaseline', '-SteadyResourceTrigger', 'WorldReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild', '-UseVsync'),
        @('-LeafForestFullView', '-FullViewTeleportGate', '-SteadyResourceTrigger', 'FullViewPresented'),
        @('-SkipClientBuild'),
        @('-ClientExecutable', $PrebuiltClient),
        @('-SkipClientBuild', '-ClientExecutable', (Join-Path $TempRoot 'missing-client.exe'))
    )
    foreach ($invalidLeafMode in $invalidLeafModes) {
        $invalidArguments = @(
            '-DryRun',
            '-DurationSeconds', '900',
            '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut
        ) + $invalidLeafMode
        $invalidResult = Invoke-Acceptance -Arguments $invalidArguments
        Assert-True ($invalidResult.ExitCode -ne 0) "invalid leaf/prebuilt mode was accepted: $($invalidLeafMode -join ' ')"
    }

    $conflictingModes = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-FullViewTeleportGate',
        '-VisualFixturePose', 'Front'
    )
    Assert-True ($conflictingModes.ExitCode -ne 0) 'full-view and visual-fixture modes were accepted together'

    $missingAssets = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', (Join-Path $TempRoot 'missing.mcpack')
    )
    Assert-True ($missingAssets.ExitCode -ne 0) 'missing assets file was accepted'

    $directoryAssets = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $BdsDir
    )
    Assert-True ($directoryAssets.ExitCode -ne 0) 'assets directory was accepted as a file'

    $short = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '59',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($short.ExitCode -ne 0) 'duration below 60 seconds was accepted'

    $missing = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', (Join-Path $TempRoot 'missing'),
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($missing.ExitCode -ne 0) 'missing BDS directory was accepted'

    . (Join-Path $ProjectRoot 'scripts\acceptance\Load.ps1')
    $source = Get-AcceptanceCompositeSource -EntryPath $AcceptanceScript
    Assert-True ($source.IndexOf('function ConvertTo-CommandArgument') -lt $source.IndexOf('function Start-LoggedProcess')) 'composite source order changed'
    Assert-True ($source.IndexOf('function Start-LoggedProcess') -lt $source.IndexOf('function Assert-AcceptanceMetrics')) 'composite source order changed'
    $identityFunctionStart = $source.IndexOf('function Get-BdsSourceWorldIdentity {', [StringComparison]::Ordinal)
    $identityFunctionEnd = $source.IndexOf('function Assert-BdsSourceWorldIdentityUnchanged {', $identityFunctionStart, [StringComparison]::Ordinal)
    $identityFunctionSource = $source.Substring($identityFunctionStart, $identityFunctionEnd - $identityFunctionStart)
    Assert-True (-not $identityFunctionSource.Contains('Test-Path -LiteralPath $worldsPath')) 'optional source identity still treats Test-Path false as proof that worlds is absent'
    Assert-True (-not $identityFunctionSource.Contains('Test-Path -LiteralPath $worldPath')) 'optional source identity still treats Test-Path false as proof that the configured world is absent'
    Assert-True ($source.Contains('CopyToAsync')) 'child logs are not streamed directly to files'
    $runtimeSnapshotIndex = $source.IndexOf('$runtimeWorldIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $RuntimeDirectory', [StringComparison]::Ordinal)
    $bdsLaunchIndex = $source.IndexOf('$bdsHandle = Start-LoggedProcess -Executable $BdsExecutable', [StringComparison]::Ordinal)
    Assert-True ($runtimeSnapshotIndex -ge 0 -and $runtimeSnapshotIndex -lt $bdsLaunchIndex) 'runtime world identity is captured after BDS can lock level.dat'
    Assert-True ($source.Contains('Get-BdsSourceWorldIdentity -SourceDirectory $RuntimeDirectory -AllowMissingWorld')) 'fresh BDS runtime world detection is not optional before bootstrap'
    Assert-True ($source.Contains("`$metadata['runtime_world_bootstrapped'] = `$true")) 'fresh BDS runtime bootstrap is not recorded in metadata'
    Assert-True ($source.Contains("'bds-bootstrap.stdout.log'")) 'fresh BDS runtime has no isolated bootstrap process evidence'
    Assert-True ($source.Contains('[IO.FileOptions]::WriteThrough')) 'child log files are not write-through'
    Assert-True (-not $source.Contains('ReadToEndAsync')) 'child logs are retained in memory'
    Assert-True ($source.Contains('-WorkingDirectory $ProjectRoot')) 'builds are not rooted at the project directory'
    Assert-True ($source.Contains("'bbe6cfdeed39713c2b20103a1294e609d5841615'")) 'gophertunnel metadata commit is not the repository pin'
    Assert-True ($source.Contains("'6cd8087fc3f0b500e41708a8afc94a0fa3291525'")) 'Valentine metadata commit is not the compiled fork revision'
    Assert-True ($source.Contains('Assert-ProtocolDependencyProvenance')) 'acceptance metadata does not detect Cargo/provenance drift'
    Assert-True (-not $source.Contains("'^RUST_MCBE_TELEPORT_SETTLED ms=")) 'live teleport path still assumes ms precedes target'
    Assert-True (-not $source.Contains("'^RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED ms=")) 'live forced-remesh path still assumes ms precedes target'
    Assert-True ($source.Contains('TeleportMarker = $teleportMarkerEvidence')) 'live metrics validation does not receive parsed teleport evidence'
    Assert-True ($source.Contains('ForcedRemeshMarker = $forcedRemeshMarkerEvidence')) 'live metrics validation does not receive parsed forced-remesh evidence'
    Assert-True ($source.Contains('ExpectedTargetCohort = $expectedTargetCohort')) 'live metrics validation does not receive the planned target cohort'
    Assert-True ($source.Contains('SteadyResourceArtifactPath = $steadyResourceArtifactPath')) 'live metrics validation does not require the steady-resource artifact'
    Assert-True (([regex]::Matches($source, '-TeleportMarker \$teleportMarkerEvidence')).Count -ge 1) 'steady-resource sampler does not receive teleport trigger evidence'
    Assert-True (([regex]::Matches($source, '-ForcedRemeshMarker \$forcedRemeshMarkerEvidence')).Count -ge 1) 'steady-resource sampler does not receive forced-remesh trigger evidence'
    Assert-True ($source.Contains('if (-not $SkipClientBuild)')) 'prebuilt client mode does not skip the app build'
    Assert-True ($source.Contains('RUST_MCBE_TARGET_MUTATION_ARMED ')) 'live harness does not wait for the target-mutation arming marker'
    Assert-True ($source.Contains('ConvertFrom-TargetMutationArmedMarker')) 'live harness does not parse target-mutation evidence'
    Assert-True ($source.Contains('RUST_MCBE_MOVE_PLAYER_INGRESS ')) 'live harness does not wait for binding MovePlayer ingress evidence'
    Assert-True ($source.Contains('ConvertFrom-MovePlayerIngressMarker')) 'live harness does not parse binding MovePlayer ingress evidence'
    Assert-True ($source.Contains('-PassThruEvidence')) 'binding marker waits do not retain stdout positions'
    Assert-True ($source.Contains('Write-AcceptanceEvent')) 'live harness does not persist ordered fixture/teleport events'
    Assert-True `
        ([regex]::IsMatch(
            $source,
            'if \(\$isLeafEvidence\) \{\s*\$sourceWorldIdentity = Get-BdsSourceWorldIdentity',
            [Text.RegularExpressions.RegexOptions]::CultureInvariant
        )) `
        'generic live smoke runs still require a pre-created source world identity'
    Assert-True ($source.Contains('Move-Item -LiteralPath $temporaryPath -Destination $Path')) 'fixture manifest publication is not an atomic sibling rename'
    Assert-True ($source.Contains('$cpuPercent = 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount)')) 'steady CPU normalization formula changed'
    Assert-True (([regex]::Matches($source, '\.Refresh\(\)')).Count -ge 4) 'resource sampling does not refresh both process handles before/during sampling'
    $baselineSourceMutationIndex = $source.IndexOf('$baselineSourceMutationCommand = Publish-BaselineSourceMutation', [StringComparison]::Ordinal)
    $resourceSamplingIndex = $source.IndexOf('$resourceDocument = Measure-SteadyResources', [StringComparison]::Ordinal)
    $baselineForestPlanIndex = $source.IndexOf(
        '$baselineForestPlan = New-LeafForestPlan -MutationCoordinate $coordinate -Mode Baseline',
        [Math]::Max(0, $baselineSourceMutationIndex),
        [StringComparison]::Ordinal
    )
    $baselinePreloadIndex = $source.IndexOf(
        '$null = Start-BdsFixtureLoadArea',
        [Math]::Max(0, $baselineForestPlanIndex),
        [StringComparison]::Ordinal
    )
    $baselineForestPublishIndex = $source.IndexOf('$fixturePlan = $baselineForestPlan', [StringComparison]::Ordinal)
    Assert-True ($baselineSourceMutationIndex -ge 0 -and $resourceSamplingIndex -gt $baselineSourceMutationIndex) 'baseline did not issue its source mutation immediately before the WorldReady observation window'
    Assert-True ($baselineForestPlanIndex -gt $baselineSourceMutationIndex) 'baseline did not derive its exact far preload plan after source mutation'
    Assert-True ($baselinePreloadIndex -gt $baselineForestPlanIndex -and $baselinePreloadIndex -lt $resourceSamplingIndex) 'baseline did not start its ticking-area preload before the 30s WorldReady observation window'
    Assert-True `
        ([regex]::IsMatch(
            $source.Substring($baselinePreloadIndex, $resourceSamplingIndex - $baselinePreloadIndex),
            'Start-BdsFixtureLoadArea[\s\S]*?-SettleMilliseconds 0',
            [Text.RegularExpressions.RegexOptions]::CultureInvariant
        )) `
        'baseline added an extra preload settle instead of using the existing 30s WorldReady sample'
    Assert-True ($baselineForestPublishIndex -gt $resourceSamplingIndex) 'baseline far forest could publish before the source mutation observation window'
    $metricsValidationIndex = $source.IndexOf('$metrics = Assert-AcceptanceMetrics', [StringComparison]::Ordinal)
    Assert-True ($resourceSamplingIndex -ge 0 -and $metricsValidationIndex -gt $resourceSamplingIndex) 'full-view metrics SLA validation can run before steady-resource sampling/artifact publication'
    $cleanupFailureThrowIndex = $source.LastIndexOf('throw "acceptance cleanup failed:', [StringComparison]::Ordinal)
    $passedStatusIndex = $source.LastIndexOf('$metadata[''status''] = ''passed''', [StringComparison]::Ordinal)
    $successArtifactOutputIndex = $source.LastIndexOf('Write-Output "ACCEPTANCE_ARTIFACTS=', [StringComparison]::Ordinal)
    Assert-True ($cleanupFailureThrowIndex -ge 0) 'main finalizer omitted its cleanup-failure barrier'
    Assert-True ($passedStatusIndex -gt $cleanupFailureThrowIndex) 'metadata could claim passed before required cleanup/source verification succeeded'
    Assert-True ($successArtifactOutputIndex -gt $cleanupFailureThrowIndex) 'acceptance success markers could be emitted before required cleanup/source verification succeeded'

    $env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY = '1'
    try {
        . $AcceptanceScript `
            -DryRun `
            -DurationSeconds 900 `
            -BdsDir $BdsDir `
            -MetricsOut $MetricsOut `
            -Assets $Assets `
            -VisualFixturePose Front
    }
    finally {
        Remove-Item Env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -ErrorAction SilentlyContinue
    }

    $runtimeMetadataMarker = ConvertFrom-AcceptanceRuntimeMetadataMarker -Line 'RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA={"build_profile":"release","requested_present_mode":"Fifo","effective_present_mode":"Fifo","present_mode_proven":true,"backend":"Dx12","adapter":"Test Adapter","driver":"test-driver","driver_info":"1.2.3"}'
    Assert-Equal 'release' $runtimeMetadataMarker.build_profile 'runtime metadata lost build profile'
    Assert-Equal 'Fifo' $runtimeMetadataMarker.effective_present_mode 'runtime metadata lost effective present mode'
    Assert-Equal 'Dx12' $runtimeMetadataMarker.backend 'runtime metadata lost backend'
    Assert-Equal 'Test Adapter' $runtimeMetadataMarker.adapter 'runtime metadata lost adapter'
    Assert-Equal 'test-driver' $runtimeMetadataMarker.driver 'runtime metadata lost driver'
    Assert-Equal $true $runtimeMetadataMarker.present_mode_proven 'runtime metadata lost authoritative present-mode proof'
    Assert-Throws {
        ConvertFrom-AcceptanceRuntimeMetadataMarker -Line 'RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA={"build_profile":"release","requested_present_mode":"Immediate","effective_present_mode":"Immediate","backend":"Dx12","adapter":"Test Adapter","driver":"test-driver","driver_info":"1.2.3"}'
    } 'runtime metadata without present-mode proof was accepted'
    Assert-Throws {
        ConvertFrom-AcceptanceRuntimeMetadataMarker -Line 'RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA={"build_profile":"release","requested_present_mode":"Immediate","effective_present_mode":"Immediate","present_mode_proven":false,"backend":"Dx12","adapter":"Test Adapter","driver":"test-driver","driver_info":"1.2.3"}'
    } 'runtime metadata with unproven present mode was accepted'
    Assert-Throws {
        ConvertFrom-AcceptanceRuntimeMetadataMarker -Line 'RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA={"build_profile":"debug"}'
    } 'incomplete runtime metadata was accepted'

    $publicationFields = [ordered]@{
        accepted_light_jobs = [uint64]::MaxValue
        noop_light_jobs = 2
        value_changed_light_jobs = 3
        provenance_only_light_jobs = 5
        light_mesh_invalidations = 7
        stale_light_jobs = 11
        stale_mesh_jobs = 13
        queued_decode_jobs = 17
        in_flight_decode_jobs = 19
        pending_light_jobs = 23
        in_flight_light_jobs = 29
        pending_mesh_jobs = 31
        in_flight_mesh_jobs = 37
        max_decode_queue_wait_ms = 41.0
        max_light_queue_wait_ms = 43.0
        max_mesh_queue_wait_ms = 47.0
        max_decode_worker_ms = 53.0
        max_light_worker_ms = 59.0
        max_mesh_worker_ms = 61.0
        upload_queue_items = 67
        upload_queue_bytes = 71
        gpu_upload_bytes = 73
        frame_generation = 79
        pose_generation = 83
        view_generation = 89
        draw_mode = 'Direct'
        build_profile = 'release'
        requested_present_mode = 'Fifo'
        effective_present_mode = 'Fifo'
        present_mode_proven = $true
        backend = 'Dx12'
        adapter = 'Test Adapter'
        driver = 'test-driver'
        driver_info = '1.2.3'
    }
    $publicationLine = 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=' + ($publicationFields | ConvertTo-Json -Compress)
    $publicationSnapshot = ConvertFrom-WorldPublicationSnapshotMarker `
        -Line $publicationLine `
        -ExpectedBuildProfile 'release' `
        -ExpectedPresentMode 'Fifo'
    Assert-Equal 'Direct' $publicationSnapshot.draw_mode 'publication snapshot lost exact draw mode'
    Assert-Equal ([uint64]::MaxValue) ([uint64]$publicationSnapshot.accepted_light_jobs) 'publication snapshot lost saturating counter range'
    Assert-Equal 41.0 ([double]$publicationSnapshot.max_decode_queue_wait_ms) 'publication snapshot lost decode queue wait'
    Assert-Equal 53.0 ([double]$publicationSnapshot.max_decode_worker_ms) 'publication snapshot conflated queue wait and worker duration'
    $stringIntegerLine = $publicationLine.Replace('"queued_decode_jobs":17', '"queued_decode_jobs":"17"')
    Assert-Throws {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $stringIntegerLine -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } 'publication snapshot accepted a JSON string for an integer field'
    $stringDurationLine = $publicationLine.Replace('"max_decode_queue_wait_ms":41', '"max_decode_queue_wait_ms":"41.0"')
    Assert-True ($stringDurationLine.Contains('"max_decode_queue_wait_ms":"41.0"')) 'duration wrong-type fixture did not mutate the JSON token'
    Assert-Throws {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $stringDurationLine -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } 'publication snapshot accepted a JSON string for a duration field'
    $stringBooleanLine = $publicationLine.Replace('"present_mode_proven":true', '"present_mode_proven":"true"')
    Assert-Throws {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $stringBooleanLine -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } 'publication snapshot accepted a JSON string for a boolean field'

    $missingPublicationFields = [ordered]@{}
    foreach ($entry in $publicationFields.GetEnumerator()) {
        if ($entry.Key -cne 'max_mesh_queue_wait_ms') {
            $missingPublicationFields[$entry.Key] = $entry.Value
        }
    }
    Assert-ThrowsLike {
        ConvertFrom-WorldPublicationSnapshotMarker -Line ('RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=' + ($missingPublicationFields | ConvertTo-Json -Compress)) -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } '*missing=max_mesh_queue_wait_ms*' 'publication snapshot accepted a missing stage field'
    $duplicatePublicationLine = $publicationLine.Replace('"draw_mode":"Direct"', '"draw_mode":"Direct","draw_mode":"MultiDrawIndirect"')
    Assert-ThrowsLike {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $duplicatePublicationLine -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } '*duplicate field*draw_mode*' 'publication snapshot accepted a duplicate draw identity'
    Assert-Throws {
        ConvertFrom-WorldPublicationSnapshotMarker -Line 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT={' -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } 'publication snapshot accepted malformed JSON'
    Assert-ThrowsLike {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $publicationLine -ExpectedBuildProfile debug -ExpectedPresentMode Fifo
    } '*build profile mismatch*' 'release and debug publication rows were conflated'
    Assert-ThrowsLike {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $publicationLine -ExpectedBuildProfile release -ExpectedPresentMode Immediate
    } '*present mode mismatch*' 'FIFO and Immediate publication rows were conflated'
    $conflatedDrawLine = $publicationLine.Replace('"draw_mode":"Direct"', '"draw_mode":"Direct|MultiDrawIndirect"')
    Assert-ThrowsLike {
        ConvertFrom-WorldPublicationSnapshotMarker -Line $conflatedDrawLine -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } '*invalid draw_mode*' 'Direct and MDI were conflated in one publication row'

    $publicationLog = Join-Path $TempRoot 'publication-snapshots.log'
    $mdiLine = $publicationLine.Replace('"draw_mode":"Direct"', '"draw_mode":"MultiDrawIndirect"')
    Set-Content -LiteralPath $publicationLog -Value @($publicationLine, $mdiLine)
    Assert-ThrowsLike {
        Read-WorldPublicationSnapshots -Path $publicationLog -ExpectedBuildProfile release -ExpectedPresentMode Fifo
    } '*draw mode changed*' 'periodic publication rows silently changed draw mode'
    Assert-True (-not $source.Contains('PresentMode Immediate requested but not available. Falling back to Fifo')) 'acceptance still inferred effective mode from a suppressible INFO log'

