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
$DryRunDirectory = Join-Path $ProjectRoot '.local\acceptance\dry-run'

try {
    New-Item -ItemType Directory -Path $BdsDir -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $BdsDir 'bedrock_server.exe') -Value 'fixture' -NoNewline
    Set-Content -LiteralPath $Assets -Value 'assets fixture' -NoNewline
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

    $mutationCoordinate = @(101, 64, -37)
    $frontPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Front
    $frontPlanAgain = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Front
    $backPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Back
    $teleportPlan = New-FullViewTeleportPlan -MutationCoordinate $mutationCoordinate

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
    Publish-VisualFixture `
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
    Assert-Equal ($frontPlan.Commands -join "`n") ((Get-Content -LiteralPath $fixtureLogPath) -join "`n") 'fixture console log did not record every command in order'
    Assert-Equal ($frontPlan.Commands -join [Environment]::NewLine) $fixtureInput.ToString().TrimEnd("`r", "`n") 'fixture commands were not sent through the owned standard input in order'
    Assert-Equal $frontPlan.FenceMarker $script:ObservedFixtureFence 'fixture publisher did not wait for the processing fence'
    $fixtureReady = Get-Content -Raw -LiteralPath $fixtureReadyPath | ConvertFrom-Json
    Assert-Equal 'Front' $fixtureReady.pose 'fixture ready artifact recorded the wrong pose'
    Assert-Equal 'list' $fixtureReady.processing_fence.command 'fixture ready artifact recorded the wrong fence command'
    Assert-Equal 'players online:' $fixtureReady.processing_fence.stdout_marker 'fixture ready artifact recorded the wrong fence marker'
    Assert-Equal 3000 $fixtureReady.settle_milliseconds 'fixture ready artifact did not record the production settle duration'
    Assert-Equal $frontPlan.TeleportCommand $fixtureReady.teleport_command 'fixture ready artifact recorded the wrong teleport'

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

    $metrics = [ordered]@{
        session_seconds = 900.0; world_ready = $true; requested_radius_chunks = 16
        received_radius_chunks = 16; publisher_radius_chunks = 16
        mutation_coordinate = @(1, 2, 3); visible_mutation_count = 1; frame_count = 1
        p50_frame_ms = 1.0; p95_frame_ms = 2.0; p99_frame_ms = 3.0; max_frame_ms = 4.0
        max_decode_ms = 1.0; max_mesh_ms = 1.0; max_remesh_ms = 1.0
        full_view_teleport_ms = $null
        max_mutation_to_visible_ms = 50.0; decode_error_count = 0
        rendered_sub_chunks = 1; resident_sub_chunks = 1; visible_sub_chunks = 1
        peak_admitted_world_events = 1; peak_admitted_heavy_events = 1
        peak_queued_decode_jobs = 1; peak_in_flight_decode_jobs = 1
        peak_completed_decode_results = 1; peak_pending_retry_requests = 1
        peak_outbound_requests = 1; peak_pending_mesh_jobs = 1
        peak_in_flight_mesh_jobs = 1; gpu_upload_bytes = 1
    }
    $metricsPath = Join-Path $TempRoot 'validation-metrics.json'
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath

    $metrics.full_view_teleport_ms = 1500.0
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireFullViewTeleport
    $metrics.full_view_teleport_ms = 2000.1
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath -RequireFullViewTeleport } 'over-budget full-view teleport passed validation'
    $metrics.full_view_teleport_ms = $null

    $resourceSamples = @(
        [pscustomobject]@{ combined_rss_bytes = 300MB; cpu_percent = 5.0 },
        [pscustomobject]@{ combined_rss_bytes = 400MB; cpu_percent = 10.0 },
        [pscustomobject]@{ combined_rss_bytes = 350MB; cpu_percent = 15.0 }
    )
    $resourceSummary = Get-SteadyResourceSummary -Samples $resourceSamples
    Assert-Equal (400MB) $resourceSummary.max_combined_rss_bytes 'resource summary chose the wrong RSS maximum'
    Assert-Equal 10.0 $resourceSummary.mean_cpu_percent 'resource summary chose the wrong CPU mean'
    Assert-Equal 15.0 $resourceSummary.p95_cpu_percent 'resource summary chose the wrong CPU p95'

    $metrics.publisher_radius_chunks = 4
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'publisher radius below 16 passed validation'
    $metrics.publisher_radius_chunks = 16
    $metrics.frame_count = 0
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'zero frame_count passed validation'
    $metrics.frame_count = 1
    $metrics.p99_frame_ms = 'not-finite'
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'nonnumeric p99 passed validation'

    Write-Output 'acceptance.ps1 dry-run tests: PASS'
}
finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force
    }
}
