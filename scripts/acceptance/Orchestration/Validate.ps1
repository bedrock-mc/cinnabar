$script:AcceptanceValidationPhase = {

    
    if ($DurationSeconds -lt 60) {
        throw 'DurationSeconds must be at least 60'
    }
    $canonicalVisualFixturePoses = @('None', 'Front', 'Back', 'LeafGalleryFront', 'LeafGalleryBack', 'CrossCropGalleryFront', 'CrossCropGalleryBack', 'AquaticGalleryFront', 'AquaticGalleryBack', 'WaterGalleryFront', 'WaterGalleryBack', 'FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite', 'SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite', 'VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')
    if (-not ($canonicalVisualFixturePoses -ccontains $VisualFixturePose)) {
        throw "VisualFixturePose must use canonical casing: $VisualFixturePose"
    }
    $isLeafGallery = $VisualFixturePose -in @('LeafGalleryFront', 'LeafGalleryBack')
    $isCrossCropGallery = $VisualFixturePose -in @('CrossCropGalleryFront', 'CrossCropGalleryBack')
    $isAquaticGallery = $VisualFixturePose -in @('AquaticGalleryFront', 'AquaticGalleryBack')
    $isWaterGallery = $VisualFixturePose -in @('WaterGalleryFront', 'WaterGalleryBack')
    $isFlowerBedGallery = $VisualFixturePose -in @('FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite')
    $isSlabStairGallery = $VisualFixturePose -in @('SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite')
    $isVineGallery = $VisualFixturePose -in @('VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')
    $isModelWitnessGallery = $isSlabStairGallery -or $isVineGallery
    $isDeterministicGallery = $isLeafGallery -or $isCrossCropGallery -or $isAquaticGallery -or $isWaterGallery -or $isFlowerBedGallery -or $isModelWitnessGallery
    $isLeafEvidence = $isDeterministicGallery -or $LeafForestBaseline -or $LeafForestFullView
    $hasClientExecutable = $AcceptanceBoundParameters.ContainsKey('ClientExecutable')
    if ($AcceptanceBoundParameters.ContainsKey('SteadyResourceTrigger') -and
        -not (@('WorldReady', 'VisualFixtureReady', 'FullViewPresented') -ccontains $SteadyResourceTrigger)) {
        throw "invalid SteadyResourceTrigger: $SteadyResourceTrigger"
    }
    if ([bool]$SkipClientBuild -ne $hasClientExecutable) {
        throw 'ClientExecutable and SkipClientBuild must be supplied together'
    }
    if ($LeafForestBaseline -and $LeafForestFullView) {
        throw 'LeafForestBaseline and LeafForestFullView cannot be combined'
    }
    if ($FullViewTeleportGate -and $VisualFixturePose -ne 'None') {
        throw 'FullViewTeleportGate and VisualFixturePose cannot be combined'
    }
    if (($LeafForestBaseline -or $LeafForestFullView) -and $VisualFixturePose -ne 'None') {
        throw 'leaf forest modes and VisualFixturePose cannot be combined'
    }
    if ($LeafForestBaseline -and $FullViewTeleportGate) {
        throw 'LeafForestBaseline cannot arm FullViewTeleportGate'
    }
    if ($LeafForestFullView -and -not $FullViewTeleportGate) {
        throw 'LeafForestFullView requires FullViewTeleportGate'
    }
    if ($LeafForestBaseline) {
        if (-not $SkipClientBuild) {
            throw 'LeafForestBaseline requires ClientExecutable and SkipClientBuild'
        }
        if ([string]$SteadyResourceTrigger -cne 'WorldReady') {
            throw 'LeafForestBaseline requires SteadyResourceTrigger WorldReady'
        }
    }
    if ($isDeterministicGallery) {
        if ([string]$SteadyResourceTrigger -cne 'VisualFixtureReady') {
            throw 'deterministic gallery modes require SteadyResourceTrigger VisualFixtureReady'
        }
    }
    if ($UseVsync -and $NoVsync) {
        throw 'UseVsync and NoVsync cannot be combined'
    }
    if ($LeafForestFullView -and [string]$SteadyResourceTrigger -cne 'FullViewPresented') {
        throw 'LeafForestFullView requires SteadyResourceTrigger FullViewPresented'
    }
    if ($AcceptanceBoundParameters.ContainsKey('SteadyResourceTrigger')) {
        if ($SteadyResourceTrigger -ceq 'WorldReady' -and -not $LeafForestBaseline) {
            throw 'SteadyResourceTrigger WorldReady is reserved for LeafForestBaseline'
        }
        if ($SteadyResourceTrigger -ceq 'VisualFixtureReady' -and -not $isDeterministicGallery) {
            throw 'SteadyResourceTrigger VisualFixtureReady requires a deterministic gallery pose'
        }
        if ($SteadyResourceTrigger -ceq 'FullViewPresented' -and -not $FullViewTeleportGate) {
            throw 'SteadyResourceTrigger FullViewPresented requires FullViewTeleportGate'
        }
    }
    $EffectiveSteadyResourceTrigger = if ($AcceptanceBoundParameters.ContainsKey('SteadyResourceTrigger')) {
        [string]$SteadyResourceTrigger
    }
    elseif ($FullViewTeleportGate) {
        'FullViewPresented'
    }
    else {
        $null
    }
    if ([string]::IsNullOrWhiteSpace($MetricsOut)) {
        throw 'MetricsOut must not be empty'
    }
    if ($isLeafEvidence -and -not $AcceptanceBoundParameters.ContainsKey('Assets')) {
        throw 'leaf evidence modes require the pinned Assets blob'
    }
    if (-not (Test-Path -LiteralPath $BdsDir -PathType Container)) {
        throw "BDS directory does not exist: $BdsDir"
    }
    $BdsDir = (Resolve-Path -LiteralPath $BdsDir).Path
    if ($AcceptanceBoundParameters.ContainsKey('Assets')) {
        if (-not (Test-Path -LiteralPath $Assets -PathType Leaf)) {
            throw "assets file does not exist: $Assets"
        }
        $Assets = (Resolve-Path -LiteralPath $Assets).Path
    }
    $AssetBlobSha256 = if ($AcceptanceBoundParameters.ContainsKey('Assets')) {
        (Get-FileHash -Algorithm SHA256 -LiteralPath $Assets).Hash.ToLowerInvariant()
    }
    else {
        $null
    }
    if ($hasClientExecutable) {
        if (-not (Test-Path -LiteralPath $ClientExecutable -PathType Leaf)) {
            throw "client executable does not exist: $ClientExecutable"
        }
        $ClientExecutable = (Resolve-Path -LiteralPath $ClientExecutable).Path
    }
    $PrebuiltClientSha256 = if ($hasClientExecutable) {
        (Get-FileHash -Algorithm SHA256 -LiteralPath $ClientExecutable).Hash.ToLowerInvariant()
    }
    else {
        $null
    }
    $BdsExecutableName = 'bedrock_server.exe'
    $BdsSourceExecutable = Join-Path $BdsDir $BdsExecutableName
    if (-not (Test-Path -LiteralPath $BdsSourceExecutable -PathType Leaf)) {
        throw "BDS executable does not exist: $BdsSourceExecutable"
    }
    
    $ProjectRoot = (Resolve-Path (Join-Path $script:AcceptanceEntryRoot '..')).Path
    $null = Assert-ProtocolDependencyProvenance `
        -ProjectRoot $ProjectRoot `
        -ExpectedForkRevision $PinnedValentineForkCommit `
        -ExpectedUpstreamRevision $PinnedValentineUpstreamCommit `
        -ExpectedLicenseSha256 $PinnedValentineLicenseSha256
    $BlockRegistryPath = Join-Path $ProjectRoot 'crates\assets\data\block-registry-v1001.bin'
    $CrossCropCoverage = if ($isCrossCropGallery) {
        Get-CrossCropCoverageEvidence -RegistryPath $BlockRegistryPath -AssetsPath $Assets
    }
    else {
        $null
    }
    $AquaticCoverage = if ($isAquaticGallery) {
        Get-AquaticCoverageEvidence -RegistryPath $BlockRegistryPath -AssetsPath $Assets
    }
    else {
        $null
    }
    $FlowerBedCoverage = if ($isFlowerBedGallery) {
        Get-FlowerBedCoverageEvidence -RegistryPath $BlockRegistryPath
    }
    else {
        $null
    }
    $SlabStairCoverage = if ($isSlabStairGallery) {
        Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistryPath -AssetsPath $Assets
    }
    else {
        $null
    }
    $VineCoverage = if ($isVineGallery) {
        Get-VineCoverageEvidence -RegistryPath $BlockRegistryPath -AssetsPath $Assets
    }
    else {
        $null
    }
    $MetricsOut = [IO.Path]::GetFullPath($MetricsOut)
    $RuntimeDirectory = if ($AcceptanceBoundParameters.ContainsKey('BdsRuntimeDirectory')) {
        if ([string]::IsNullOrWhiteSpace($BdsRuntimeDirectory)) {
            throw 'BdsRuntimeDirectory must not be empty'
        }
        ConvertTo-NormalizedRuntimePath -Path $BdsRuntimeDirectory
    }
    else {
        Join-Path (Join-Path $ProjectRoot '.local\bds-runtime') (Split-Path -Leaf $BdsDir)
    }
    $RunName = if ($DryRun) { 'dry-run' } else { "{0}-{1}" -f [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'), $PID }
    $RunDirectory = Join-Path (Join-Path $ProjectRoot '.local\acceptance') $RunName
    $TransparentWitnessRequestPath = Join-Path $RunDirectory 'transparent-witness-request.json'
    $ModelWitnessRequestPath = Join-Path $RunDirectory 'model-witness-request.json'
    $SocketDirectory = Join-Path $RunDirectory 'socket'
    $CanonicalMetrics = Join-Path $RunDirectory 'app-metrics.json'
    $BdsExecutable = Join-Path $RuntimeDirectory $BdsExecutableName
    $CoreExecutable = Join-Path $ProjectRoot 'target\release\bedrock-core.exe'
    $AppExecutable = if ($hasClientExecutable) {
        $ClientExecutable
    }
    else {
        Join-Path $ProjectRoot 'target\release\bedrock-client.exe'
    }
    if ($hasClientExecutable) {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $ClientExecutable `
            -RuntimeDirectory $RuntimeDirectory `
            -RunDirectory $RunDirectory `
            -CoreExecutable $CoreExecutable `
            -MetricsOut $MetricsOut
    }
    $Upstream = '127.0.0.1:19132'
    $BdsArguments = @()
    $CoreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Upstream)
    $AppArguments = @(
        '--socket-dir', $SocketDirectory,
        '--acceptance-seconds', $DurationSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--metrics-out', $CanonicalMetrics
    )
    if ($AcceptanceBoundParameters.ContainsKey('Assets')) {
        $AppArguments += @('--assets', $Assets)
    }
    if ($isWaterGallery) {
        $AppArguments += @('--require-transparent-presentation', '--transparent-witness-request', $TransparentWitnessRequestPath)
    }
    if ($isModelWitnessGallery) {
        $AppArguments += @('--model-witness-request', $ModelWitnessRequestPath)
    }
    if ($VisualFixturePose -eq 'None' -and -not $FullViewTeleportGate -and -not $LeafForestBaseline) {
        $AppArguments += '--auto-fly'
    }
    if ($FullViewTeleportGate) {
        $AppArguments += @('--full-view-teleport-gate', '--frame-cap', '60')
    }
    if ($NoVsync) {
        $AppArguments += '--no-vsync'
    }
    $BdsCommand = Format-ResolvedCommand $BdsExecutable $BdsArguments
    $CoreCommand = Format-ResolvedCommand $CoreExecutable $CoreArguments
    $AppCommand = Format-ResolvedCommand $AppExecutable $AppArguments
    
    if ($DryRun) {
        Write-Output "BDS_COMMAND=$BdsCommand"
        Write-Output "CORE_COMMAND=$CoreCommand"
        Write-Output "APP_COMMAND=$AppCommand"
        Write-Output 'BUILD_PROFILE=release'
        if ($NoVsync) {
            Write-Output 'REQUESTED_PRESENT_MODE=Immediate'
        }
        else {
            Write-Output 'REQUESTED_PRESENT_MODE=Fifo'
        }
        Write-Output 'EFFECTIVE_PRESENT_MODE=UNPROVEN'
        if ($VisualFixturePose -ne 'None') {
            Write-Output "VISUAL_FIXTURE_POSE=$VisualFixturePose"
        }
        if ($isCrossCropGallery) {
            Write-Output "CROSS_CROP_GALLERY_ASSETS_SHA256=$($CrossCropCoverage.assets_sha256)"
            $galleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose
                state_set_sha256 = $CrossCropCoverage.state_set_sha256
                state_count = $CrossCropCoverage.state_count
            }
            Write-Output "CROSS_CROP_GALLERY_ARGUMENTS_SHA256=$(Get-CanonicalObjectHash -Value $galleryArguments)"
        }
        if ($isAquaticGallery) {
            Write-Output "AQUATIC_GALLERY_ASSETS_SHA256=$($AquaticCoverage.assets_sha256)"
            $aquaticGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose
                state_set_sha256 = $AquaticCoverage.state_set_sha256
                state_count = $AquaticCoverage.state_count
            }
            Write-Output "AQUATIC_GALLERY_ARGUMENTS_SHA256=$(Get-CanonicalObjectHash -Value $aquaticGalleryArguments)"
        }
        if ($isFlowerBedGallery) {
            $flowerBedGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose
                state_set_sha256 = $FlowerBedCoverage.state_set_sha256
                state_count = $FlowerBedCoverage.state_count
            }
            Write-Output "FLOWERBED_GALLERY_ARGUMENTS_SHA256=$(Get-CanonicalObjectHash -Value $flowerBedGalleryArguments)"
        }
        if ($isSlabStairGallery) {
            Write-Output "SLAB_STAIR_GALLERY_ASSETS_SHA256=$($SlabStairCoverage.assets_sha256)"
            $slabStairGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose; state_set_sha256 = $SlabStairCoverage.state_set_sha256
                slab_state_count = $SlabStairCoverage.slab_state_count; stair_state_count = $SlabStairCoverage.stair_state_count
            }
            Write-Output "SLAB_STAIR_GALLERY_ARGUMENTS_SHA256=$(Get-CanonicalObjectHash -Value $slabStairGalleryArguments)"
        }
        if ($isVineGallery) {
            Write-Output "VINE_GALLERY_ASSETS_SHA256=$($VineCoverage.assets_sha256)"
            $vineGalleryArguments = [pscustomobject][ordered]@{
                pose = $VisualFixturePose; state_set_sha256 = $VineCoverage.state_set_sha256; state_count = $VineCoverage.state_count
            }
            Write-Output "VINE_GALLERY_ARGUMENTS_SHA256=$(Get-CanonicalObjectHash -Value $vineGalleryArguments)"
        }
        if ($FullViewTeleportGate) {
            Write-Output 'FULL_VIEW_TELEPORT_GATE=1'
        }
        if ($LeafForestBaseline) {
            Write-Output 'LEAF_FOREST_BASELINE=1'
        }
        if ($LeafForestFullView) {
            Write-Output 'LEAF_FOREST_FULL_VIEW=1'
        }
        if ($null -ne $EffectiveSteadyResourceTrigger) {
            Write-Output "STEADY_RESOURCE_TRIGGER=$EffectiveSteadyResourceTrigger"
        }
        if ($SkipClientBuild) {
            Write-Output 'SKIP_CLIENT_BUILD=1'
        }
        if ($UseVsync) {
            Write-Output 'USE_VSYNC=1'
        }
        exit 0
    }
    
    $lease = $null
    $portReservation = $null
    $portV6Reservation = $null
    $bdsHandle = $null
    $coreHandle = $null
    $appHandle = $null
    $runFailure = $null
    $metadata = $null
    $teleportMarkerEvidence = $null
    $forcedRemeshMarkerEvidence = $null
    $expectedTargetCohort = $null
    $steadyResourceArtifactPath = Join-Path $RunDirectory 'steady-resources.json'
    $fixturePublication = $null
    $steadyTriggerEvidence = $null
    $targetMutationEvidence = $null
    $movePlayerIngressEvidence = $null
    $movePlayerIngressMarkerEvidence = $null
    $teleportMarkerOutputEvidence = $null
    $forcedRemeshMarkerOutputEvidence = $null
    $targetMutationMarkerOutputEvidence = $null
    $activeMutationCoordinate = $null
    $baselineSourceMutationCommand = $null
    $baselineForestPlan = $null
    $sourceWorldIdentity = $null
    $runtimeWorldIdentity = $null
    $metrics = $null
    
}
