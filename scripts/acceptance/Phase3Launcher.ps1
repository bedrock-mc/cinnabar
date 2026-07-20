[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg')]
    [string]$Target,
    [ValidateRange(60, [int]::MaxValue)]
    [int]$DurationSeconds = 300,
    [ValidateSet('CandidatePhysics', 'FastTransferWitness', 'FreeCameraSilence')]
    [string]$Scenario = 'CandidatePhysics',
    [string]$BdsEndpoint = '127.0.0.1:19132',
    [string]$AuthCache,
    [string]$Assets,
    [string]$OutputDirectory,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
. (Join-Path $PSScriptRoot 'Common.ps1')
. (Join-Path $PSScriptRoot 'Process.ps1')
. (Join-Path $PSScriptRoot 'Phase3Launch.ps1')
. (Join-Path $PSScriptRoot 'Load.ps1')

Assert-Phase3CleanTrackedSource -ProjectRoot $projectRoot
$buildCommit = (& git -C $projectRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $buildCommit -cnotmatch '^[0-9a-f]{40}$') {
    throw 'failed to resolve exact clean Phase 3 build commit'
}
$endpoint = Get-Phase3TargetEndpoint -Target $Target -BdsEndpoint $BdsEndpoint
$authCacheFull = if ($Target -ceq 'Bds' -or [string]::IsNullOrWhiteSpace($AuthCache)) {
    $null
}
else {
    Resolve-Phase3ContainedPath -ProjectRoot $projectRoot -Path $AuthCache -Scope Local -RequireLeaf
}
$assetsFull = if ([string]::IsNullOrWhiteSpace($Assets)) {
    $null
}
else {
    Resolve-Phase3ContainedPath -ProjectRoot $projectRoot -Path $Assets -Scope Local -RequireLeaf
}
$runId = [guid]::NewGuid().ToString('N').ToLowerInvariant()
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $projectRoot ".local\acceptance\phase3-$runId"
}
$runDirectory = Resolve-Phase3ContainedPath -ProjectRoot $projectRoot -Path $OutputDirectory -Scope Acceptance
$socketDirectory = Join-Path $runDirectory 'socket'
$metricsPath = Join-Path $runDirectory 'app-metrics.json'
$logPath = Join-Path $runDirectory 'app.stdout.log'
$metadataPath = Join-Path $runDirectory 'run-metadata.json'
$aggregatePath = Join-Path $runDirectory 'phase3-final.json'
$validationErrorPath = Join-Path $runDirectory 'validation-error.txt'
$scenarioManifestPath = Join-Path $runDirectory 'scenario-manifest.json'
$launcherErrorPath = Join-Path $runDirectory 'launcher-error.json'
$plan = New-Phase3LaunchPlan -Target $Target -Endpoint $endpoint -RunId $runId `
    -SocketDirectory $socketDirectory -MetricsPath $metricsPath `
    -DurationSeconds $DurationSeconds -Scenario $Scenario -AuthCache $authCacheFull -Assets $assetsFull

$isWindowsPlatform = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
$executableSuffix = if ($isWindowsPlatform) { '.exe' } else { '' }
$coreExecutable = Join-Path $projectRoot "target\debug\bedrock-core$executableSuffix"
$appExecutable = Join-Path $projectRoot "target\debug\bedrock-client$executableSuffix"
$pregPath = Join-Path $projectRoot '.local\assets\block-physics-v1001.bin'
$bregPath = Join-Path $projectRoot 'crates\assets\data\block-registry-v1001.bin'
$assetsSha256 = if ($null -eq $assetsFull) { $null } else {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $assetsFull).Hash.ToLowerInvariant()
}

if ($DryRun) {
    Write-Output "PHASE3_TARGET=$Target"
    Write-Output "PHASE3_ENDPOINT=$endpoint"
    Write-Output "CORE_COMMAND=$(Format-ResolvedCommand $coreExecutable $plan.CoreArguments)"
    Write-Output "APP_COMMAND=$(Format-ResolvedCommand $appExecutable $plan.AppArguments)"
    Write-Output "PHASE3_SCENARIO=$Scenario"
    Write-Output "PHASE3_CANDIDATE_PHYSICS=$($Scenario -cin @('CandidatePhysics', 'FastTransferWitness'))"
    Write-Output 'PRODUCTION_PHYSICS_DEFAULT_ENABLED=false'
    return
}

$runDirectory = Initialize-Phase3RunDirectory -Path $runDirectory
$runDirectory = Resolve-Phase3ContainedPath -ProjectRoot $projectRoot -Path $runDirectory -Scope Acceptance
$runDirectoryMarker = "PHASE3_RUN_DIRECTORY=$runDirectory"
Write-Output $runDirectoryMarker
if ($Scenario -ceq 'FastTransferWitness') {
    Write-Output 'FAST_TRANSFER_WITNESS_ACTIONS=capture before; run /transfer sm3; capture after; move at least 0.5 blocks'
}
$pregSha256 = $null
$bregSha256 = $null
$endpointGuard = $null
$coreHandle = $null
$appHandle = $null
$coreProcessId = $null
$appProcessId = $null
$bridgeEndpoint = $null
$coreSha256 = $null
$appSha256 = $null
$failurePhase = 'scenario_manifest'
$savedRunId = $env:RUST_MCBE_PHASE3_RUN_ID
$savedEndpoint = $env:RUST_MCBE_PHASE3_ENDPOINT
$savedBridgeEndpoint = $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT
$savedCoreSha256 = $env:RUST_MCBE_PHASE3_CORE_SHA256
$savedCoreProcessId = $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID
try {
    $failurePhase = 'prebuild_identity'
    foreach ($required in @($pregPath, $bregPath)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "Phase 3 launcher requires $required"
        }
    }
    $pregSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant()
    $bregSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant()
    $failurePhase = 'endpoint_guard'
    $endpointGuard = New-Phase3EndpointPublicationGuard -SocketDirectory $socketDirectory
    $failurePhase = 'scenario_manifest'
$scenarioManifest = if ($Scenario -ceq 'CandidatePhysics') {
    [ordered]@{
        schema = 'rust-mcbe-phase3-scenario-v1'; scenario = 'CandidatePhysics'
        required_input_modes = @('KeyboardMouse', 'GamePad', 'Touch')
        required_perspective_sequence = @(
            'FirstPerson', 'ThirdPersonBack', 'ThirdPersonFront', 'FirstPerson'
        )
        require_replay = $true; require_snap = $true; require_held_jump_rejump = $true
        require_release_before_landing = $true; require_camera_blocked = $true
        require_camera_fallback = $true; require_avatar_visibility_states = $true
        required_controlled_matrix = [ordered]@{
            sprint = $true; sneak_ledge = $true; slabs_stairs = $true; ladder = $true
            liquids = @('Water', 'Lava')
            special_surfaces = @('Cobweb', 'Slime', 'Bed', 'SoulSand', 'Honey', 'BubbleColumn')
            knockback = $true; teleport = $true; dimension_change = $true
            focus_loss = $true; controller_disconnect = $true
            frame_caps = @(30, 60, 144); targeting_ray_invariant = $true
            flat_walk_min_magnitude = 0.25; diagonal_walk_min_axis_magnitude = 0.25
            single_jump_non_repeated_min_count = 1
            camera_wall_outcome = 'WallBlocked'; camera_corner_outcome = 'CornerBlocked'
            camera_ceiling_outcome = 'CeilingBlocked'
        }
    }
}
elseif ($Scenario -ceq 'FastTransferWitness') {
    [ordered]@{
        schema = 'rust-mcbe-fast-transfer-witness-scenario-v1'
        scenario = 'FastTransferWitness'
        target = 'Lbsg'
        required_command = '/transfer sm3'
        assets_sha256 = $assetsSha256
        maximum_command_to_reset_arm_milliseconds = 30000
        minimum_post_reset_network_position_delta = 0.5
        minimum_duration_seconds = 600
        screenshot_slots = @(
            [ordered]@{ filename = 'fast-transfer-before.png'; sha256 = $null },
            [ordered]@{ filename = 'fast-transfer-after.png'; sha256 = $null }
        )
    }
}
else {
    [ordered]@{
        schema = 'rust-mcbe-phase3-scenario-v1'; scenario = 'FreeCameraSilence'
        required_input_modes = @(); required_perspective_sequence = @()
        require_replay = $false; require_snap = $false; require_held_jump_rejump = $false
        require_release_before_landing = $false; require_camera_blocked = $false
        require_camera_fallback = $false; require_avatar_visibility_states = $false
        required_controlled_matrix = [ordered]@{
            sprint = $false; sneak_ledge = $false; slabs_stairs = $false; ladder = $false
            liquids = @(); special_surfaces = @(); knockback = $false; teleport = $false
            dimension_change = $false; focus_loss = $false; controller_disconnect = $false
            frame_caps = @(); targeting_ray_invariant = $false
            flat_walk_min_magnitude = 0.0; diagonal_walk_min_axis_magnitude = 0.0
            single_jump_non_repeated_min_count = 0
            camera_wall_outcome = 'NotRequired'; camera_corner_outcome = 'NotRequired'
            camera_ceiling_outcome = 'NotRequired'
        }
    }
}
[IO.File]::WriteAllText(
    $scenarioManifestPath,
    ($scenarioManifest | ConvertTo-Json -Depth 6) + [Environment]::NewLine,
    [Text.UTF8Encoding]::new($false)
)

$savedBuildCommit = $env:RUST_MCBE_BUILD_COMMIT
$savedSourceDirty = $env:RUST_MCBE_SOURCE_DIRTY
try {
    $failurePhase = 'build_app'
    Assert-Phase3ExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
    $env:RUST_MCBE_BUILD_COMMIT = $buildCommit
    $env:RUST_MCBE_SOURCE_DIRTY = 'false'
    Invoke-CheckedBuild -Executable 'cargo' `
        -Arguments @('build', '--locked', '-p', 'bedrock-client') `
        -LogPath (Join-Path $runDirectory 'build-app.log') -WorkingDirectory $projectRoot
}
finally {
    $env:RUST_MCBE_BUILD_COMMIT = $savedBuildCommit
    $env:RUST_MCBE_SOURCE_DIRTY = $savedSourceDirty
}
Assert-Phase3ExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
$failurePhase = 'build_core'
Invoke-CheckedBuild -Executable 'go' `
    -Arguments @('build', '-trimpath', '-o', $coreExecutable, './core/cmd/bedrock-core') `
    -LogPath (Join-Path $runDirectory 'build-core.log') -WorkingDirectory $projectRoot
Assert-Phase3ExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
$failurePhase = 'prelaunch_identity'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant() -cne $pregSha256 -or
    (Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant() -cne $bregSha256 -or
    ($null -ne $assetsFull -and
        (Get-FileHash -Algorithm SHA256 -LiteralPath $assetsFull).Hash.ToLowerInvariant() -cne $assetsSha256)) {
    throw 'Phase 3 registry or asset carrier identity changed before launch'
}

$coreSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $coreExecutable).Hash.ToLowerInvariant()
$appSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $appExecutable).Hash.ToLowerInvariant()
$timedOut = $false
$appExitCode = $null
$coreExitCode = $null
$coreTerminatedByLauncher = $false
$lifecycleFailure = $null
$cleanupErrors = [Collections.Generic.List[string]]::new()
try {
    $failurePhase = 'start_core'
    $coreHandle = Start-LoggedProcess -Executable $coreExecutable -Arguments $plan.CoreArguments `
        -WorkingDirectory $projectRoot -StdoutPath (Join-Path $runDirectory 'core.stdout.log') `
        -StderrPath (Join-Path $runDirectory 'core.stderr.log')
    $coreProcessId = [int]$coreHandle.Process.Id
    $failurePhase = 'wait_endpoint'
    $endpointWitness = Wait-Phase3BridgeEndpoint -Guard $endpointGuard -CoreHandle $coreHandle `
        -TimeoutSeconds 30
    $bridgeEndpoint = [string]$endpointWitness.Endpoint

    try {
        $failurePhase = 'start_app'
        $env:RUST_MCBE_PHASE3_RUN_ID = $runId
        $env:RUST_MCBE_PHASE3_ENDPOINT = $endpoint
        $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT = $bridgeEndpoint
        $env:RUST_MCBE_PHASE3_CORE_SHA256 = $coreSha256
        $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID = $coreProcessId.ToString([Globalization.CultureInfo]::InvariantCulture)
        $appHandle = Start-LoggedProcess -Executable $appExecutable -Arguments $plan.AppArguments `
            -WorkingDirectory $projectRoot -StdoutPath $logPath `
            -StderrPath (Join-Path $runDirectory 'app.stderr.log')
        $appProcessId = [int]$appHandle.Process.Id
    }
    finally {
        $env:RUST_MCBE_PHASE3_RUN_ID = $savedRunId
        $env:RUST_MCBE_PHASE3_ENDPOINT = $savedEndpoint
        $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT = $savedBridgeEndpoint
        $env:RUST_MCBE_PHASE3_CORE_SHA256 = $savedCoreSha256
        $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID = $savedCoreProcessId
    }
    $failurePhase = 'wait_app'
    if (-not $appHandle.Process.WaitForExit(($DurationSeconds + 120) * 1000)) {
        $timedOut = $true
        Stop-BoundedProcess -Handle $appHandle -Kind app
    }
    $appExitCode = $appHandle.Process.ExitCode
}
catch {
    $lifecycleFailure = $_
}
finally {
    if ($null -ne $appHandle -and -not $appHandle.Process.HasExited) {
        try { Stop-BoundedProcess -Handle $appHandle -Kind app }
        catch { $cleanupErrors.Add("stop app: $($_.Exception.Message)") }
    }
    if ($null -ne $coreHandle -and -not $coreHandle.Process.HasExited) {
        $coreTerminatedByLauncher = $true
        try { Stop-BoundedProcess -Handle $coreHandle -Kind core }
        catch { $cleanupErrors.Add("stop core: $($_.Exception.Message)") }
    }
    if ($null -ne $coreHandle -and $coreHandle.Process.HasExited) { $coreExitCode = $coreHandle.Process.ExitCode }
    if ($null -ne $appHandle) {
        try { Complete-ProcessLogs $appHandle }
        catch { $cleanupErrors.Add("complete app logs: $($_.Exception.Message)") }
    }
    if ($null -ne $coreHandle) {
        try { Complete-ProcessLogs $coreHandle }
        catch { $cleanupErrors.Add("complete core logs: $($_.Exception.Message)") }
    }
}
if ($null -ne $lifecycleFailure) { throw $lifecycleFailure }
if ($cleanupErrors.Count -ne 0) { throw ($cleanupErrors -join '; ') }

$failurePhase = 'metadata'
$screenshotSlots = if ($Scenario -ceq 'FastTransferWitness') {
    @(
        [ordered]@{ filename = 'fast-transfer-before.png'; sha256 = $null },
        [ordered]@{ filename = 'fast-transfer-after.png'; sha256 = $null }
    )
}
else { @() }
$metadata = [ordered]@{
    schema = 'rust-mcbe-phase3-run-v1'; run_id = $runId; target = $Target; endpoint = $endpoint
    bridge_endpoint = $bridgeEndpoint
    build_commit = $buildCommit; source_dirty = $false; core_sha256 = $coreSha256
    app_sha256 = $appSha256; assets_sha256 = $assetsSha256; core_process_id = $coreProcessId
    app_process_id = $appProcessId; app_exit_code = $appExitCode; core_exit_code = $coreExitCode
    core_terminated_by_launcher = $coreTerminatedByLauncher; timed_out = $timedOut
    duration_seconds = $DurationSeconds; scenario = $Scenario; screenshot_slots = $screenshotSlots
}
[IO.File]::WriteAllText($metadataPath, ($metadata | ConvertTo-Json -Depth 6) + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))

$failurePhase = 'postrun_identity'
Assert-Phase3ExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant() -cne $pregSha256 -or
    (Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant() -cne $bregSha256 -or
    ($null -ne $assetsFull -and
        (Get-FileHash -Algorithm SHA256 -LiteralPath $assetsFull).Hash.ToLowerInvariant() -cne $assetsSha256)) {
    throw 'Phase 3 registry or asset carrier identity changed during the run'
}
$failurePhase = 'validation'
try {
    if ($Scenario -ceq 'FastTransferWitness') {
        & (Join-Path $PSScriptRoot 'FastTransferWitnessValidate.ps1') `
            -LogPath $logPath -RunMetadataPath $metadataPath -MetricsPath $metricsPath `
            -ScenarioManifestPath $scenarioManifestPath -OutputPath $aggregatePath `
            -ExpectedBuildCommit $buildCommit -ExpectedPregSha256 $pregSha256 `
            -ExpectedBregSha256 $bregSha256 -ExpectedCoreSha256 $coreSha256 `
            -ExpectedAppSha256 $appSha256 -ExpectedAssetsSha256 $assetsSha256 -ExpectedRunId $runId `
            -ExpectedBridgeEndpoint $bridgeEndpoint -ExpectedCoreProcessId $coreProcessId `
            -ExpectedAppProcessId $appProcessId -ExpectedPresentMode Fifo
    }
    else {
        & (Join-Path $PSScriptRoot 'Phase3.ps1') `
            -LogPath $logPath -ExpectedTarget $Target -ExpectedBuildCommit $buildCommit `
            -ExpectedPregSha256 $pregSha256 -ExpectedBregSha256 $bregSha256 `
            -ExpectedRunId $runId -ExpectedEndpoint $endpoint -ExpectedBridgeEndpoint $bridgeEndpoint `
            -ExpectedCoreSha256 $coreSha256 `
            -ExpectedCoreProcessId $coreProcessId -ExpectedAppProcessId $appProcessId `
            -RunMetadataPath $metadataPath -MetricsPath $metricsPath -OutputPath $aggregatePath `
            -ScenarioManifestPath $scenarioManifestPath
        if ($LASTEXITCODE -ne 0) { throw "Phase 3 evidence validation failed with code $LASTEXITCODE" }
    }
}
catch {
    [IO.File]::WriteAllText(
        $validationErrorPath,
        $_.Exception.Message + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    throw
}
}
catch {
    $reason = [string]$_.Exception.Message
    if ($reason.Length -gt 2048) { $reason = $reason.Substring(0, 2048) }
    $availableLogs = @(
        Get-ChildItem -LiteralPath $runDirectory -File -ErrorAction SilentlyContinue |
            Where-Object { $_.Name -like '*.log' } |
            ForEach-Object {
                $logSha256 = $null
                try { $logSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant() }
                catch {}
                [ordered]@{
                    name = $_.Name
                    sha256 = $logSha256
                }
            }
    )
    $failure = [ordered]@{
        schema = 'rust-mcbe-phase3-launcher-error-v1'
        run_id = $runId
        phase = $failurePhase
        reason = $reason
        build_commit = $buildCommit
        preg_sha256 = $pregSha256
        breg_sha256 = $bregSha256
        assets_sha256 = $assetsSha256
        core_sha256 = $coreSha256
        app_sha256 = $appSha256
        core_process_id = $coreProcessId
        app_process_id = $appProcessId
        logs = $availableLogs
    }
    $launcherErrorTemporaryPath = "$launcherErrorPath.tmp"
    [IO.File]::WriteAllText(
        $launcherErrorTemporaryPath,
        ($failure | ConvertTo-Json -Depth 6) + [Environment]::NewLine,
        [Text.UTF8Encoding]::new($false)
    )
    Move-Item -LiteralPath $launcherErrorTemporaryPath -Destination $launcherErrorPath -Force
    throw
}
finally {
    foreach ($entry in @(@($appHandle, 'app'), @($coreHandle, 'core'))) {
        $handle = $entry[0]
        if ($null -ne $handle -and -not $handle.Process.HasExited) {
            try { Stop-BoundedProcess -Handle $handle -Kind ([string]$entry[1]) } catch {}
        }
    }
    $env:RUST_MCBE_PHASE3_RUN_ID = $savedRunId
    $env:RUST_MCBE_PHASE3_ENDPOINT = $savedEndpoint
    $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT = $savedBridgeEndpoint
    $env:RUST_MCBE_PHASE3_CORE_SHA256 = $savedCoreSha256
    $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID = $savedCoreProcessId
}
