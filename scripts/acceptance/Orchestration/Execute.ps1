$script:AcceptanceExecutionPhase = {
    try {
        New-Item -ItemType Directory -Path $RunDirectory -Force | Out-Null
        if ($isLeafEvidence) {
            $sourceWorldIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $BdsDir -AllowMissingWorld
        }
        $repoCommit = (& git -C $ProjectRoot rev-parse HEAD).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw 'failed to resolve repository commit'
        }
        $metadata = [ordered]@{
            status = 'preparing'
            started_utc = [DateTime]::UtcNow.ToString('o')
            repo_commit = $repoCommit
            pinned_gophertunnel_commit = $PinnedGophertunnelCommit
            pinned_valentine_commit = $PinnedValentineCommit
            bds_source = $BdsDir
            bds_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $BdsSourceExecutable).Hash.ToLowerInvariant()
            duration_seconds = $DurationSeconds
            build_app_command = if ($SkipClientBuild) { $null } else { 'cargo build --release -p bedrock-client --locked' }
            build_profile = 'release'
            client_executable = $AppExecutable
            skip_client_build = [bool]$SkipClientBuild
            use_vsync = -not [bool]$NoVsync
            no_vsync_ab = [bool]$NoVsync
            requested_present_mode = if ($NoVsync) { 'Immediate' } else { 'Fifo' }
            effective_present_mode = $null
            present_mode_proven = $false
            steady_resource_trigger = $EffectiveSteadyResourceTrigger
            build_core_command = Format-ResolvedCommand 'go' @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core')
            bds_command = $BdsCommand
            core_command = $CoreCommand
            app_command = $AppCommand
            machine = $env:COMPUTERNAME
            operating_system = Get-OptionalCimValue 'Win32_OperatingSystem' 'Caption'
            cpu = Get-OptionalCimValue 'Win32_Processor' 'Name'
            gpu = Get-OptionalCimValue 'Win32_VideoController' 'Name'
            display = Get-OptionalCimValue 'Win32_VideoController' 'VideoModeDescription'
        }
        if ($null -ne $sourceWorldIdentity) {
            $metadata['source_world_identity'] = $sourceWorldIdentity
        }
        if ($AcceptanceBoundParameters.ContainsKey('Assets')) {
            $metadata['assets'] = $Assets
            $metadata['assets_sha256'] = $AssetBlobSha256
        }
        if ($VisualFixturePose -ne 'None') {
            $metadata['visual_fixture_pose'] = $VisualFixturePose
        }
        if ($isCrossCropGallery) {
            $crossCropGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose
                state_set_sha256 = $CrossCropCoverage.state_set_sha256
                state_count = $CrossCropCoverage.state_count
            }
            $metadata['cross_crop_gallery'] = [pscustomobject][ordered]@{
                arguments = $crossCropGalleryArguments
                arguments_sha256 = Get-CanonicalObjectHash -Value $crossCropGalleryArguments
                coverage_evidence = [pscustomobject][ordered]@{
                    schema = $CrossCropCoverage.schema
                    state_set_sha256 = $CrossCropCoverage.state_set_sha256
                    state_count = $CrossCropCoverage.state_count
                    cross_state_count = $CrossCropCoverage.cross_state_count
                    crop_state_count = $CrossCropCoverage.crop_state_count
                    diagnostic_cross = $CrossCropCoverage.diagnostic_cross
                    diagnostic_crop = $CrossCropCoverage.diagnostic_crop
                }
                artifact_identity = [pscustomobject][ordered]@{
                    assets = $Assets
                    assets_sha256 = $CrossCropCoverage.assets_sha256
                    registry = $BlockRegistryPath
                    registry_sha256 = $CrossCropCoverage.registry_sha256
                    registry_protocol = $CrossCropCoverage.registry_protocol
                    compiler_schema = $CrossCropCoverage.compiler_schema
                }
            }
        }
        if ($isAquaticGallery) {
            $aquaticGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose
                state_set_sha256 = $AquaticCoverage.state_set_sha256
                state_count = $AquaticCoverage.state_count
            }
            $metadata['aquatic_gallery'] = [pscustomobject][ordered]@{
                arguments = $aquaticGalleryArguments
                arguments_sha256 = Get-CanonicalObjectHash -Value $aquaticGalleryArguments
                coverage_evidence = [pscustomobject][ordered]@{
                    schema = $AquaticCoverage.schema
                    state_set_sha256 = $AquaticCoverage.state_set_sha256
                    state_count = $AquaticCoverage.state_count
                    seagrass_state_count = $AquaticCoverage.seagrass_state_count
                    kelp_state_count = $AquaticCoverage.kelp_state_count
                    diagnostic_seagrass_kelp = $AquaticCoverage.diagnostic_seagrass_kelp
                }
                artifact_identity = [pscustomobject][ordered]@{
                    assets = $Assets
                    assets_sha256 = $AquaticCoverage.assets_sha256
                    registry = $BlockRegistryPath
                    registry_sha256 = $AquaticCoverage.registry_sha256
                    registry_protocol = $AquaticCoverage.registry_protocol
                    compiler_schema = $AquaticCoverage.compiler_schema
                }
            }
        }
        if ($isFlowerBedGallery) {
            $flowerBedGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose
                state_set_sha256 = $FlowerBedCoverage.state_set_sha256
                state_count = $FlowerBedCoverage.state_count
            }
            $metadata['flowerbed_gallery'] = [pscustomobject][ordered]@{
                arguments = $flowerBedGalleryArguments
                arguments_sha256 = Get-CanonicalObjectHash -Value $flowerBedGalleryArguments
                coverage_evidence = [pscustomobject][ordered]@{
                    schema = $FlowerBedCoverage.schema
                    state_set_sha256 = $FlowerBedCoverage.state_set_sha256
                    state_count = $FlowerBedCoverage.state_count
                }
                artifact_identity = [pscustomobject][ordered]@{
                    registry = $BlockRegistryPath
                    registry_sha256 = $FlowerBedCoverage.registry_sha256
                    registry_protocol = $FlowerBedCoverage.registry_protocol
                }
            }
        }
        if ($isSlabStairGallery) {
            $slabStairGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose; state_set_sha256 = $SlabStairCoverage.state_set_sha256
                slab_state_count = $SlabStairCoverage.slab_state_count; stair_state_count = $SlabStairCoverage.stair_state_count
            }
            $metadata['slab_stair_gallery'] = [pscustomobject][ordered]@{
                arguments = $slabStairGalleryArguments
                arguments_sha256 = Get-CanonicalObjectHash -Value $slabStairGalleryArguments
                coverage_evidence = [pscustomobject][ordered]@{
                    schema = $SlabStairCoverage.schema
                    state_set_sha256 = $SlabStairCoverage.state_set_sha256
                    state_count = $SlabStairCoverage.state_count
                    slab_state_count = $SlabStairCoverage.slab_state_count
                    stair_state_count = $SlabStairCoverage.stair_state_count
                    stair_name_count = $SlabStairCoverage.stair_name_count
                    diagnostic_slab_stair = $SlabStairCoverage.diagnostic_slab_stair
                }
                artifact_identity = [pscustomobject][ordered]@{
                    assets = $Assets; assets_sha256 = $SlabStairCoverage.assets_sha256
                    registry = $BlockRegistryPath; registry_sha256 = $SlabStairCoverage.registry_sha256
                    registry_protocol = $SlabStairCoverage.registry_protocol; compiler_schema = $SlabStairCoverage.compiler_schema
                }
            }
        }
        if ($isVineGallery) {
            $vineGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose; state_set_sha256 = $VineCoverage.state_set_sha256; state_count = $VineCoverage.state_count
            }
            $metadata['vine_gallery'] = [pscustomobject][ordered]@{
                arguments = $vineGalleryArguments
                arguments_sha256 = Get-CanonicalObjectHash -Value $vineGalleryArguments
                coverage_evidence = [pscustomobject][ordered]@{
                    schema = $VineCoverage.schema; state_set_sha256 = $VineCoverage.state_set_sha256
                    state_count = $VineCoverage.state_count; diagnostic_vine = $VineCoverage.diagnostic_vine
                }
                artifact_identity = [pscustomobject][ordered]@{
                    assets = $Assets; assets_sha256 = $VineCoverage.assets_sha256
                    registry = $BlockRegistryPath; registry_sha256 = $VineCoverage.registry_sha256
                    registry_protocol = $VineCoverage.registry_protocol; compiler_schema = $VineCoverage.compiler_schema
                }
            }
        }
        if ($FullViewTeleportGate) {
            $metadata['full_view_teleport_gate'] = $true
            $metadata['frame_cap'] = 60
        }
        if ($LeafForestBaseline) {
            $metadata['leaf_forest_baseline'] = $true
        }
        if ($LeafForestFullView) {
            $metadata['leaf_forest_full_view'] = $true
        }
        $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        New-Item -ItemType Directory -Path (Split-Path -Parent $RuntimeDirectory) -Force | Out-Null
        New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null
        $lockPath = $RuntimeDirectory + '.lock'
        $lease = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
        $BdsExecutable = Set-StableRuntime -SourceDirectory $BdsDir -RuntimeDirectory $RuntimeDirectory -ExecutableName $BdsExecutableName
        if ($VisualFixturePose -ne 'None' -or $LeafForestBaseline -or $LeafForestFullView) {
            $runtimeWorldIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $RuntimeDirectory -AllowMissingWorld
            if ($null -eq $runtimeWorldIdentity) {
                $bootstrapPortReservation = $null
                $bootstrapPortV6Reservation = $null
                try {
                    $bootstrapPortReservation = New-ReservedUdpPort
                    $bootstrapPortV6Reservation = New-ReservedUdpPort
                    $bootstrapPort = $bootstrapPortReservation.Port
                    Set-ServerProperties `
                        -Path (Join-Path $RuntimeDirectory 'server.properties') `
                        -Port $bootstrapPort `
                        -PortV6 $bootstrapPortV6Reservation.Port
                    $bootstrapPortReservation.Client.Close()
                    $bootstrapPortReservation = $null
                    $bootstrapPortV6Reservation.Client.Close()
                    $bootstrapPortV6Reservation = $null
                    $bdsHandle = Start-LoggedProcess `
                        -Executable $BdsExecutable `
                        -Arguments $BdsArguments `
                        -WorkingDirectory $RuntimeDirectory `
                        -StdoutPath (Join-Path $RunDirectory 'bds-bootstrap.stdout.log') `
                        -StderrPath (Join-Path $RunDirectory 'bds-bootstrap.stderr.log')
                    $rakNetUnconnectedPong = ${function:Test-RakNetUnconnectedPong}
                    if ($null -eq $rakNetUnconnectedPong) {
                        throw 'RakNet readiness helper was not loaded'
                    }
                    $bootstrapReadinessProbe = {
                        & $rakNetUnconnectedPong `
                            -Address '127.0.0.1' `
                            -Port $bootstrapPort `
                            -TimeoutMilliseconds 500
                    }.GetNewClosure()
                    $null = Wait-ProcessOutputMarker `
                        -Handle $bdsHandle `
                        -Marker 'Server started.' `
                        -TimeoutSeconds 120 `
                        -ReadinessProbe $bootstrapReadinessProbe
                    Stop-BoundedProcess `
                        -Handle $bdsHandle `
                        -Kind 'bds' `
                        -BdsConsoleLogPath (Join-Path $RunDirectory 'bds-bootstrap.console.log')
                    Complete-ProcessLogs $bdsHandle
                    $bdsHandle = $null
                }
                finally {
                    foreach ($reservation in @($bootstrapPortReservation, $bootstrapPortV6Reservation)) {
                        if ($null -ne $reservation) {
                            $reservation.Client.Close()
                        }
                    }
                }
                $runtimeWorldIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $RuntimeDirectory
                $metadata['runtime_world_bootstrapped'] = $true
            }
            $metadata['runtime_world_identity'] = $runtimeWorldIdentity
            $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        }
        if ($hasClientExecutable) {
            Assert-FileHashUnchanged -Path $AppExecutable -ExpectedSha256 $PrebuiltClientSha256 -Label 'prebuilt client after BDS runtime setup'
        }
        $portReservation = New-ReservedUdpPort
        $portV6Reservation = New-ReservedUdpPort
        $bdsPort = $portReservation.Port
        $Upstream = "127.0.0.1:$bdsPort"
        Set-ServerProperties -Path (Join-Path $RuntimeDirectory 'server.properties') -Port $bdsPort -PortV6 $portV6Reservation.Port
        $CoreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Upstream)
        $BdsCommand = Format-ResolvedCommand $BdsExecutable $BdsArguments
        $CoreCommand = Format-ResolvedCommand $CoreExecutable $CoreArguments
        $AppCommand = Format-ResolvedCommand $AppExecutable $AppArguments
        Write-Output "BDS_COMMAND=$BdsCommand"
        Write-Output "CORE_COMMAND=$CoreCommand"
        Write-Output "APP_COMMAND=$AppCommand"
        $metadata['status'] = 'building'
        $metadata['bds_command'] = $BdsCommand
        $metadata['core_command'] = $CoreCommand
        $metadata['app_command'] = $AppCommand
        $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        if (-not $SkipClientBuild) {
            Invoke-CheckedBuild -Executable 'cargo' -Arguments @('build', '--release', '-p', 'bedrock-client', '--locked') -LogPath (Join-Path $RunDirectory 'build-app.log') -WorkingDirectory $ProjectRoot
        }
        if (-not (Test-Path -LiteralPath $AppExecutable -PathType Leaf)) {
            throw "client executable was not available after build selection: $AppExecutable"
        }
        $metadata['client_executable_sha256'] = (Get-FileHash -Algorithm SHA256 -LiteralPath $AppExecutable).Hash.ToLowerInvariant()
        Invoke-CheckedBuild -Executable 'go' -Arguments @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core') -LogPath (Join-Path $RunDirectory 'build-core.log') -WorkingDirectory $ProjectRoot
        if ($hasClientExecutable) {
            Assert-FileHashUnchanged -Path $AppExecutable -ExpectedSha256 $PrebuiltClientSha256 -Label 'prebuilt client after generated builds'
        }
        $metadata['status'] = 'launching'
        $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        $portReservation.Client.Close()
        $portReservation = $null
        $portV6Reservation.Client.Close()
        $portV6Reservation = $null
        $bdsHandle = Start-LoggedProcess -Executable $BdsExecutable -Arguments $BdsArguments -WorkingDirectory $RuntimeDirectory -StdoutPath (Join-Path $RunDirectory 'bds.stdout.log') -StderrPath (Join-Path $RunDirectory 'bds.stderr.log')
        $rakNetUnconnectedPong = ${function:Test-RakNetUnconnectedPong}
        if ($null -eq $rakNetUnconnectedPong) {
            throw 'RakNet readiness helper was not loaded'
        }
        $bdsReadinessProbe = {
            & $rakNetUnconnectedPong `
                -Address '127.0.0.1' `
                -Port $bdsPort `
                -TimeoutMilliseconds 500
        }.GetNewClosure()
        $null = Wait-ProcessOutputMarker `
            -Handle $bdsHandle `
            -Marker 'Server started.' `
            -TimeoutSeconds 120 `
            -ReadinessProbe $bdsReadinessProbe
        $coreHandle = Start-LoggedProcess -Executable $CoreExecutable -Arguments $CoreArguments -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'core.stdout.log') -StderrPath (Join-Path $RunDirectory 'core.stderr.log')
        $endpointPath = Join-Path $SocketDirectory 'game.addr'
        $endpointDeadline = [DateTime]::UtcNow.AddSeconds(30)
        while (-not (Test-Path -LiteralPath $endpointPath -PathType Leaf)) {
            if ($coreHandle.Process.HasExited) {
                throw "core exited before endpoint publication with code $($coreHandle.Process.ExitCode)"
            }
            if ([DateTime]::UtcNow -ge $endpointDeadline) {
                throw "timed out waiting for core endpoint: $endpointPath"
            }
            Start-Sleep -Milliseconds 100
        }
        $appHandle = Start-LoggedProcess -Executable $AppExecutable -Arguments $AppArguments -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'app.stdout.log') -StderrPath (Join-Path $RunDirectory 'app.stderr.log')
        $worldReadyMarkerLine = $null
        if ($isModelWitnessGallery) {
            $galleryAnchorMarkerEvidence = Wait-ProcessOutputMarker `
                -Handle $appHandle `
                -Marker 'RUST_MCBE_GALLERY_ANCHOR_READY ' `
                -TimeoutSeconds 180 `
                -PassThruEvidence
            $galleryAnchor = ConvertFrom-GalleryAnchorReadyMarker -Line ([string]$galleryAnchorMarkerEvidence.Line)
            $coordinate = @($galleryAnchor.coordinate)
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'gallery_anchor_ready' -Fields ([ordered]@{
                coordinate = $coordinate -join ','
                visible = [bool]$galleryAnchor.visible
                stdout_line = [uint64]$galleryAnchorMarkerEvidence.LineNumber
            })
        }
        else {
            $coordinateMarker = Wait-ProcessOutputMarker -Handle $appHandle -Marker 'RUST_MCBE_MUTATION_COORDINATE=' -TimeoutSeconds 180
            $worldReadyMarkerLine = Wait-ProcessOutputMarker -Handle $appHandle -Marker 'RUST_MCBE_WORLD_READY ' -TimeoutSeconds 180
            if ($coordinateMarker -notmatch '^RUST_MCBE_MUTATION_COORDINATE=(-?\d+),(-?\d+),(-?\d+)$') {
                throw "invalid mutation marker: $coordinateMarker"
            }
            $coordinate = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
        }
        $activeMutationCoordinate = if ($FullViewTeleportGate) { $null } else { @($coordinate) }
        $blocks = @('minecraft:gold_block', 'minecraft:diamond_block')
        $blockIndex = 0
        if ($LeafForestBaseline) {
            $baselineSourceMutationCommand = Publish-BaselineSourceMutation `
                -Handle $bdsHandle `
                -Coordinate $coordinate `
                -RunDirectory $RunDirectory
            $baselineForestPlan = New-LeafForestPlan -MutationCoordinate $coordinate -Mode Baseline
            Set-BdsSourceWorldIdentityOnPlan -Plan $baselineForestPlan -Identity $sourceWorldIdentity -RuntimeIdentity $runtimeWorldIdentity
            $null = Start-BdsFixtureLoadArea `
                -Handle $bdsHandle `
                -Plan $baselineForestPlan `
                -RunDirectory $RunDirectory `
                -SettleMilliseconds 0
            $activeMutationCoordinate = $null
            $blockIndex = 1
            $metadata['baseline_source_mutation_command'] = $baselineSourceMutationCommand
        }
        if ($EffectiveSteadyResourceTrigger -ceq 'WorldReady') {
            $steadyTriggerEvidence = New-SteadyResourceTriggerEvidence `
                -Kind WorldReady `
                -WorldReadyMarker $worldReadyMarkerLine
            $resourceDocument = Measure-SteadyResources `
                -ClientHandle $appHandle `
                -CoreHandle $coreHandle `
                -RunDirectory $RunDirectory `
                -Trigger $steadyTriggerEvidence `
                -DurationSeconds 30
            Assert-SteadyResourceArtifact `
                -Path $steadyResourceArtifactPath `
                -ExpectedTrigger $steadyTriggerEvidence
            $metadata['steady_resources'] = $resourceDocument.summary
            if ($LeafForestBaseline) {
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'source_mutation_observation_window_completed' -Fields ([ordered]@{
                    duration_seconds = 30
                    command = $baselineSourceMutationCommand
                })
            }
        }
        if ($FullViewTeleportGate) {
            $teleportPlan = if ($LeafForestFullView) {
                New-FullViewTeleportPlan -MutationCoordinate $coordinate -LeafForest
            }
            else {
                New-FullViewTeleportPlan -MutationCoordinate $coordinate
            }
            Set-BdsSourceWorldIdentityOnPlan -Plan $teleportPlan -Identity $sourceWorldIdentity -RuntimeIdentity $runtimeWorldIdentity
            $fixturePublication = Publish-FullViewTeleport `
                -Handle $bdsHandle `
                -Plan $teleportPlan `
                -RunDirectory $RunDirectory
            if ($LeafForestFullView) {
                $metadata['fixture_manifest'] = $fixturePublication.Path
                $metadata['fixture_manifest_sha256'] = $fixturePublication.ManifestSha256
                $metadata['fixture_layout_hash'] = $fixturePublication.LayoutHash
            }
            $targetChunkX = [int][Math]::Floor([double]$teleportPlan.Target.x / 16.0)
            $targetChunkZ = [int][Math]::Floor([double]$teleportPlan.Target.z / 16.0)
            $expectedTargetCohort = '{0}:{1}:{2}:16' -f 0, $targetChunkX, $targetChunkZ
            if ($LeafForestFullView) {
                $movePlayerIngressMarkerEvidence = Wait-ProcessOutputMarker `
                    -Handle $appHandle `
                    -Marker 'RUST_MCBE_MOVE_PLAYER_INGRESS ' `
                    -TimeoutSeconds 180 `
                    -PassThruEvidence
                $movePlayerIngressEvidence = ConvertFrom-MovePlayerIngressMarker -Line $movePlayerIngressMarkerEvidence.Line
                $ingressFloorX = [int][Math]::Floor([double]$movePlayerIngressEvidence.position[0])
                $ingressFloorZ = [int][Math]::Floor([double]$movePlayerIngressEvidence.position[2])
                if ($ingressFloorX -ne [int]$teleportPlan.Target.x -or
                    $ingressFloorZ -ne [int]$teleportPlan.Target.z) {
                    throw "MovePlayer ingress did not match planned far camera X/Z: expected=$($teleportPlan.Target.x),$($teleportPlan.Target.z) actual_floor=$ingressFloorX,$ingressFloorZ"
                }
                $metadata['move_player_ingress'] = [ordered]@{
                    sequence = [uint64]$movePlayerIngressEvidence.sequence
                    position = @($movePlayerIngressEvidence.position)
                    stdout_line = [uint64]$movePlayerIngressMarkerEvidence.LineNumber
                }
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'move_player_ingress' -Fields ([ordered]@{
                    sequence = [uint64]$movePlayerIngressEvidence.sequence
                    position = @($movePlayerIngressEvidence.position)
                    stdout_line = [uint64]$movePlayerIngressMarkerEvidence.LineNumber
                })
            }
            $teleportMarkerOutputEvidence = Wait-ProcessOutputMarker `
                -Handle $appHandle `
                -Marker 'RUST_MCBE_TELEPORT_SETTLED ' `
                -TimeoutSeconds 180 `
                -PassThruEvidence
            $teleportMarkerLine = $teleportMarkerOutputEvidence.Line
            if ($LeafForestFullView -and
                [uint64]$teleportMarkerOutputEvidence.LineNumber -le [uint64]$movePlayerIngressMarkerEvidence.LineNumber) {
                throw "teleport settle marker did not follow MovePlayer ingress in stdout: ingress=$($movePlayerIngressMarkerEvidence.LineNumber) teleport=$($teleportMarkerOutputEvidence.LineNumber)"
            }
            $teleportMarkerEvidence = ConvertFrom-FullViewSettleMarker `
                -Line $teleportMarkerLine `
                -Kind Teleport
            $teleportMilliseconds = [double]$teleportMarkerEvidence.ms
            if ($LeafForestFullView) {
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'binding_teleport_settled' -Fields ([ordered]@{
                    target = [string]$teleportMarkerEvidence.target
                    view_generation = [uint64]$teleportMarkerEvidence.view_generation
                    stdout_line = [uint64]$teleportMarkerOutputEvidence.LineNumber
                })
            }
            $forcedRemeshMarkerOutputEvidence = Wait-ProcessOutputMarker `
                -Handle $appHandle `
                -Marker 'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED ' `
                -TimeoutSeconds 30 `
                -PassThruEvidence
            if ([uint64]$forcedRemeshMarkerOutputEvidence.LineNumber -le [uint64]$teleportMarkerOutputEvidence.LineNumber) {
                throw "forced-remesh marker did not follow teleport settle in stdout: teleport=$($teleportMarkerOutputEvidence.LineNumber) remesh=$($forcedRemeshMarkerOutputEvidence.LineNumber)"
            }
            $forcedRemeshMarkerLine = $forcedRemeshMarkerOutputEvidence.Line
            $forcedRemeshMarkerEvidence = ConvertFrom-FullViewSettleMarker `
                -Line $forcedRemeshMarkerLine `
                -Kind ForcedRemesh
            $remeshMilliseconds = [double]$forcedRemeshMarkerEvidence.ms
            if ($LeafForestFullView) {
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'forced_remesh_settled' -Fields ([ordered]@{
                    target = [string]$forcedRemeshMarkerEvidence.target
                    view_generation = [uint64]$forcedRemeshMarkerEvidence.view_generation
                    stdout_line = [uint64]$forcedRemeshMarkerOutputEvidence.LineNumber
                })
                $targetMutationMarkerOutputEvidence = Wait-ProcessOutputMarker `
                    -Handle $appHandle `
                    -Marker 'RUST_MCBE_TARGET_MUTATION_ARMED ' `
                    -TimeoutSeconds 30 `
                    -PassThruEvidence
                if ([uint64]$targetMutationMarkerOutputEvidence.LineNumber -le [uint64]$forcedRemeshMarkerOutputEvidence.LineNumber) {
                    throw "target-mutation marker did not follow forced remesh in stdout: remesh=$($forcedRemeshMarkerOutputEvidence.LineNumber) arm=$($targetMutationMarkerOutputEvidence.LineNumber)"
                }
                $targetMutationMarkerLine = $targetMutationMarkerOutputEvidence.Line
                $targetMutationEvidence = ConvertFrom-TargetMutationArmedMarker -Line $targetMutationMarkerLine
                $expectedSourceMutation = $coordinate -join ','
                $expectedTargetMutation = @(
                    $teleportPlan.TargetMutation.x,
                    $teleportPlan.TargetMutation.y,
                    $teleportPlan.TargetMutation.z
                ) -join ','
                if (($targetMutationEvidence.source -join ',') -cne $expectedSourceMutation) {
                    throw "target mutation source did not match original manifest coordinate: expected=$expectedSourceMutation actual=$($targetMutationEvidence.source -join ',')"
                }
                if (($targetMutationEvidence.target -join ',') -cne $expectedTargetMutation) {
                    throw "target mutation did not match visual fixture manifest: expected=$expectedTargetMutation actual=$($targetMutationEvidence.target -join ',')"
                }
                if ([uint64]$targetMutationEvidence.view_generation -ne [uint64]$forcedRemeshMarkerEvidence.view_generation) {
                    throw "target mutation generation did not match forced-remesh generation: target=$($targetMutationEvidence.view_generation) remesh=$($forcedRemeshMarkerEvidence.view_generation)"
                }
                $activeMutationCoordinate = @($targetMutationEvidence.target)
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'target_mutation_armed' -Fields ([ordered]@{
                    source_mutation = $expectedSourceMutation
                    target_mutation = $expectedTargetMutation
                    view_generation = [uint64]$targetMutationEvidence.view_generation
                    stdout_line = [uint64]$targetMutationMarkerOutputEvidence.LineNumber
                })
                $initialTargetCommand = "setblock $($activeMutationCoordinate[0]) $($activeMutationCoordinate[1]) $($activeMutationCoordinate[2]) $($blocks[0])"
                Write-BdsConsoleCommand `
                    -Handle $bdsHandle `
                    -Command $initialTargetCommand `
                    -LogPath (Join-Path $RunDirectory 'bds.console.log')
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'target_mutation_command' -Fields ([ordered]@{
                    command = $initialTargetCommand
                    block = $blocks[0]
                })
                $blockIndex = 1
            }
            $steadyTriggerEvidence = New-SteadyResourceTriggerEvidence `
                -Kind FullViewPresented `
                -TeleportMarker $teleportMarkerEvidence `
                -ForcedRemeshMarker $forcedRemeshMarkerEvidence
            $resourceDocument = Measure-SteadyResources `
                -ClientHandle $appHandle `
                -CoreHandle $coreHandle `
                -RunDirectory $RunDirectory `
                -Trigger $steadyTriggerEvidence `
                -DurationSeconds 30
            $metadata['teleport_settle_ms'] = $teleportMilliseconds
            $metadata['forced_full_view_remesh_ms'] = $remeshMilliseconds
            $metadata['steady_resources'] = $resourceDocument.summary
            $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        }
        elseif ($LeafForestBaseline) {
            $fixturePlan = $baselineForestPlan
            if ($null -eq $fixturePlan) {
                throw 'baseline forest plan was not prepared before the WorldReady observation window'
            }
            $fixturePublication = Publish-VisualFixture `
                -Handle $bdsHandle `
                -Plan $fixturePlan `
                -RunDirectory $RunDirectory
            $metadata['fixture_manifest'] = $fixturePublication.Path
            $metadata['fixture_manifest_sha256'] = $fixturePublication.ManifestSha256
            $metadata['fixture_layout_hash'] = $fixturePublication.LayoutHash
        }
        elseif ($VisualFixturePose -ne 'None') {
            $fixturePlan = New-VisualFixturePlan `
                -MutationCoordinate $coordinate `
                -Pose $VisualFixturePose `
                -RegistryPath $BlockRegistryPath `
                -AssetsPath $Assets
            Set-BdsSourceWorldIdentityOnPlan -Plan $fixturePlan -Identity $sourceWorldIdentity -RuntimeIdentity $runtimeWorldIdentity
            $fixturePublication = Publish-VisualFixture `
                -Handle $bdsHandle `
                -Plan $fixturePlan `
                -RunDirectory $RunDirectory `
                -AppHandle $appHandle
            if ($isDeterministicGallery) {
                $steadyTriggerEvidence = New-SteadyResourceTriggerEvidence `
                    -Kind VisualFixtureReady `
                    -FixturePublication $fixturePublication
                $resourceDocument = Measure-SteadyResources `
                    -ClientHandle $appHandle `
                    -CoreHandle $coreHandle `
                    -RunDirectory $RunDirectory `
                    -Trigger $steadyTriggerEvidence `
                    -DurationSeconds 30
                Assert-SteadyResourceArtifact `
                    -Path $steadyResourceArtifactPath `
                    -ExpectedTrigger $steadyTriggerEvidence
                $metadata['steady_resources'] = $resourceDocument.summary
                $metadata['fixture_manifest'] = $fixturePublication.Path
                $metadata['fixture_manifest_sha256'] = $fixturePublication.ManifestSha256
                $metadata['fixture_layout_hash'] = $fixturePublication.LayoutHash
            }
        }
        $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        $nextMutation = [DateTime]::UtcNow
        $appDeadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds + 90)
        while (-not $appHandle.Process.HasExited) {
            if ([DateTime]::UtcNow -ge $appDeadline) {
                throw "app exceeded acceptance deadline of $($DurationSeconds + 90) seconds"
            }
            if ($null -ne $activeMutationCoordinate -and [DateTime]::UtcNow -ge $nextMutation) {
                $command = "setblock $($activeMutationCoordinate[0]) $($activeMutationCoordinate[1]) $($activeMutationCoordinate[2]) $($blocks[$blockIndex])"
                Write-BdsConsoleCommand `
                    -Handle $bdsHandle `
                    -Command $command `
                    -LogPath (Join-Path $RunDirectory 'bds.console.log')
                $blockIndex = ($blockIndex + 1) % $blocks.Count
                $nextMutation = [DateTime]::UtcNow.AddSeconds(2)
            }
            Start-Sleep -Milliseconds 100
        }
        if ($appHandle.Process.ExitCode -ne 0) {
            throw "app exited with code $($appHandle.Process.ExitCode)"
        }
        $runtimeMetadata = Read-AcceptanceRuntimeMetadata -Path $appHandle.StdoutPath
        $expectedPresentMode = if ($NoVsync) { 'Immediate' } else { 'Fifo' }
        $publicationSnapshot = Read-WorldPublicationSnapshots `
            -Path $appHandle.StdoutPath `
            -ExpectedBuildProfile 'release' `
            -ExpectedPresentMode $expectedPresentMode
        $metadata['build_profile'] = [string]$runtimeMetadata.build_profile
        $metadata['requested_present_mode'] = [string]$runtimeMetadata.requested_present_mode
        $metadata['effective_present_mode'] = [string]$runtimeMetadata.effective_present_mode
        $metadata['present_mode_proven'] = [bool]$runtimeMetadata.present_mode_proven
        $metadata['graphics_backend'] = [string]$runtimeMetadata.backend
        $metadata['graphics_adapter'] = [string]$runtimeMetadata.adapter
        $metadata['graphics_driver'] = [string]$runtimeMetadata.driver
        $metadata['graphics_driver_info'] = [string]$runtimeMetadata.driver_info
        if ([string]$runtimeMetadata.build_profile -cne 'release') {
            throw "acceptance requires a release client, observed $($runtimeMetadata.build_profile)"
        }
        if ([string]$runtimeMetadata.requested_present_mode -cne $expectedPresentMode -or
            [string]$runtimeMetadata.effective_present_mode -cne $expectedPresentMode) {
            throw "acceptance present mode mismatch: expected=$expectedPresentMode requested=$($runtimeMetadata.requested_present_mode) effective=$($runtimeMetadata.effective_present_mode)"
        }
        foreach ($field in @('build_profile', 'requested_present_mode', 'effective_present_mode', 'backend', 'adapter', 'driver', 'driver_info')) {
            if ([string]$publicationSnapshot.$field -cne [string]$runtimeMetadata.$field) {
                throw "world publication snapshot did not match runtime metadata at $field"
            }
        }
        $metadata['draw_mode'] = [string]$publicationSnapshot.draw_mode
        if ($hasClientExecutable) {
            Assert-FileHashUnchanged -Path $AppExecutable -ExpectedSha256 $PrebuiltClientSha256 -Label 'prebuilt client after acceptance run'
        }
        if ($FullViewTeleportGate) {
            $fullViewMetricArguments = @{
                Path = $CanonicalMetrics
                RequireFullViewTeleport = $true
                TeleportMarker = $teleportMarkerEvidence
                ForcedRemeshMarker = $forcedRemeshMarkerEvidence
                ExpectedTargetCohort = $expectedTargetCohort
                SteadyResourceArtifactPath = $steadyResourceArtifactPath
            }
            if ($LeafForestFullView) {
                $fullViewMetricArguments['ExpectedMutationCoordinate'] = @($activeMutationCoordinate)
                $fullViewMetricArguments['RequireAssets'] = $true
                $fullViewMetricArguments['ExpectedAssetBlobSha256'] = $AssetBlobSha256
            }
            $metrics = Assert-AcceptanceMetrics @fullViewMetricArguments
        }
        else {
            if ($LeafForestBaseline) {
                $metrics = Assert-AcceptanceMetrics `
                    -Path $CanonicalMetrics `
                    -OpaqueBaselineSchema `
                    -ExpectedMutationCoordinate $coordinate `
                    -RequireAssets `
                    -ExpectedAssetBlobSha256 $AssetBlobSha256
            }
            elseif ($isWaterGallery) {
                $metrics = Assert-AcceptanceMetrics `
                    -Path $CanonicalMetrics `
                    -RequireAssets `
                    -ExpectedAssetBlobSha256 $AssetBlobSha256 `
                    -RequireTransparentWater `
                    -MinimumTransparentWaterDistinctTintCount ([uint64]$fixturePlan.Manifest.relative_layout.biome_tint_evidence.minimum_rendered_distinct_tint_count) `
                    -MaximumP99FrameMilliseconds ([double]$fixturePlan.Manifest.performance.maximum_p99_frame_ms)
            }
            elseif ($isLeafEvidence) {
                $metrics = Assert-AcceptanceMetrics `
                    -Path $CanonicalMetrics `
                    -RequireAssets `
                    -ExpectedAssetBlobSha256 $AssetBlobSha256
            }
            else {
                $metrics = Assert-AcceptanceMetrics -Path $CanonicalMetrics
            }
        }
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $RunDirectory 'validated-metrics.json') -Encoding UTF8
    }
    catch {
        $runFailure = $_
        if ($null -ne $metadata) {
            try {
                Write-AcceptanceMetadataStatus -Metadata $metadata -RunDirectory $RunDirectory -Status failed -Failure $_.Exception.Message
            }
            catch {
                Write-Warning "failed to update failure metadata: $_"
            }
        }
        throw
    }
    finally {
        $cleanupErrors = [Collections.Generic.List[string]]::new()
        foreach ($child in @(
            [pscustomobject]@{ Handle = $appHandle; Kind = 'app' },
            [pscustomobject]@{ Handle = $coreHandle; Kind = 'core' }
        )) {
            try {
                Stop-BoundedProcess `
                    -Handle $child.Handle `
                    -Kind $child.Kind `
                    -BdsConsoleLogPath (Join-Path $RunDirectory 'bds.console.log')
            }
            catch {
                $cleanupErrors.Add("stop $($child.Kind): $($_.Exception.Message)")
                Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
            }
        }
        if ($null -ne $bdsHandle) {
            try {
                $loadAreaCleanup = Remove-BdsTickingArea `
                    -Handle $bdsHandle `
                    -RunDirectory $RunDirectory
                if ($null -ne $loadAreaCleanup -and $null -ne $metadata) {
                    $metadata['load_area_cleanup'] = $loadAreaCleanup
                    $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
                }
            }
            catch {
                $cleanupErrors.Add("remove BDS ticking area: $($_.Exception.Message)")
                Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
            }
        }
        try {
            Stop-BoundedProcess `
                -Handle $bdsHandle `
                -Kind 'bds' `
                -BdsConsoleLogPath (Join-Path $RunDirectory 'bds.console.log')
        }
        catch {
            $cleanupErrors.Add("stop bds: $($_.Exception.Message)")
            Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
        }
        if ($null -ne $sourceWorldIdentity) {
            try {
                Assert-BdsSourceWorldIdentityUnchanged `
                    -Expected $sourceWorldIdentity `
                    -SourceDirectory $BdsDir
                if ($null -ne $metadata) {
                    $metadata['source_world_identity_verified_after_run'] = $true
                    $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
                }
            }
            catch {
                $cleanupErrors.Add("verify BDS source world identity: $($_.Exception.Message)")
                Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
            }
        }
        if ($hasClientExecutable) {
            try {
                Assert-FileHashUnchanged -Path $AppExecutable -ExpectedSha256 $PrebuiltClientSha256 -Label 'prebuilt client after cleanup'
            }
            catch {
                $cleanupErrors.Add("verify prebuilt client: $($_.Exception.Message)")
                Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
            }
        }
        foreach ($handle in @($appHandle, $coreHandle, $bdsHandle)) {
            try {
                Complete-ProcessLogs $handle
            }
            catch {
                $cleanupErrors.Add("complete child logs: $($_.Exception.Message)")
                Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
            }
        }
        foreach ($reservation in @($portReservation, $portV6Reservation)) {
            if ($null -ne $reservation) {
                try {
                    $reservation.Client.Close()
                }
                catch {
                    $cleanupErrors.Add("close UDP reservation: $($_.Exception.Message)")
                }
            }
        }
        if ($null -ne $lease) {
            try {
                $lease.Dispose()
            }
            catch {
                $cleanupErrors.Add("release BDS runtime lease: $($_.Exception.Message)")
            }
        }
        try {
            if (Test-Path -LiteralPath $CanonicalMetrics -PathType Leaf) {
                New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null
                Copy-Item -LiteralPath $CanonicalMetrics -Destination $MetricsOut -Force
            }
        }
        catch {
            $cleanupErrors.Add("copy requested metrics: $($_.Exception.Message)")
        }
        try {
            if (($null -ne $runFailure -or $cleanupErrors.Count -ne 0) -and (Test-Path -LiteralPath $RunDirectory -PathType Container)) {
                $failureDetails = @()
                if ($null -ne $runFailure) {
                    $failureDetails += ($runFailure | Out-String)
                }
                $failureDetails += $cleanupErrors
                ($failureDetails -join [Environment]::NewLine) | Set-Content -LiteralPath (Join-Path $RunDirectory 'failure.txt') -Encoding UTF8
            }
        }
        catch {
            Write-Warning "failed to write cleanup report: $_"
        }
        if ($null -eq $runFailure -and $cleanupErrors.Count -ne 0) {
            if ($null -ne $metadata) {
                try {
                    Write-AcceptanceMetadataStatus -Metadata $metadata -RunDirectory $RunDirectory -Status failed -Failure "cleanup: $($cleanupErrors -join '; ')"
                }
                catch {
                    Write-Warning "failed to update cleanup-failure metadata: $_"
                }
            }
            throw "acceptance cleanup failed: $($cleanupErrors -join '; ')"
        }
    }
    if ($null -eq $metrics) {
        throw 'acceptance metrics were unavailable after successful finalization'
    }
    $metadata['status'] = 'passed'
    $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
    $metadata | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
    Write-Output "ACCEPTANCE_ARTIFACTS=$RunDirectory"; Write-Output "ACCEPTANCE_P99_FRAME_MS=$($metrics.p99_frame_ms)"
}
