$ErrorActionPreference = 'Stop'

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param($Expected, $Actual, [string]$Message)
    if ($Expected -cne $Actual) {
        throw "$Message`nexpected: $Expected`nactual:   $Actual"
    }
}

function Assert-Throws {
    param([scriptblock]$Action, [string]$Message)
    $threw = $false
    try {
        & $Action
    }
    catch {
        $threw = $true
    }
    Assert-True $threw $Message
}

function Assert-ThrowsLike {
    param(
        [scriptblock]$Action,
        [string]$Pattern,
        [string]$Message
    )
    $observed = $null
    try {
        & $Action
    }
    catch {
        $observed = $_.Exception.Message
    }
    Assert-True ($null -ne $observed) $Message
    Assert-True ($observed -like $Pattern) "$Message`nexpected error like: $Pattern`nactual error:        $observed"
}

function ConvertTo-TestCommandArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Format-TestResolvedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments
    )

    $parts = @((ConvertTo-TestCommandArgument $Executable))
    $parts += @($Arguments | ForEach-Object { ConvertTo-TestCommandArgument $_ })
    return $parts -join ' '
}

function Invoke-Acceptance {
    param([string[]]$Arguments)
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $script:AcceptanceScript @Arguments 2>&1
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }
    [pscustomobject]@{
        ExitCode = $LASTEXITCODE
        Output = @($output | ForEach-Object { $_.ToString() })
    }
}

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$AcceptanceScript = Join-Path $ProjectRoot 'scripts\acceptance.ps1'
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("rust-mcbe acceptance tests {0}" -f [guid]::NewGuid().ToString('N'))
$BdsDir = Join-Path $TempRoot 'bds source'
$MetricsOut = Join-Path $TempRoot 'metrics output\metrics.json'
$Assets = Join-Path $TempRoot 'vanilla assets with spaces.mcpack'
$PrebuiltClient = Join-Path $TempRoot 'opaque base client\bedrock-client.exe'
$DryRunDirectory = Join-Path $ProjectRoot '.local\acceptance\dry-run'

try {
    New-Item -ItemType Directory -Path $BdsDir -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $BdsDir 'bedrock_server.exe') -Value 'fixture' -NoNewline
    Set-Content -LiteralPath $Assets -Value 'assets fixture' -NoNewline
    New-Item -ItemType Directory -Path (Split-Path -Parent $PrebuiltClient) -Force | Out-Null
    Set-Content -LiteralPath $PrebuiltClient -Value 'pinned opaque client fixture' -NoNewline
    $prebuiltHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) "pre-existing dry-run artifact prevents an immutability assertion: $DryRunDirectory"

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
    Assert-True ($success.Output.Count -eq 3) 'default dry-run output changed'
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
                '--auto-fly',
                '--no-vsync'
            )))
    )
    Assert-Equal `
        ($expectedCommands -join [Environment]::NewLine) `
        ($commands -join [Environment]::NewLine) `
        'default dry-run commands changed'
    foreach ($flag in @('--socket-dir', '--acceptance-seconds 900', '--metrics-out', '--auto-fly', '--no-vsync')) {
        Assert-True ($commands[2].Contains($flag)) "app command is missing $flag"
    }
    Assert-True (-not $commands[2].Contains('--assets')) 'default app command unexpectedly gained --assets'
    Assert-True (-not ($success.Output -match '^VISUAL_FIXTURE_POSE=')) 'default dry-run recorded a fixture pose'
    Assert-True ($commands[0].Contains('"')) 'path containing spaces was not quoted'
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) 'dry-run created its run directory'
    Assert-True (-not (Test-Path -LiteralPath $MetricsOut)) 'dry-run wrote metrics'

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
    Assert-True ($frontAppCommand[0].Contains('--no-vsync')) 'front fixture app command lost --no-vsync'
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
        '-Assets', $Assets,
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

    $source = Get-Content -Raw -LiteralPath $AcceptanceScript
    Assert-True ($source.Contains('CopyToAsync')) 'child logs are not streamed directly to files'
    Assert-True ($source.Contains('[IO.FileOptions]::WriteThrough')) 'child log files are not write-through'
    Assert-True (-not $source.Contains('ReadToEndAsync')) 'child logs are retained in memory'
    Assert-True ($source.Contains('-WorkingDirectory $ProjectRoot')) 'builds are not rooted at the project directory'
    Assert-True ($source.Contains("'9948b1729395d2e819fce28e079d4a7bfc67716c'")) 'gophertunnel metadata commit is not the repository pin'
    Assert-True ($source.Contains("'6f6806e821a579c183c44d786f76d9b358a2b825'")) 'Valentine metadata commit is not the repository pin'
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
    Assert-True ($source.Contains('Move-Item -LiteralPath $temporaryPath -Destination $Path')) 'fixture manifest publication is not an atomic sibling rename'
    Assert-True ($source.Contains('$cpuPercent = 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount)')) 'steady CPU normalization formula changed'
    Assert-True (([regex]::Matches($source, '\.Refresh\(\)')).Count -ge 4) 'resource sampling does not refresh both process handles before/during sampling'
    $baselineSourceMutationIndex = $source.IndexOf('$baselineSourceMutationCommand = Publish-BaselineSourceMutation', [StringComparison]::Ordinal)
    $resourceSamplingIndex = $source.IndexOf('$resourceDocument = Measure-SteadyResources', [StringComparison]::Ordinal)
    $baselineForestPublishIndex = $source.IndexOf('$fixturePlan = New-LeafForestPlan -MutationCoordinate $coordinate -Mode Baseline', [StringComparison]::Ordinal)
    Assert-True ($baselineSourceMutationIndex -ge 0 -and $resourceSamplingIndex -gt $baselineSourceMutationIndex) 'baseline did not issue its source mutation immediately before the WorldReady observation window'
    Assert-True ($baselineForestPublishIndex -gt $resourceSamplingIndex) 'baseline far forest could publish before the source mutation observation window'
    $metricsValidationIndex = $source.IndexOf('$metrics = Assert-AcceptanceMetrics', [StringComparison]::Ordinal)
    Assert-True ($resourceSamplingIndex -ge 0 -and $metricsValidationIndex -gt $resourceSamplingIndex) 'full-view metrics SLA validation can run before steady-resource sampling/artifact publication'

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

    $teleportMarkerLine = 'RUST_MCBE_TELEPORT_SETTLED target=0:65:65:16 committed=0:65:65:16 ms=1500.0000 view_generation=7 render_ready_ms=1200.0000 publisher_ms=100.0000 first_level_ms=200.0000 last_level_ms=600.0000 level_events=1089 first_sub_ms=250.0000 last_sub_ms=900.0000 sub_events=1089 first_frame_sequence=41 stable_frame_sequence=42 first_present_ms=1300.0000 first_gpu_ms=1350.0000 stable_present_ms=1400.0000 stable_gpu_ms=1500.0000 expected_manifest_count=4 expected_manifest_hash=1111222233334444 first_presented_manifest_count=4 first_presented_manifest_hash=1111222233334444 stable_presented_manifest_count=4 stable_presented_manifest_hash=1111222233334444 expected=1089 loaded_target=1089 missing_target=0 foreign_loaded=0 foreign_requested=0 foreign_resident=0 source_leftover=0 resident_count=3 resident_hash=aaaabbbbccccdddd known_air_count=1 known_air_hash=eeeeffff00001111 missing_target_instances=0 unexpected_target_instances=0 source_instances=0 foreign_instances=0 stale_generation_instances=0 orphan_allocations=0 frame_count=90'
    $forcedMarkerLine = 'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED target=0:65:65:16 committed=0:65:65:16 ms=1500.0000 view_generation=8 render_ready_ms=0.0000 first_frame_sequence=43 stable_frame_sequence=44 first_present_ms=1200.0000 first_gpu_ms=1300.0000 stable_present_ms=1400.0000 stable_gpu_ms=1500.0000 expected_manifest_count=4 expected_manifest_hash=5555666677778888 first_presented_manifest_count=4 first_presented_manifest_hash=5555666677778888 stable_presented_manifest_count=4 stable_presented_manifest_hash=5555666677778888 expected=1089 loaded_target=1089 missing_target=0 foreign_loaded=0 foreign_requested=0 foreign_resident=0 source_leftover=0 resident_count=3 resident_hash=aaaabbbbccccdddd known_air_count=1 known_air_hash=eeeeffff00001111 missing_target_instances=0 unexpected_target_instances=0 source_instances=0 foreign_instances=0 stale_generation_instances=0 orphan_allocations=0 frame_count=90'
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
    }
    $expectedFarCamera = @(($mutationCoordinate[0] + 1040), ($mutationCoordinate[1] + 12), ($mutationCoordinate[2] + 1040))
    $expectedTargetMutation = @(($mutationCoordinate[0] + 1040), $mutationCoordinate[1], ($mutationCoordinate[2] + 1052))
    Assert-Equal ($expectedFarCamera -join ',') (@($baselineForestPlan.Target.x, $baselineForestPlan.Target.y, $baselineForestPlan.Target.z) -join ',') 'baseline forest did not use the identical far camera/cohort'
    Assert-Equal ($expectedFarCamera -join ',') (@($fullViewForestPlan.Target.x, $fullViewForestPlan.Target.y, $fullViewForestPlan.Target.z) -join ',') 'far camera changed from the fixed 65-chunk binding target'
    Assert-Equal ($expectedTargetMutation -join ',') (@($baselineForestPlan.TargetMutation.x, $baselineForestPlan.TargetMutation.y, $baselineForestPlan.TargetMutation.z) -join ',') 'baseline forest did not use the identical far mutation coordinate'
    Assert-Equal ($expectedTargetMutation -join ',') (@($fullViewForestPlan.TargetMutation.x, $fullViewForestPlan.TargetMutation.y, $fullViewForestPlan.TargetMutation.z) -join ',') 'far target mutation changed from the no-CLI contract'
    Assert-Equal ($baselineForestPlan.Commands -join "`n") ($fullViewForestPlan.Commands -join "`n") 'baseline and full-view forests did not publish identical scene commands'
    Assert-Equal 65 $baselineForestPlan.Manifest.offset_chunks 'baseline forest did not publish the same far offset'
    $initialTargetCommand = "setblock $($expectedTargetMutation[0]) $($expectedTargetMutation[1]) $($expectedTargetMutation[2]) minecraft:diamond_block"
    Assert-True ($fullViewForestPlan.FixtureCommands -contains $initialTargetCommand) 'forest did not initialize target mutation to the opposite block'
    Assert-True (-not ($fullViewForestPlan.FixtureCommands -contains $initialTargetCommand.Replace('diamond_block', 'gold_block'))) 'forest initialized target to the first post-ARM block, making it a no-op'
    Assert-Equal 'minecraft:gold_block,minecraft:diamond_block' (@($fullViewForestPlan.Manifest.mutation_blocks) -join ',') 'target mutation alternation changed'
    Assert-Equal ($mutationCoordinate -join ',') (@($fullViewForestPlan.Manifest.source_mutation.x, $fullViewForestPlan.Manifest.source_mutation.y, $fullViewForestPlan.Manifest.source_mutation.z) -join ',') 'forest manifest lost source mutation identity'
    Assert-Equal ($expectedTargetMutation -join ',') (@($fullViewForestPlan.Manifest.target_mutation.x, $fullViewForestPlan.Manifest.target_mutation.y, $fullViewForestPlan.Manifest.target_mutation.z) -join ',') 'forest manifest lost target mutation identity'

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

    $forestInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $forestHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $forestInput }
    }
    $script:ObservedForestFence = $null
    $forestRunDirectory = Join-Path $TempRoot 'forest full view run'
    New-Item -ItemType Directory -Path $forestRunDirectory | Out-Null
    $forestPublication = Publish-FullViewTeleport `
        -Handle $forestHandle `
        -Plan $fullViewForestPlan `
        -RunDirectory $forestRunDirectory `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedForestFence = $Marker
            return $Marker
        }
    Assert-Equal $fullViewForestPlan.FenceMarker $script:ObservedForestFence 'forest publisher did not observe the list fence'
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
        'fixture_commands_completed,processing_fence_observed,visual_fixture_ready,teleport_issued' `
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
    $null = Publish-VisualFixture `
        -Handle $baselineHandle `
        -Plan $baselineForestPlan `
        -RunDirectory $baselineRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence { param($Handle, $Marker, $TimeoutSeconds); return $Marker }
    Assert-Equal 'setblock 101 64 -37 minecraft:gold_block' $baselineSourceCommand 'baseline source mutation prelude changed'
    $expectedBaselineConsole = @($baselineSourceCommand) + @($baselineForestPlan.Commands)
    Assert-Equal ($expectedBaselineConsole -join "`n") ((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) -join "`n") 'baseline source mutation did not precede the far forest fence/teleport'
    $baselineEvents = @(Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal `
        'source_mutation_command,fixture_commands_completed,processing_fence_observed,visual_fixture_ready,teleport_issued' `
        (@($baselineEvents | ForEach-Object { $_.event }) -join ',') `
        'baseline event evidence did not order source mutation before the far forest'

    $serverPropertiesPath = Join-Path $TempRoot 'server.properties'
    [IO.File]::WriteAllLines(
        $serverPropertiesPath,
        @(
            'server-port=19132'
            'server-portv6=19133'
            'online-mode=true'
            'allow-list=true'
            'enable-lan-visibility=true'
            'server-name=fixture'
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
        'server-name=fixture'
    )) {
        Assert-True ($serverProperties -contains $expectedProperty) "missing rewritten property: $expectedProperty"
    }

    $runtimeSource = (Resolve-Path -LiteralPath $BdsDir).Path.TrimEnd('\', '/')
    if ($runtimeSource.StartsWith('\\')) {
        $goRuntimeSource = '\\?\UNC\' + $runtimeSource.TrimStart('\')
    }
    else {
        $goRuntimeSource = '\\?\' + $runtimeSource
    }
    $goRuntimeOwner = "rust-mcbe-bds-runtime-v1`nsource=$($goRuntimeSource.ToLowerInvariant())`n"
    $goOwnedRuntime = Join-Path $TempRoot 'go-owned stable runtime'
    New-Item -ItemType Directory -Path $goOwnedRuntime -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $goOwnedRuntime '.rust-mcbe-runtime-owner'),
        $goRuntimeOwner,
        [Text.UTF8Encoding]::new($false)
    )
    $goOwnedExecutable = Set-StableRuntime `
        -SourceDirectory $BdsDir `
        -RuntimeDirectory $goOwnedRuntime `
        -ExecutableName 'bedrock_server.exe'
    Assert-True (Test-Path -LiteralPath $goOwnedExecutable) 'Go-style extended owner path was rejected for the same BDS source'

    $differentRuntime = Join-Path $TempRoot 'different-owned stable runtime'
    New-Item -ItemType Directory -Path $differentRuntime -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $differentRuntime '.rust-mcbe-runtime-owner'),
        "rust-mcbe-bds-runtime-v1`nsource=\\?\c:\definitely-different-bds-source`n",
        [Text.UTF8Encoding]::new($false)
    )
    Assert-Throws {
        Set-StableRuntime `
            -SourceDirectory $BdsDir `
            -RuntimeDirectory $differentRuntime `
            -ExecutableName 'bedrock_server.exe'
    } 'different Go-style BDS owner marker was accepted'

    $runtimeSafetyScript = Join-Path $PSScriptRoot 'acceptance.RuntimeSafety.Tests.ps1'
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $runtimeSafetyOutput = & powershell `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -File $runtimeSafetyScript `
            -Case All 2>&1
        $runtimeSafetyExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }
    Assert-True ($runtimeSafetyExitCode -eq 0) "runtime safety tests failed: $($runtimeSafetyOutput -join [Environment]::NewLine)"
    Assert-True (@($runtimeSafetyOutput | Where-Object { $_.ToString().Contains('acceptance runtime safety tests (All): PASS') }).Count -eq 1) 'runtime safety test success marker was missing'

    Invoke-CheckedBuild `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "if ((Get-Location).Path -ne '$TempRoot') { exit 9 }") `
        -LogPath (Join-Path $TempRoot 'working-directory.log') `
        -WorkingDirectory $TempRoot

    $stderrBuildLog = Join-Path $TempRoot 'successful-stderr-build.log'
    Invoke-CheckedBuild `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "[Console]::Error.WriteLine('compiler-progress'); exit 0") `
        -LogPath $stderrBuildLog `
        -WorkingDirectory $TempRoot
    Assert-True ((Get-Content -Raw -LiteralPath $stderrBuildLog).Contains('compiler-progress')) 'successful native stderr was not retained in the build log'

    $helper = Start-LoggedProcess `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "Write-Output 'TEST_READY'; [Console]::Error.WriteLine('error-line')") `
        -WorkingDirectory $TempRoot `
        -StdoutPath (Join-Path $TempRoot 'helper.stdout.log') `
        -StderrPath (Join-Path $TempRoot 'helper.stderr.log')
    Assert-True ((Wait-ProcessOutputMarker -Handle $helper -Marker 'TEST_READY' -TimeoutSeconds 10) -eq 'TEST_READY') 'direct log stream did not expose readiness marker'
    Assert-True ($helper.Process.WaitForExit(10000)) 'logging helper did not exit'
    Complete-ProcessLogs $helper
    Assert-True ((Get-Content -Raw -LiteralPath $helper.StdoutPath).Contains('TEST_READY')) 'stdout was not preserved'
    Assert-True ((Get-Content -Raw -LiteralPath $helper.StderrPath).Contains('error-line')) 'stderr was not preserved'

    $orderedMarkerHelper = Start-LoggedProcess `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "[Console]::Out.WriteLine('CURSOR_FIRST'); [Console]::Out.WriteLine('CURSOR_SECOND')") `
        -WorkingDirectory $TempRoot `
        -StdoutPath (Join-Path $TempRoot 'ordered-markers.stdout.log') `
        -StderrPath (Join-Path $TempRoot 'ordered-markers.stderr.log')
    $firstMarkerEvidence = Wait-ProcessOutputMarker -Handle $orderedMarkerHelper -Marker 'CURSOR_FIRST' -TimeoutSeconds 10 -PassThruEvidence
    $secondMarkerEvidence = Wait-ProcessOutputMarker -Handle $orderedMarkerHelper -Marker 'CURSOR_SECOND' -TimeoutSeconds 10 -PassThruEvidence
    Assert-Equal 'CURSOR_FIRST' $firstMarkerEvidence.Line 'marker cursor returned the wrong first line'
    Assert-Equal 'CURSOR_SECOND' $secondMarkerEvidence.Line 'marker cursor lost the buffered second line'
    Assert-True ([uint64]$secondMarkerEvidence.LineNumber -gt [uint64]$firstMarkerEvidence.LineNumber) 'marker cursor did not preserve increasing stdout line positions'
    Assert-True ($orderedMarkerHelper.Process.WaitForExit(10000)) 'ordered marker helper did not exit'
    Complete-ProcessLogs $orderedMarkerHelper

    $reversedMarkerHelper = Start-LoggedProcess `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "[Console]::Out.WriteLine('HISTORICAL_MARKER'); [Console]::Out.WriteLine('CURRENT_MARKER')") `
        -WorkingDirectory $TempRoot `
        -StdoutPath (Join-Path $TempRoot 'reversed-markers.stdout.log') `
        -StderrPath (Join-Path $TempRoot 'reversed-markers.stderr.log')
    $null = Wait-ProcessOutputMarker -Handle $reversedMarkerHelper -Marker 'CURRENT_MARKER' -TimeoutSeconds 10 -PassThruEvidence
    Assert-ThrowsLike {
        Wait-ProcessOutputMarker -Handle $reversedMarkerHelper -Marker 'HISTORICAL_MARKER' -TimeoutSeconds 1 -PassThruEvidence
    } "timed out waiting for 'HISTORICAL_MARKER'*" 'marker wait rescanned and accepted an earlier stdout line'
    Assert-True ($reversedMarkerHelper.Process.WaitForExit(10000)) 'reversed marker helper did not exit'
    Complete-ProcessLogs $reversedMarkerHelper

    $udpHelper = $null
    $bufferedHelper = $null
    try {
        $udpReservation = New-ReservedUdpPort
        $udpPort = $udpReservation.Port
        $udpReservation.Client.Dispose()
        $udpServerScript = @'
$ErrorActionPreference = 'Stop'
$udp = [Net.Sockets.UdpClient]::new(__PORT__)
$udp.Client.ReceiveTimeout = 5000
$magic = [byte[]]@(0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78)
[Console]::Out.WriteLine('UDP_READY')
[Console]::Out.Flush()
try {
    foreach ($mode in @('wrong-id', 'wrong-magic', 'valid')) {
        $remote = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
        $request = $udp.Receive([ref]$remote)
        $response = [byte[]]::new(33)
        $response[0] = if ($mode -eq 'wrong-id') { 0x1b } else { 0x1c }
        if ($request.Length -ge 9) {
            [Array]::Copy($request, 1, $response, 1, 8)
        }
        if ($mode -ne 'wrong-magic') {
            [Array]::Copy($magic, 0, $response, 17, $magic.Length)
        }
        $null = $udp.Send($response, $response.Length, $remote)
    }
}
finally {
    $udp.Dispose()
}
'@.Replace('__PORT__', $udpPort.ToString([Globalization.CultureInfo]::InvariantCulture))
        $udpServerCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($udpServerScript))
        $udpHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-EncodedCommand', $udpServerCommand) `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'udp-helper.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'udp-helper.stderr.log')
        $null = Wait-ProcessOutputMarker -Handle $udpHelper -Marker 'UDP_READY' -TimeoutSeconds 10

        $bufferedChildScript = @'
$writer = [IO.StreamWriter]::new(
    [Console]::OpenStandardOutput(),
    [Text.UTF8Encoding]::new($false),
    4096
)
$writer.AutoFlush = $false
$writer.WriteLine('BUFFERED_READY')
$null = [Console]::In.ReadLine()
$writer.Dispose()
'@
        $bufferedChildCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($bufferedChildScript))
        $bufferedHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-EncodedCommand', $bufferedChildCommand) `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'buffered-helper.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'buffered-helper.stderr.log')
        $readinessProbe = {
            Test-RakNetUnconnectedPong `
                -Address '127.0.0.1' `
                -Port $udpPort `
                -TimeoutMilliseconds 500
        }.GetNewClosure()
        $observed = Wait-ProcessOutputMarker `
            -Handle $bufferedHelper `
            -Marker 'BUFFERED_READY' `
            -TimeoutSeconds 10 `
            -ReadinessProbe $readinessProbe
        Assert-Equal 'BUFFERED_READY' $observed 'RakNet readiness did not release the buffered BDS marker wait'
        Assert-True (-not $bufferedHelper.Process.HasExited) 'buffered logging helper exited before alternate readiness was observed'
        Assert-True ((Get-Item -LiteralPath $bufferedHelper.StdoutPath).Length -eq 0) 'buffered marker unexpectedly reached the log before alternate readiness'
        Assert-True ($udpHelper.Process.WaitForExit(2000)) 'invalid RakNet responses were accepted before the valid pong'
    }
    finally {
        if ($null -ne $bufferedHelper) {
            Stop-BoundedProcess -Handle $bufferedHelper -Kind 'core'
            Complete-ProcessLogs $bufferedHelper
        }
        if ($null -ne $udpHelper) {
            Stop-BoundedProcess -Handle $udpHelper -Kind 'core'
            Complete-ProcessLogs $udpHelper
        }
    }
    Assert-True ((Get-Content -Raw -LiteralPath $bufferedHelper.StdoutPath).Contains('BUFFERED_READY')) 'alternate readiness lost continuously captured stdout'

    $eofHelper = Start-LoggedProcess `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', '[Console]::In.ReadToEnd() | Out-Null') `
        -WorkingDirectory $TempRoot `
        -StdoutPath (Join-Path $TempRoot 'eof.stdout.log') `
        -StderrPath (Join-Path $TempRoot 'eof.stderr.log')
    Stop-BoundedProcess -Handle $eofHelper -Kind 'core'
    Assert-True $eofHelper.Process.HasExited 'core-style EOF cleanup left its child running'
    Complete-ProcessLogs $eofHelper

    $bdsStopState = [pscustomobject]@{
        Commands = [Collections.Generic.List[string]]::new()
        FlushCount = 0
        CloseCount = 0
        WaitTimeout = 0
    }
    $bdsStopInput = [pscustomobject]@{ State = $bdsStopState }
    $bdsStopInput | Add-Member -MemberType ScriptMethod -Name WriteLine -Value {
        param([string]$Command)
        $this.State.Commands.Add($Command)
    }
    $bdsStopInput | Add-Member -MemberType ScriptMethod -Name Flush -Value {
        $this.State.FlushCount++
    }
    $bdsStopInput | Add-Member -MemberType ScriptMethod -Name Close -Value {
        $this.State.CloseCount++
    }
    $bdsStopProcess = [pscustomobject]@{
        StandardInput = $bdsStopInput
        HasExited = $false
        State = $bdsStopState
    }
    $bdsStopProcess | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
        param([int]$Timeout)
        $this.State.WaitTimeout = $Timeout
        $this.HasExited = $true
        return $true
    }
    $bdsStopHandle = [pscustomobject]@{ Process = $bdsStopProcess }
    $bdsStopLog = Join-Path $TempRoot 'bds-stop.console.log'
    Stop-BoundedProcess `
        -Handle $bdsStopHandle `
        -Kind 'bds' `
        -BdsConsoleLogPath $bdsStopLog
    Assert-Equal 1 $bdsStopState.Commands.Count 'BDS cleanup did not write exactly one command'
    Assert-Equal 'stop' $bdsStopState.Commands[0] 'BDS cleanup wrote the wrong command'
    Assert-Equal 1 $bdsStopState.FlushCount 'BDS cleanup did not flush standard input exactly once'
    Assert-Equal 1 $bdsStopState.CloseCount 'BDS cleanup did not close standard input exactly once'
    Assert-Equal 20000 $bdsStopState.WaitTimeout 'BDS cleanup changed its graceful wait timeout'
    $loggedStopCommands = @(Get-Content -LiteralPath $bdsStopLog)
    Assert-Equal 1 $loggedStopCommands.Count 'BDS cleanup did not log exactly one command'
    Assert-Equal 'stop' $loggedStopCommands[0] 'BDS cleanup logged the wrong command'

    $expectedAssetBlobSha256 = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
    $metrics = [ordered]@{
        session_seconds = 900.0; world_ready = $true; requested_radius_chunks = 16
        received_radius_chunks = 16; publisher_radius_chunks = 16
        mutation_coordinate = @(1, 2, 3); visible_mutation_count = 1; frame_count = 1
        p50_frame_ms = 1.0; p95_frame_ms = 2.0; p99_frame_ms = 3.0; max_frame_ms = 4.0
        max_decode_ms = 1.0; max_mesh_ms = 1.0; max_remesh_ms = 1.0
        teleport_settle_ms = $null; forced_full_view_remesh_ms = $null
        max_mutation_to_visible_ms = 50.0; decode_error_count = 0
        rendered_sub_chunks = 1; resident_sub_chunks = 1; visible_sub_chunks = 1
        peak_admitted_world_events = 1; peak_admitted_heavy_events = 1
        peak_queued_decode_jobs = 1; peak_in_flight_decode_jobs = 1
        peak_completed_decode_results = 1; peak_pending_retry_requests = 1
        peak_outbound_requests = 1; peak_pending_mesh_jobs = 1
        peak_in_flight_mesh_jobs = 1; gpu_upload_bytes = 1
        assets = [ordered]@{
            source_tag = 'v1.26.30.32-preview'
            source_sha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
            blob_sha256 = $expectedAssetBlobSha256
            texture_layers = 372
            texture_bytes_including_mips = 1000
            material_count = 405
            missing_mapping_count = 0
            diagnostic_quad_count = 12
        }
        teleport_proof = [ordered]@{
            target = '0:65:65:16'; committed = '0:65:65:16'; ms = 1500.0
            view_generation = 7; render_ready_ms = 1200.0; publisher_ms = 100.0
            first_level_ms = 200.0; last_level_ms = 600.0; level_events = 1089
            first_sub_ms = 250.0; last_sub_ms = 900.0; sub_events = 1089
            first_frame_sequence = 41; stable_frame_sequence = 42
            first_present_ms = 1300.0; first_gpu_ms = 1350.0
            stable_present_ms = 1400.0; stable_gpu_ms = 1500.0; frame_count = 90
            expected_manifest_count = 4; expected_manifest_hash = '1111222233334444'
            first_presented_manifest_count = 4; first_presented_manifest_hash = '1111222233334444'
            stable_presented_manifest_count = 4; stable_presented_manifest_hash = '1111222233334444'
            expected = 1089; loaded_target = 1089; missing_target = 0
            foreign_loaded = 0; foreign_requested = 0; foreign_resident = 0; source_leftover = 0
            resident_count = 3; resident_hash = 'aaaabbbbccccdddd'
            known_air_count = 1; known_air_hash = 'eeeeffff00001111'
            missing_target_instances = 0; unexpected_target_instances = 0; source_instances = 0
            foreign_instances = 0; stale_generation_instances = 0; orphan_allocations = 0
        }
        forced_full_view_remesh_proof = [ordered]@{
            target = '0:65:65:16'; committed = '0:65:65:16'; ms = 1500.0
            view_generation = 8; render_ready_ms = 0.0
            first_frame_sequence = 43; stable_frame_sequence = 44
            first_present_ms = 1200.0; first_gpu_ms = 1300.0
            stable_present_ms = 1400.0; stable_gpu_ms = 1500.0; frame_count = 90
            expected_manifest_count = 4; expected_manifest_hash = '5555666677778888'
            first_presented_manifest_count = 4; first_presented_manifest_hash = '5555666677778888'
            stable_presented_manifest_count = 4; stable_presented_manifest_hash = '5555666677778888'
            expected = 1089; loaded_target = 1089; missing_target = 0
            foreign_loaded = 0; foreign_requested = 0; foreign_resident = 0; source_leftover = 0
            resident_count = 3; resident_hash = 'aaaabbbbccccdddd'
            known_air_count = 1; known_air_hash = 'eeeeffff00001111'
            missing_target_instances = 0; unexpected_target_instances = 0; source_instances = 0
            foreign_instances = 0; stale_generation_instances = 0; orphan_allocations = 0
        }
    }
    $metricsPath = Join-Path $TempRoot 'validation-metrics.json'
    $steadyResourceArtifactPath = Join-Path $TempRoot 'steady-resources.json'
    $steadyArtifactSamples = @(1..30 | ForEach-Object {
        [pscustomobject]@{
            elapsed_seconds = [double]$_
            combined_rss_bytes = 350MB
            cpu_percent = 10.0
        }
    })
    $steadyArtifactTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    $steadyArtifact = New-SteadyResourceDocument `
        -Samples $steadyArtifactSamples `
        -DurationSeconds 30 `
        -Trigger $steadyArtifactTrigger
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath

    $metrics.teleport_settle_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 1500.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $fullViewArguments = @{
        Path = $metricsPath
        RequireFullViewTeleport = $true
        TeleportMarker = $teleportMarker
        ForcedRemeshMarker = $forcedMarker
        ExpectedTargetCohort = '0:65:65:16'
        SteadyResourceArtifactPath = $steadyResourceArtifactPath
        ExpectedMutationCoordinate = @(1, 2, 3)
        RequireAssets = $true
        ExpectedAssetBlobSha256 = $expectedAssetBlobSha256
    }
    $null = Assert-AcceptanceMetrics @fullViewArguments

    $metrics.visible_mutation_count = 0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'visible_mutation_count was zero for target mutation*' 'full-view leaf evidence accepted no visible target mutation'
    $metrics.visible_mutation_count = 1
    $metrics.mutation_coordinate = @(9, 9, 9)
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'mutation_coordinate did not match manifested target*' 'full-view leaf evidence accepted the source/wrong mutation coordinate'
    $metrics.mutation_coordinate = @(1, 2, 3)
    $metrics.assets.missing_mapping_count = 1
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'asset missing_mapping_count=1, expected zero*' 'leaf evidence accepted a missing asset mapping'
    $metrics.assets.missing_mapping_count = 0
    $metrics.assets.blob_sha256 = 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'asset blob_sha256 did not match supplied blob*' 'leaf evidence accepted metrics from the wrong asset blob'
    $metrics.assets.blob_sha256 = $expectedAssetBlobSha256
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath

    $staleResourceArtifact = $steadyArtifact | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    $staleResourceArtifact.trigger.target = '0:66:65:16'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($staleResourceArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'steady resource artifact trigger mismatch for target*' 'stale steady-resource trigger provenance passed validation'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $tamperedResourceArtifact = $steadyArtifact | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    $tamperedResourceArtifact.summary.max_combined_rss_bytes = 1
    $tamperedResourceArtifact.summary.mean_cpu_percent = 0.0
    $tamperedResourceArtifact.summary.p95_cpu_percent = 0.0
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($tamperedResourceArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'steady resource artifact summary did not match samples*' 'tampered steady-resource summary passed validation'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )

    $singleFrameTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('frame_count=90', 'frame_count=1') `
        -Kind Teleport
    $singleFrameArguments = $fullViewArguments.Clone()
    $singleFrameArguments.TeleportMarker = $singleFrameTeleportMarker
    $metrics.teleport_proof.frame_count = 1
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @singleFrameArguments } 'teleport_proof.frame_count must cover at least two presented frames*' 'a one-frame presented interval passed validation'
    $metrics.teleport_proof.frame_count = 90

    $changedCohortRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('resident_hash=aaaabbbbccccdddd', 'resident_hash=0000000000000001') `
        -Kind ForcedRemesh
    $changedCohortArguments = $fullViewArguments.Clone()
    $changedCohortArguments.ForcedRemeshMarker = $changedCohortRemeshMarker
    $metrics.forced_full_view_remesh_proof.resident_hash = '0000000000000001'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @changedCohortArguments } 'full-view proof cohort changed between teleport and forced remesh at resident_hash*' 'forced remesh silently accepted a changed resident cohort'
    $metrics.forced_full_view_remesh_proof.resident_hash = 'aaaabbbbccccdddd'

    $changedManifestCountMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('manifest_count=4', 'manifest_count=5') `
        -Kind ForcedRemesh
    $changedManifestCountArguments = $fullViewArguments.Clone()
    $changedManifestCountArguments.ForcedRemeshMarker = $changedManifestCountMarker
    $metrics.forced_full_view_remesh_proof.expected_manifest_count = 5
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_count = 5
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_count = 5
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @changedManifestCountArguments } 'forced remesh expected manifest count changed from teleport*' 'forced remesh silently changed its mesh-bearing key count'
    $metrics.forced_full_view_remesh_proof.expected_manifest_count = 4
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_count = 4
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_count = 4

    $earlyRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('first_frame_sequence=43 stable_frame_sequence=44', 'first_frame_sequence=42 stable_frame_sequence=43') `
        -Kind ForcedRemesh
    $earlyRemeshArguments = $fullViewArguments.Clone()
    $earlyRemeshArguments.ForcedRemeshMarker = $earlyRemeshMarker
    $metrics.forced_full_view_remesh_proof.first_frame_sequence = 42
    $metrics.forced_full_view_remesh_proof.stable_frame_sequence = 43
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @earlyRemeshArguments } 'forced remesh frames were not later than teleport frames*' 'forced remesh reused the teleport stable frame'
    $metrics.forced_full_view_remesh_proof.first_frame_sequence = 43
    $metrics.forced_full_view_remesh_proof.stable_frame_sequence = 44

    $staleGenerationRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('view_generation=8', 'view_generation=7') `
        -Kind ForcedRemesh
    $staleGenerationArguments = $fullViewArguments.Clone()
    $staleGenerationArguments.ForcedRemeshMarker = $staleGenerationRemeshMarker
    $metrics.forced_full_view_remesh_proof.view_generation = 7
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @staleGenerationArguments } 'forced remesh view generation did not advance beyond teleport*' 'forced remesh reused the teleport view generation'
    $metrics.forced_full_view_remesh_proof.view_generation = 8

    $unchangedManifestRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('5555666677778888', '1111222233334444') `
        -Kind ForcedRemesh
    $unchangedManifestArguments = $fullViewArguments.Clone()
    $unchangedManifestArguments.ForcedRemeshMarker = $unchangedManifestRemeshMarker
    $metrics.forced_full_view_remesh_proof.expected_manifest_hash = '1111222233334444'
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_hash = '1111222233334444'
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_hash = '1111222233334444'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @unchangedManifestArguments } 'forced remesh expected manifest hash did not change from teleport*' 'forced remesh did not prove new mesh generations'
    $metrics.forced_full_view_remesh_proof.expected_manifest_hash = '5555666677778888'
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_hash = '5555666677778888'
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_hash = '5555666677778888'

    $metrics.teleport_settle_ms = 2000.1
    $metrics.teleport_proof.ms = 2000.1
    $metrics.teleport_proof.stable_gpu_ms = 2000.1
    $slowTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('ms=1500.0000', 'ms=2000.1000') `
        -Kind Teleport
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $slowTeleportArguments = $fullViewArguments.Clone()
    $slowTeleportArguments.TeleportMarker = $slowTeleportMarker
    Assert-ThrowsLike `
        { Assert-AcceptanceMetrics @slowTeleportArguments } `
        'teleport_settle_ms failed the 2000ms gate*' `
        'over-budget end-to-end teleport with a fast remesh passed validation'
    Assert-True (Test-Path -LiteralPath $steadyResourceArtifactPath -PathType Leaf) 'resource artifact was not retained before the teleport SLA failure surfaced'

    $metrics.teleport_settle_ms = 1500.0
    $metrics.teleport_proof.ms = 1500.0
    $metrics.teleport_proof.stable_gpu_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 2000.1
    $metrics.forced_full_view_remesh_proof.ms = 2000.1
    $metrics.forced_full_view_remesh_proof.stable_gpu_ms = 2000.1
    $slowRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('ms=1500.0000', 'ms=2000.1000') `
        -Kind ForcedRemesh
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $slowRemeshArguments = $fullViewArguments.Clone()
    $slowRemeshArguments.ForcedRemeshMarker = $slowRemeshMarker
    Assert-ThrowsLike `
        { Assert-AcceptanceMetrics @slowRemeshArguments } `
        'forced_full_view_remesh_ms failed the 2000ms gate*' `
        'over-budget forced full-view remesh with a fast teleport passed validation'
    $metrics.forced_full_view_remesh_ms = $null
    $metrics.forced_full_view_remesh_proof.ms = 1500.0
    $metrics.forced_full_view_remesh_proof.stable_gpu_ms = 1500.0

    $metrics.teleport_settle_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 1500.0
    foreach ($field in @('missing_target', 'foreign_loaded', 'foreign_requested', 'foreign_resident', 'source_leftover')) {
        $metrics.teleport_proof[$field] = 1
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike `
            { Assert-AcceptanceMetrics @fullViewArguments } `
            "teleport_proof.$field*expected zero*" `
            "non-exact teleport cohort field $field passed validation"
        $metrics.teleport_proof[$field] = 0
    }
    $metrics.teleport_proof.loaded_target = 1088
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof loaded/expected cohort counts were not exact*' 'missing destination column passed validation'
    $metrics.teleport_proof.loaded_target = 1089

    $wrongCenterArguments = $fullViewArguments.Clone()
    $wrongCenterArguments.ExpectedTargetCohort = '0:66:65:16'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @wrongCenterArguments } 'teleport_proof target cohort mismatch*' 'wrong destination center passed validation'
    $wrongRadiusArguments = $fullViewArguments.Clone()
    $wrongRadiusArguments.ExpectedTargetCohort = '0:65:65:15'
    Assert-ThrowsLike { Assert-AcceptanceMetrics @wrongRadiusArguments } 'teleport_proof target cohort mismatch*' 'wrong destination radius passed validation'

    $metrics.teleport_proof.committed = $null
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof.committed was missing*' 'missing committed cohort passed validation'
    $metrics.teleport_proof.committed = '0:65:65:16'

    $overlappingCallbackMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('first_gpu_ms=1350.0000', 'first_gpu_ms=1450.0000') `
        -Kind Teleport
    $overlappingCallbackArguments = $fullViewArguments.Clone()
    $overlappingCallbackArguments.TeleportMarker = $overlappingCallbackMarker
    $metrics.teleport_proof.first_gpu_ms = 1450.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics @overlappingCallbackArguments
    $metrics.teleport_proof.first_gpu_ms = 1350.0

    $metrics.teleport_proof.stable_gpu_ms = 'NaN'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof.stable_gpu_ms was not finite*' 'nonfinite GPU-completion timestamp passed validation'
    $metrics.teleport_proof.stable_gpu_ms = 1390.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presentation timestamps were not monotonic*' 'nonmonotonic presentation timestamps passed validation'
    $metrics.teleport_proof.stable_gpu_ms = 1500.0

    $metrics.teleport_proof.stable_frame_sequence = 43
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof frame sequences were not adjacent*' 'non-adjacent presented frames passed validation'
    $metrics.teleport_proof.stable_frame_sequence = 42

    $metrics.teleport_proof.first_presented_manifest_count = 3
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presented manifest count did not equal expected*' 'partial presented manifest count passed validation'
    $metrics.teleport_proof.first_presented_manifest_count = 4
    $metrics.teleport_proof.stable_presented_manifest_hash = '9999000011112222'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presented manifest hash did not equal expected*' 'wrong presented manifest hash passed validation'
    $metrics.teleport_proof.stable_presented_manifest_hash = '1111222233334444'

    foreach ($field in @('missing_target_instances', 'unexpected_target_instances', 'source_instances', 'foreign_instances', 'stale_generation_instances', 'orphan_allocations')) {
        $metrics.forced_full_view_remesh_proof[$field] = 1
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike `
            { Assert-AcceptanceMetrics @fullViewArguments } `
            "forced_full_view_remesh_proof.$field*expected zero*" `
            "forced-remesh render counter $field passed validation"
        $metrics.forced_full_view_remesh_proof[$field] = 0
    }

    $mismatchedTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('resident_hash=aaaabbbbccccdddd', 'resident_hash=0000000000000001') `
        -Kind Teleport
    $mismatchedMarkerArguments = $fullViewArguments.Clone()
    $mismatchedMarkerArguments.TeleportMarker = $mismatchedTeleportMarker
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @mismatchedMarkerArguments } 'teleport marker/metrics mismatch for resident_hash*' 'marker/metrics mismatch passed validation'

    $overCapTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('frame_count=90', 'frame_count=92') `
        -Kind Teleport
    $overCapArguments = $fullViewArguments.Clone()
    $overCapArguments.TeleportMarker = $overCapTeleportMarker
    $metrics.teleport_proof.frame_count = 92
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @overCapArguments } 'teleport_proof exceeded its 60fps cap*' 'per-teleport interval frame cap was not enforced'
    $metrics.teleport_proof.frame_count = 90

    $lateDecodeStageMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('last_sub_ms=900.0000', 'last_sub_ms=1250.0000') `
        -Kind Teleport
    $lateDecodeStageArguments = $fullViewArguments.Clone()
    $lateDecodeStageArguments.TeleportMarker = $lateDecodeStageMarker
    $metrics.teleport_proof.last_sub_ms = 1250.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @lateDecodeStageArguments } 'teleport_proof.last_sub_ms must be JSON null or a nonnegative finite value*' 'a target decode stage after render readiness passed validation'
    $metrics.teleport_proof.last_sub_ms = 900.0

    $missingStageMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('publisher_ms=100.0000', 'publisher_ms=null') `
        -Kind Teleport
    $metrics.teleport_proof.publisher_ms = $null
    $missingStageArguments = $fullViewArguments.Clone()
    $missingStageArguments.TeleportMarker = $missingStageMarker
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics @missingStageArguments
    $metrics.teleport_proof.publisher_ms = -1.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @missingStageArguments } 'teleport_proof.publisher_ms must be JSON null or a nonnegative finite value*' 'missing stage was serialized as -1 without failing'
    $metrics.teleport_proof.publisher_ms = 100.0

    $resourceSamples = @(
        [pscustomobject]@{ combined_rss_bytes = 300MB; cpu_percent = 5.0 },
        [pscustomobject]@{ combined_rss_bytes = 400MB; cpu_percent = 10.0 },
        [pscustomobject]@{ combined_rss_bytes = 350MB; cpu_percent = 15.0 }
    )
    $resourceSummary = Get-SteadyResourceSummary -Samples $resourceSamples
    Assert-Equal (400MB) $resourceSummary.max_combined_rss_bytes 'resource summary chose the wrong RSS maximum'
    Assert-Equal 10.0 $resourceSummary.mean_cpu_percent 'resource summary chose the wrong CPU mean'
    Assert-Equal 15.0 $resourceSummary.p95_cpu_percent 'resource summary chose the wrong CPU p95'
    $resourceTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    $resourceDocument = New-SteadyResourceDocument `
        -Samples $resourceSamples `
        -DurationSeconds 30 `
        -Trigger $resourceTrigger
    Assert-Equal 'rust-mcbe-steady-resources-v2' $resourceDocument.schema 'steady-resource schema did not identify trigger provenance'
    Assert-Equal 'FullViewPresented' $resourceDocument.trigger.kind 'steady-resource trigger kind changed'
    Assert-Equal '0:65:65:16' $resourceDocument.trigger.target 'steady-resource trigger lost its exact target'
    Assert-Equal 7 $resourceDocument.trigger.teleport_view_generation 'steady-resource trigger lost teleport generation'
    Assert-Equal 42 $resourceDocument.trigger.teleport_stable_frame_sequence 'steady-resource trigger lost teleport stable frame'
    Assert-Equal 8 $resourceDocument.trigger.forced_remesh_view_generation 'steady-resource trigger lost forced-remesh generation'
    Assert-Equal 44 $resourceDocument.trigger.forced_remesh_stable_frame_sequence 'steady-resource trigger lost forced-remesh stable frame'

    $metrics.publisher_radius_chunks = 4
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'publisher radius below 16 passed validation'
    $metrics.publisher_radius_chunks = 16
    $metrics.frame_count = 0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'zero frame_count passed validation'
    $metrics.frame_count = 1
    $metrics.p99_frame_ms = 'not-finite'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'nonnumeric p99 passed validation'

    Write-Output 'acceptance.ps1 dry-run tests: PASS'
}
finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force
    }
}
