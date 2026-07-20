[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('Lunar', 'Zeqa')][string]$Server,
    [Parameter(Mandatory = $true)][ValidateSet('Diagnostic', 'Candidate', 'Final')][string]$Mode,
    [Parameter(Mandatory = $true)][string]$RunId,
    [Parameter(Mandatory = $true)][int]$DurationSeconds,
    [Parameter(Mandatory = $true)][string]$AuthCache,
    [Parameter(Mandatory = $true)][int]$InitialRadius,
    [Parameter(Mandatory = $true)][ValidateSet('Fifo')][string]$PresentMode,
    [switch]$FullViewTeleportGate,
    [switch]$OpenSettingsOverlay,
    [Parameter(Mandatory = $true)][string]$Assets,
    [string]$ClientExecutable,
    [switch]$SkipClientBuild,
    [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'acceptance\Load.ps1')

$ProjectRoot = Get-Phase2ProjectRoot -EntryPath $PSCommandPath
Assert-Phase2Duration -DurationSeconds $DurationSeconds
if ($InitialRadius -ne 16) {
    throw 'InitialRadius must be exactly 16; the client uses its authoritative radius-16 contract and receives no radius CLI flag'
}
if ($OpenSettingsOverlay) {
    throw 'OpenSettingsOverlay is unavailable until the Phase 5 adapter is integrated'
}
if ($SkipClientBuild -and [string]::IsNullOrWhiteSpace($ClientExecutable)) {
    throw 'SkipClientBuild requires ClientExecutable'
}
$AuthCacheFull = Resolve-Phase2ContainedPath -ProjectRoot $ProjectRoot -Path $AuthCache -Scope Local
$lunarPrerequisite = $null
if ($Server -eq 'Zeqa') {
    if ($Mode -cne 'Diagnostic' -and -not $FullViewTeleportGate) {
        throw "Zeqa $Mode requires FullViewTeleportGate and a matching Lunar prerequisite"
    }
    $remoteRoot = Join-Path $ProjectRoot '.local\phase2\remote'
    $lunarPrerequisite = Find-Phase2CompletedLunarPrerequisite -RemoteRoot $remoteRoot -Mode $Mode `
        -ExpectedPresentMode $PresentMode -ExpectedInitialRadius $InitialRadius `
        -RequireFullView:$FullViewTeleportGate
    if ($null -eq $lunarPrerequisite) {
        throw "Zeqa $Mode is gated on a hashable completed Lunar $Mode manifest"
    }
}
$RunDirectory = New-Phase2RunDirectory -ProjectRoot $ProjectRoot -Kind remote -RunId $RunId
$upstream = if ($Server -eq 'Lunar') { 'pvp.lunarbedrock.com:19134' } else { 'zeqa.net:19132' }
$clientAcceptanceSeconds = $DurationSeconds + 5
$clientArguments = @(
    '--socket-dir', (Join-Path $RunDirectory 'socket'),
    '--assets', $Assets,
    '--acceptance-seconds', $clientAcceptanceSeconds,
    '--metrics-out', (Join-Path $RunDirectory 'metrics.json')
    '--metrics-warmup-seconds', '30'
    '--metrics-sample-seconds', '120'
)
if ($FullViewTeleportGate) { $clientArguments += '--full-view-teleport-gate' }

$manifest = [pscustomobject][ordered]@{
    schema = 'rust-mcbe-phase2-remote-v1'
    status = if ($ValidateOnly) { 'validated' } else { 'pending' }
    server = $Server
    upstream = $upstream
    mode = $Mode
    diagnostic_complete = $false
    behavior_gate_passed = $false
    world_ready_observed = $false
    join_milliseconds = $null
    publication_snapshot_count = 0
    client_blob_cache_route = $null
    cache_boundary_evidence = $null
    first_stalled_stage = $null
    findings = @()
    metrics_evidence = [pscustomobject][ordered]@{ status = 'pending'; reason = $null }
    resources_evidence = [pscustomobject][ordered]@{ status = 'pending'; reason = $null }
    duration_seconds = $DurationSeconds
    initial_radius = 16
    requested_present_mode = $PresentMode
    require_effective_present_mode_proof = $true
    require_release_build = $true
    auth_cache_scope = '.local'
    full_view_teleport_gate = [bool]$FullViewTeleportGate
    client_arguments = @($clientArguments)
    performance = New-Phase2PerformanceContract
    client_shutdown_grace_seconds = 5
    lunar_prerequisite_mode = if ($null -eq $lunarPrerequisite) { $null } else { $lunarPrerequisite.Mode }
    lunar_prerequisite_manifest_sha256 = if ($null -eq $lunarPrerequisite) { $null } else { $lunarPrerequisite.Sha256 }
}
Write-Phase2Json -Path (Join-Path $RunDirectory 'manifest.json') -Value $manifest

if ($ValidateOnly) {
    Write-Output "PHASE2_RUN_DIRECTORY=$RunDirectory"
    return
}

. (Join-Path $PSScriptRoot 'acceptance\Common.ps1')
. (Join-Path $PSScriptRoot 'acceptance\Process.ps1')
$coreHandle = $null
$clientHandle = $null
$runSucceeded = $false
$clientLogsCompleted = $false
$coreLogsCompleted = $false
try {
    $CoreExecutable = Join-Path $ProjectRoot 'target\release\bedrock-core.exe'
    Invoke-CheckedBuild -Executable 'go' -Arguments @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core') `
        -LogPath (Join-Path $RunDirectory 'build-core.log') -WorkingDirectory $ProjectRoot
    if (-not $SkipClientBuild) {
        Invoke-CheckedBuild -Executable 'cargo' -Arguments @('build', '-p', 'bedrock-client', '--release', '--locked') `
            -LogPath (Join-Path $RunDirectory 'build-client.log') -WorkingDirectory $ProjectRoot
    }
    $ClientExecutableFull = if ([string]::IsNullOrWhiteSpace($ClientExecutable)) {
        Join-Path $ProjectRoot 'target\release\bedrock-client.exe'
    }
    elseif ([IO.Path]::IsPathRooted($ClientExecutable)) {
        [IO.Path]::GetFullPath($ClientExecutable)
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot $ClientExecutable))
    }
    foreach ($executable in @($CoreExecutable, $ClientExecutableFull)) {
        if (-not (Test-Path -LiteralPath $executable -PathType Leaf)) {
            throw "required release executable is missing"
        }
    }
    $socketDirectory = Join-Path $RunDirectory 'socket'
    $coreHandle = Start-LoggedProcess -Executable $CoreExecutable `
        -Arguments @('-socket-dir', $socketDirectory, '-upstream', $upstream, '-auth-cache', $AuthCacheFull) `
        -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'core.stdout.log') `
        -StderrPath (Join-Path $RunDirectory 'core.stderr.log')
    $endpoint = Join-Path $socketDirectory 'game.addr'
    $endpointDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $endpoint -PathType Leaf)) {
        if ($coreHandle.Process.HasExited) { throw 'core exited before publishing its bridge endpoint' }
        if ([DateTime]::UtcNow -ge $endpointDeadline) { throw 'core did not publish its bridge endpoint within 30 seconds' }
        Start-Sleep -Milliseconds 100
    }
    $joinStopwatch = [Diagnostics.Stopwatch]::StartNew()
    $clientHandle = Start-LoggedProcess -Executable $ClientExecutableFull -Arguments $clientArguments `
        -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'client.stdout.log') `
        -StderrPath (Join-Path $RunDirectory 'client.stderr.log')
    try {
        $null = Wait-ProcessOutputMarker -Handle $clientHandle -Marker 'RUST_MCBE_WORLD_READY ' -TimeoutSeconds 180
    }
    catch {
        if ($Mode -cne 'Diagnostic') { throw }
        if ($clientHandle.Process.HasExited -or $coreHandle.Process.HasExited -or
            -not $_.Exception.Message.StartsWith("timed out waiting for 'RUST_MCBE_WORLD_READY '", [StringComparison]::Ordinal)) {
            throw
        }
        $joinStopwatch.Stop()
        Stop-BoundedProcess -Handle $clientHandle -Kind app
        Complete-ProcessLogs $clientHandle
        $clientLogsCompleted = $true
        Stop-BoundedProcess -Handle $coreHandle -Kind core
        Complete-ProcessLogs $coreHandle
        $coreLogsCompleted = $true
        Complete-Phase2DiagnosticEvidence -Manifest $manifest `
            -ClientLogPath (Join-Path $RunDirectory 'client.stdout.log') `
            -CoreLogPath (Join-Path $RunDirectory 'core.stderr.log') `
            -ExpectedPresentMode $PresentMode -WorldReadyObserved:$false -Server $Server
        Write-Phase2Json -Path (Join-Path $RunDirectory 'manifest.json') -Value $manifest
        $runSucceeded = $true
        Write-Output "PHASE2_RUN_DIRECTORY=$RunDirectory"
        return
    }
    $joinStopwatch.Stop()
    $joinMilliseconds = $joinStopwatch.Elapsed.TotalMilliseconds
    $manifest.world_ready_observed = $true
    $resourcesPath = Join-Path $RunDirectory 'resources.json'
    $null = Measure-Phase2Resources -ClientHandle $clientHandle -CoreHandle $coreHandle -OutputPath $resourcesPath
    if (-not $clientHandle.Process.WaitForExit(($DurationSeconds + 30) * 1000)) {
        throw 'client did not exit after its bounded acceptance session'
    }
    Stop-BoundedProcess -Handle $coreHandle -Kind core
    Complete-ProcessLogs $clientHandle
    $clientLogsCompleted = $true
    Complete-ProcessLogs $coreHandle
    $coreLogsCompleted = $true
    $manifest.cache_boundary_evidence = Get-Phase2CacheBoundaryEvidence `
        -CoreLogPath (Join-Path $RunDirectory 'core.stderr.log')
    $evidence = Assert-Phase2Evidence -MetricsPath (Join-Path $RunDirectory 'metrics.json') `
        -ResourcesPath $resourcesPath -ClientLogPath (Join-Path $RunDirectory 'client.stdout.log') `
        -ExpectedPresentMode $PresentMode -JoinMilliseconds $joinMilliseconds `
        -RequireFullView:$FullViewTeleportGate
    $publicationSequence = Get-Phase2PublicationSequenceEvidence `
        -ClientLogPath (Join-Path $RunDirectory 'client.stdout.log') -ExpectedPresentMode $PresentMode `
        -WorldReadyObserved:$true -Server $Server
    if ($Mode -cne 'Diagnostic' -and
        $publicationSequence.FirstStalledStage -cne 'none') {
        throw "binding $Mode evidence retained first stalled stage $($publicationSequence.FirstStalledStage)"
    }
    Assert-Phase2CacheBoundaryConsistency -Server $Server `
        -ClientBlobCacheRoute $publicationSequence.ClientBlobCacheRoute `
        -BoundaryEvidence $manifest.cache_boundary_evidence
    $manifest.status = 'passed'
    $manifest.join_milliseconds = $joinMilliseconds
    $manifest.diagnostic_complete = ($Mode -eq 'Diagnostic')
    $manifest.behavior_gate_passed = ($Mode -cne 'Diagnostic')
    $manifest.publication_snapshot_count = $publicationSequence.SnapshotCount
    $manifest.client_blob_cache_route = $publicationSequence.ClientBlobCacheRoute
    $manifest.first_stalled_stage = $publicationSequence.FirstStalledStage
    $manifest.findings = @($publicationSequence.Findings)
    $manifest.metrics_evidence = [pscustomobject][ordered]@{ status = 'passed'; reason = $null }
    $manifest.resources_evidence = [pscustomobject][ordered]@{ status = 'passed'; reason = $null }
    $manifest | Add-Member -MemberType NoteProperty -Name final_publication -Value $evidence.publication -Force
    Write-Phase2Json -Path (Join-Path $RunDirectory 'manifest.json') -Value $manifest
    $runSucceeded = $true
    Write-Output "PHASE2_RUN_DIRECTORY=$RunDirectory"
}
finally {
    if (-not $runSucceeded) {
        $manifest.status = 'failed'
        Write-Phase2Json -Path (Join-Path $RunDirectory 'manifest.json') -Value $manifest
    }
    if ($null -ne $clientHandle) {
        Stop-BoundedProcess -Handle $clientHandle -Kind app
        if (-not $clientLogsCompleted) { Complete-ProcessLogs $clientHandle }
    }
    if ($null -ne $coreHandle) {
        Stop-BoundedProcess -Handle $coreHandle -Kind core
        if (-not $coreLogsCompleted) { Complete-ProcessLogs $coreHandle }
    }
}
