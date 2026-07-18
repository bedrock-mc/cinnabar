[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg')]
    [string]$Target,
    [ValidateRange(60, [int]::MaxValue)]
    [int]$DurationSeconds = 300,
    [ValidateSet('CandidatePhysics', 'FreeCameraSilence')]
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
    $resolved = Resolve-Phase2ContainedPath -ProjectRoot $projectRoot -Path $AuthCache -Scope Local
    if (-not (Test-Path -LiteralPath $resolved -PathType Leaf)) {
        throw "Phase 3 authentication cache does not exist: $resolved"
    }
    $resolved
}
$runId = [guid]::NewGuid().ToString('N').ToLowerInvariant()
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $projectRoot ".local\acceptance\phase3-$runId"
}
$runDirectory = [IO.Path]::GetFullPath($OutputDirectory)
$socketDirectory = Join-Path $runDirectory 'socket'
$metricsPath = Join-Path $runDirectory 'app-metrics.json'
$logPath = Join-Path $runDirectory 'app.stdout.log'
$metadataPath = Join-Path $runDirectory 'run-metadata.json'
$aggregatePath = Join-Path $runDirectory 'phase3-final.json'
$scenarioManifestPath = Join-Path $runDirectory 'scenario-manifest.json'
$plan = New-Phase3LaunchPlan -Target $Target -Endpoint $endpoint -RunId $runId `
    -SocketDirectory $socketDirectory -MetricsPath $metricsPath `
    -DurationSeconds $DurationSeconds -Scenario $Scenario -AuthCache $authCacheFull -Assets $Assets

$isWindowsPlatform = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
$executableSuffix = if ($isWindowsPlatform) { '.exe' } else { '' }
$coreExecutable = Join-Path $projectRoot "target\debug\bedrock-core$executableSuffix"
$appExecutable = Join-Path $projectRoot "target\debug\bedrock-client$executableSuffix"
$pregPath = Join-Path $projectRoot '.local\assets\block-physics-v1001.bin'
$bregPath = Join-Path $projectRoot 'crates\assets\data\block-registry-v1001.bin'

if ($DryRun) {
    Write-Output "PHASE3_TARGET=$Target"
    Write-Output "PHASE3_ENDPOINT=$endpoint"
    Write-Output "CORE_COMMAND=$(Format-ResolvedCommand $coreExecutable $plan.CoreArguments)"
    Write-Output "APP_COMMAND=$(Format-ResolvedCommand $appExecutable $plan.AppArguments)"
    Write-Output "PHASE3_SCENARIO=$Scenario"
    Write-Output "PHASE3_CANDIDATE_PHYSICS=$($Scenario -ceq 'CandidatePhysics')"
    Write-Output 'PRODUCTION_PHYSICS_DEFAULT_ENABLED=false'
    return
}

foreach ($required in @($pregPath, $bregPath)) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
        throw "Phase 3 launcher requires $required"
    }
}
$runDirectory = Initialize-Phase3RunDirectory -Path $runDirectory
$endpointGuard = New-Phase3EndpointPublicationGuard -SocketDirectory $socketDirectory
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
Invoke-CheckedBuild -Executable 'go' `
    -Arguments @('build', '-trimpath', '-o', $coreExecutable, './core/cmd/bedrock-core') `
    -LogPath (Join-Path $runDirectory 'build-core.log') -WorkingDirectory $projectRoot

$coreSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $coreExecutable).Hash.ToLowerInvariant()
$appSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $appExecutable).Hash.ToLowerInvariant()
$coreHandle = $null
$appHandle = $null
$timedOut = $false
$appExitCode = $null
$coreExitCode = $null
$coreTerminatedByLauncher = $false
try {
    $coreHandle = Start-LoggedProcess -Executable $coreExecutable -Arguments $plan.CoreArguments `
        -WorkingDirectory $projectRoot -StdoutPath (Join-Path $runDirectory 'core.stdout.log') `
        -StderrPath (Join-Path $runDirectory 'core.stderr.log')
    $endpointWitness = Wait-Phase3BridgeEndpoint -Guard $endpointGuard -CoreHandle $coreHandle `
        -TimeoutSeconds 30
    $bridgeEndpoint = [string]$endpointWitness.Endpoint

    $savedRunId = $env:RUST_MCBE_PHASE3_RUN_ID
    $savedEndpoint = $env:RUST_MCBE_PHASE3_ENDPOINT
    $savedBridgeEndpoint = $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT
    $savedCoreSha256 = $env:RUST_MCBE_PHASE3_CORE_SHA256
    $savedCoreProcessId = $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID
    try {
        $env:RUST_MCBE_PHASE3_RUN_ID = $runId
        $env:RUST_MCBE_PHASE3_ENDPOINT = $endpoint
        $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT = $bridgeEndpoint
        $env:RUST_MCBE_PHASE3_CORE_SHA256 = $coreSha256
        $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID = $coreHandle.Process.Id.ToString([Globalization.CultureInfo]::InvariantCulture)
        $appHandle = Start-LoggedProcess -Executable $appExecutable -Arguments $plan.AppArguments `
            -WorkingDirectory $projectRoot -StdoutPath $logPath `
            -StderrPath (Join-Path $runDirectory 'app.stderr.log')
    }
    finally {
        $env:RUST_MCBE_PHASE3_RUN_ID = $savedRunId
        $env:RUST_MCBE_PHASE3_ENDPOINT = $savedEndpoint
        $env:RUST_MCBE_PHASE3_BRIDGE_ENDPOINT = $savedBridgeEndpoint
        $env:RUST_MCBE_PHASE3_CORE_SHA256 = $savedCoreSha256
        $env:RUST_MCBE_PHASE3_CORE_PROCESS_ID = $savedCoreProcessId
    }
    if (-not $appHandle.Process.WaitForExit(($DurationSeconds + 120) * 1000)) {
        $timedOut = $true
        Stop-BoundedProcess -Handle $appHandle -Kind app
    }
    $appExitCode = $appHandle.Process.ExitCode
}
finally {
    if ($null -ne $coreHandle -and -not $coreHandle.Process.HasExited) {
        $coreTerminatedByLauncher = $true
        Stop-BoundedProcess -Handle $coreHandle -Kind core
    }
    if ($null -ne $coreHandle -and $coreHandle.Process.HasExited) { $coreExitCode = $coreHandle.Process.ExitCode }
    Complete-ProcessLogs $appHandle
    Complete-ProcessLogs $coreHandle
}

$metadata = [ordered]@{
    schema = 'rust-mcbe-phase3-run-v1'; run_id = $runId; target = $Target; endpoint = $endpoint
    bridge_endpoint = $bridgeEndpoint
    build_commit = $buildCommit; source_dirty = $false; core_sha256 = $coreSha256
    app_sha256 = $appSha256; core_process_id = $coreHandle.Process.Id
    app_process_id = $appHandle.Process.Id; app_exit_code = $appExitCode; core_exit_code = $coreExitCode
    core_terminated_by_launcher = $coreTerminatedByLauncher; timed_out = $timedOut
    duration_seconds = $DurationSeconds; scenario = $Scenario
}
[IO.File]::WriteAllText($metadataPath, ($metadata | ConvertTo-Json -Depth 6) + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))

& (Join-Path $PSScriptRoot 'Phase3.ps1') `
    -LogPath $logPath -ExpectedTarget $Target -ExpectedBuildCommit $buildCommit `
    -ExpectedPregSha256 ((Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant()) `
    -ExpectedBregSha256 ((Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant()) `
    -ExpectedRunId $runId -ExpectedEndpoint $endpoint -ExpectedBridgeEndpoint $bridgeEndpoint `
    -ExpectedCoreSha256 $coreSha256 `
    -ExpectedCoreProcessId $coreHandle.Process.Id -ExpectedAppProcessId $appHandle.Process.Id `
    -RunMetadataPath $metadataPath -MetricsPath $metricsPath -OutputPath $aggregatePath `
    -ScenarioManifestPath $scenarioManifestPath
if ($LASTEXITCODE -ne 0) { throw "Phase 3 evidence validation failed with code $LASTEXITCODE" }
