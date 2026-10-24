<#
.SYNOPSIS
    Builds or simulates one bounded movement acceptance session against a fixed target.
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')]
    [string]$Target,
    [ValidateRange(60, [int]::MaxValue)]
    [int]$DurationSeconds = 300,
    [ValidateSet('CandidatePhysics', 'FastTransferWitness', 'FreeCameraSilence')]
    [string]$Scenario = 'CandidatePhysics',
    [string]$BdsEndpoint = '127.0.0.1:19132',
    [string]$Assets,
    [string]$OutputDirectory,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$projectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
. (Join-Path $PSScriptRoot 'Common.ps1')
. (Join-Path $PSScriptRoot 'Process.ps1')
. (Join-Path $PSScriptRoot 'MovementLaunch.ps1')
. (Join-Path $PSScriptRoot 'Load.ps1')

$Target = ConvertTo-EvidenceTarget -Target $Target
$Scenario = ConvertTo-MovementScenario -Scenario $Scenario
$bedrockTarget = Get-BedrockTargetManifest -ProjectRoot $projectRoot
if ($Scenario -ceq 'FastTransferWitness' -and [string]::IsNullOrWhiteSpace($Assets)) {
    $Assets = [string]$bedrockTarget.artifacts.world_assets
}

Assert-MovementCleanTrackedSource -ProjectRoot $projectRoot
$buildCommit = (& git -C $projectRoot rev-parse HEAD).Trim()
if ($LASTEXITCODE -ne 0 -or $buildCommit -cnotmatch '^[0-9a-f]{40}$') {
    throw 'failed to resolve exact clean movement build commit'
}
$endpoint = Get-EvidenceTargetEndpoint -Target $Target -BdsEndpoint $BdsEndpoint
$assetsFull = if ([string]::IsNullOrWhiteSpace($Assets)) {
    $null
}
else {
    Resolve-MovementContainedPath -ProjectRoot $projectRoot -Path $Assets -Scope Local -RequireLeaf
}
$runId = [guid]::NewGuid().ToString('N').ToLowerInvariant()
if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $projectRoot ".local\acceptance\movement-$runId"
}
$runDirectory = Resolve-MovementContainedPath -ProjectRoot $projectRoot -Path $OutputDirectory -Scope Acceptance
$metricsPath = Join-Path $runDirectory 'app-metrics.json'
$logPath = Join-Path $runDirectory 'app.stdout.log'
$metadataPath = Join-Path $runDirectory 'run-metadata.json'
$aggregatePath = Join-Path $runDirectory 'movement-final.json'
$validationErrorPath = Join-Path $runDirectory 'validation-error.txt'
$scenarioManifestPath = Join-Path $runDirectory 'scenario-manifest.json'
$launcherErrorPath = Join-Path $runDirectory 'launcher-error.json'
$plan = New-MovementLaunchPlan -Target $Target -Endpoint $endpoint -RunId $runId `
    -MetricsPath $metricsPath `
    -DurationSeconds $DurationSeconds -Scenario $Scenario -Assets $assetsFull

$isWindowsPlatform = [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT
$executableSuffix = if ($isWindowsPlatform) { '.exe' } else { '' }
$buildProfile = if ($Scenario -ceq 'FastTransferWitness') { 'release' } else { 'debug' }
$appExecutable = Join-Path $projectRoot "target\$buildProfile\cinnabar$executableSuffix"
$pregPath = Join-Path $projectRoot (Join-Path '.local\assets' (Split-Path -Leaf ([string]$bedrockTarget.artifacts.physics_registry)))
$bregPath = Resolve-BedrockTargetArtifact -ProjectRoot $projectRoot -Target $bedrockTarget -Artifact block_registry
$assetsSha256 = if ($null -eq $assetsFull) { $null } else {
    (Get-FileHash -Algorithm SHA256 -LiteralPath $assetsFull).Hash.ToLowerInvariant()
}

if ($DryRun) {
    Write-Output "MOVEMENT_TARGET=$Target"
    Write-Output "ACCEPTANCE_ENDPOINT=$endpoint"
    Write-Output "APP_COMMAND=$(Format-ResolvedCommand $appExecutable $plan.AppArguments)"
    Write-Output "MOVEMENT_SCENARIO=$Scenario"
    Write-Output "BUILD_PROFILE=$buildProfile"
    Write-Output "MOVEMENT_CANDIDATE_PHYSICS=$($Scenario -ceq 'CandidatePhysics')"
    Write-Output 'PRODUCTION_PHYSICS_DEFAULT_ENABLED=false'
    return
}

$runDirectory = Initialize-MovementRunDirectory -Path $runDirectory
$runDirectory = Resolve-MovementContainedPath -ProjectRoot $projectRoot -Path $runDirectory -Scope Acceptance
$runDirectoryMarker = "MOVEMENT_RUN_DIRECTORY=$runDirectory"
Write-Output $runDirectoryMarker
if ($Scenario -ceq 'FastTransferWitness') {
    Write-Output 'FAST_TRANSFER_WITNESS_ACTIONS=capture before; run /transfer sm3; capture after; move at least 0.5 blocks'
}
$pregSha256 = $null
$bregSha256 = $null
$appHandle = $null
$appProcessId = $null
$appSha256 = $null
$failurePhase = 'scenario_manifest'
$savedRunId = $env:CINNABAR_ACCEPTANCE_RUN_ID
$savedEndpoint = $env:CINNABAR_ACCEPTANCE_ENDPOINT
$savedBuildCommit = $env:CINNABAR_BUILD_COMMIT
$savedSourceDirty = $env:CINNABAR_SOURCE_DIRTY
$savedUserRoot = $env:CINNABAR_USER_ROOT
try {
    $failurePhase = 'prebuild_identity'
    foreach ($required in @($pregPath, $bregPath)) {
        if (-not (Test-Path -LiteralPath $required -PathType Leaf)) {
            throw "movement launcher requires $required"
        }
    }
    $pregSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant()
    $bregSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant()
    $failurePhase = 'scenario_manifest'
$scenarioManifest = if ($Scenario -ceq 'CandidatePhysics') {
    [ordered]@{
        schema = 'rust-mcbe-movement-scenario-v2'; scenario = 'CandidatePhysics'
        required_input_modes = @('KeyboardMouse', 'GamePad')
        deferred_input_modes = @('Touch')
        input_witness_deferral_reason = 'Owner decision: touch parity is deprioritized; it does not gate movement acceptance and remains open.'
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
        schema = 'rust-mcbe-fast-transfer-witness-scenario-v2'
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
        schema = 'rust-mcbe-movement-scenario-v2'; scenario = 'FreeCameraSilence'
        required_input_modes = @(); deferred_input_modes = @()
        input_witness_deferral_reason = ''; required_perspective_sequence = @()
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

$failurePhase = 'build_app'
Assert-MovementExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
$appBuildArguments = @('build', '--locked', '-p', 'cinnabar')
if ($buildProfile -ceq 'release') { $appBuildArguments += '--release' }
Invoke-CheckedBuild -Executable 'cargo' `
    -Arguments $appBuildArguments `
    -LogPath (Join-Path $runDirectory 'build-app.log') -WorkingDirectory $projectRoot
Assert-MovementExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
$failurePhase = 'prelaunch_identity'
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant() -cne $pregSha256 -or
    (Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant() -cne $bregSha256 -or
    ($null -ne $assetsFull -and
        (Get-FileHash -Algorithm SHA256 -LiteralPath $assetsFull).Hash.ToLowerInvariant() -cne $assetsSha256)) {
    throw 'movement registry or asset carrier identity changed before launch'
}

$appSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $appExecutable).Hash.ToLowerInvariant()
$timedOut = $false
$appExitCode = $null
$lifecycleFailure = $null
$cleanupErrors = [Collections.Generic.List[string]]::new()
try {
    try {
        $failurePhase = 'start_app'
        $env:CINNABAR_ACCEPTANCE_RUN_ID = $runId
        $env:CINNABAR_ACCEPTANCE_ENDPOINT = $endpoint
        $env:CINNABAR_BUILD_COMMIT = $buildCommit
        $env:CINNABAR_SOURCE_DIRTY = 'false'
        if ($Target -ceq 'Bds') { $env:CINNABAR_USER_ROOT = Join-Path $runDirectory 'guest-user' }
        $appHandle = Start-LoggedProcess -Executable $appExecutable -Arguments $plan.AppArguments `
            -WorkingDirectory $projectRoot -StdoutPath $logPath `
            -StderrPath (Join-Path $runDirectory 'app.stderr.log')
        $appProcessId = [int]$appHandle.Process.Id
    }
    finally {
        $env:CINNABAR_ACCEPTANCE_RUN_ID = $savedRunId
        $env:CINNABAR_ACCEPTANCE_ENDPOINT = $savedEndpoint
        $env:CINNABAR_BUILD_COMMIT = $savedBuildCommit
        $env:CINNABAR_SOURCE_DIRTY = $savedSourceDirty
        $env:CINNABAR_USER_ROOT = $savedUserRoot
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
    if ($null -ne $appHandle) {
        try { Complete-ProcessLogs $appHandle }
        catch { $cleanupErrors.Add("complete app logs: $($_.Exception.Message)") }
    }
}
if ($null -ne $lifecycleFailure) { throw $lifecycleFailure }
if ($cleanupErrors.Count -ne 0) { throw ($cleanupErrors -join '; ') }

$failurePhase = 'metadata'
[object[]]$screenshotSlots = @()
if ($Scenario -ceq 'FastTransferWitness') {
    $screenshotSlots = @(
        [ordered]@{ filename = 'fast-transfer-before.png'; sha256 = $null },
        [ordered]@{ filename = 'fast-transfer-after.png'; sha256 = $null }
    )
}
$metadata = [ordered]@{
    schema = 'rust-mcbe-movement-run-v2'; run_id = $runId; target = $Target; endpoint = $endpoint
    build_commit = $buildCommit; source_dirty = $false
    app_sha256 = $appSha256; assets_sha256 = $assetsSha256
    app_process_id = $appProcessId; app_exit_code = $appExitCode; timed_out = $timedOut
    duration_seconds = $DurationSeconds; scenario = $Scenario; screenshot_slots = $screenshotSlots
}
[IO.File]::WriteAllText($metadataPath, ($metadata | ConvertTo-Json -Depth 6) + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))

$failurePhase = 'postrun_identity'
Assert-MovementExactCleanHead -ProjectRoot $projectRoot -ExpectedCommit $buildCommit
if ((Get-FileHash -Algorithm SHA256 -LiteralPath $pregPath).Hash.ToLowerInvariant() -cne $pregSha256 -or
    (Get-FileHash -Algorithm SHA256 -LiteralPath $bregPath).Hash.ToLowerInvariant() -cne $bregSha256 -or
    ($null -ne $assetsFull -and
        (Get-FileHash -Algorithm SHA256 -LiteralPath $assetsFull).Hash.ToLowerInvariant() -cne $assetsSha256)) {
    throw 'movement registry or asset carrier identity changed during the run'
}
$failurePhase = 'validation'
try {
    if ($Scenario -ceq 'FastTransferWitness') {
        & (Join-Path $PSScriptRoot 'FastTransferWitnessValidate.ps1') `
            -LogPath $logPath -RunMetadataPath $metadataPath -MetricsPath $metricsPath `
            -ScenarioManifestPath $scenarioManifestPath -OutputPath $aggregatePath `
            -ExpectedBuildCommit $buildCommit -ExpectedPregSha256 $pregSha256 `
            -ExpectedBregSha256 $bregSha256 `
            -ExpectedProtocol ([uint32]$bedrockTarget.wire_protocol) `
            -ExpectedAppSha256 $appSha256 -ExpectedAssetsSha256 $assetsSha256 -ExpectedRunId $runId `
            -ExpectedAppProcessId $appProcessId -ExpectedPresentMode Fifo
    }
    else {
        & (Join-Path $PSScriptRoot 'MovementEvidence.ps1') `
            -LogPath $logPath -ExpectedTarget $Target -ExpectedBuildCommit $buildCommit `
            -ExpectedPregSha256 $pregSha256 -ExpectedBregSha256 $bregSha256 `
            -ExpectedProtocol ([uint32]$bedrockTarget.wire_protocol) `
            -ExpectedRunId $runId -ExpectedEndpoint $endpoint `
            -ExpectedAppSha256 $appSha256 -ExpectedAppProcessId $appProcessId `
            -RunMetadataPath $metadataPath -MetricsPath $metricsPath -OutputPath $aggregatePath `
            -ScenarioManifestPath $scenarioManifestPath
        if ($LASTEXITCODE -ne 0) { throw "movement evidence validation failed with code $LASTEXITCODE" }
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
        schema = 'rust-mcbe-movement-launcher-error-v1'
        run_id = $runId
        phase = $failurePhase
        reason = $reason
        build_commit = $buildCommit
        preg_sha256 = $pregSha256
        breg_sha256 = $bregSha256
        assets_sha256 = $assetsSha256
        app_sha256 = $appSha256
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
    if ($null -ne $appHandle -and -not $appHandle.Process.HasExited) {
        try { Stop-BoundedProcess -Handle $appHandle -Kind app } catch {}
    }
    $env:CINNABAR_ACCEPTANCE_RUN_ID = $savedRunId
    $env:CINNABAR_ACCEPTANCE_ENDPOINT = $savedEndpoint
    $env:CINNABAR_BUILD_COMMIT = $savedBuildCommit
    $env:CINNABAR_SOURCE_DIRTY = $savedSourceDirty
    $env:CINNABAR_USER_ROOT = $savedUserRoot
}
