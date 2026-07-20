[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RunId,
    [Parameter(Mandatory = $true)][string]$BdsDir,
    [Parameter(Mandatory = $true)][string]$Assets,
    [Parameter(Mandatory = $true)][string]$NativeRoot,
    [Parameter(Mandatory = $true)][int]$DurationSeconds,
    [string]$ClientExecutable,
    [switch]$SkipClientBuild,
    [switch]$ValidateOnly
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'acceptance\Load.ps1')

$ProjectRoot = Get-Phase2ProjectRoot -EntryPath $PSCommandPath
Assert-Phase2Duration -DurationSeconds $DurationSeconds
if ($SkipClientBuild -and [string]::IsNullOrWhiteSpace($ClientExecutable)) {
    throw 'SkipClientBuild requires ClientExecutable'
}
$NativeRootFull = Resolve-Phase2ContainedPath -ProjectRoot $ProjectRoot -Path $NativeRoot -Scope Phase2
$RunDirectory = New-Phase2RunDirectory -ProjectRoot $ProjectRoot -Kind motion -RunId $RunId
$sceneText = "$BdsDir|$Assets|$NativeRootFull|$DurationSeconds"
$sha = [Security.Cryptography.SHA256]::Create()
try {
    $sceneIdentity = ([BitConverter]::ToString($sha.ComputeHash([Text.Encoding]::UTF8.GetBytes($sceneText)))).Replace('-', '').ToLowerInvariant()
}
finally { $sha.Dispose() }
$legs = @('Fifo', 'Immediate') | ForEach-Object {
    [pscustomobject][ordered]@{
        requested_present_mode = $_
        require_effective_present_mode_proof = $true
        scene_identity_sha256 = $sceneIdentity
        duration_seconds = $DurationSeconds
        require_release_build = $true
        require_coherent_stage_identities = $true
        require_temporary_capture_hash = $true
    }
}
$manifest = [pscustomobject][ordered]@{
    schema = 'rust-mcbe-phase2-motion-ab-v1'
    status = if ($ValidateOnly) { 'validated' } else { 'pending' }
    scene_identity_sha256 = $sceneIdentity
    legs = @($legs)
    performance = [pscustomobject][ordered]@{
        warmup_seconds_per_leg = 30
        steady_seconds_per_leg = 120
        resource_samples_per_leg = 120
        p95_frame_ms_max = 16.6666666667
        p99_frame_ms_max = 16.6666666667
        max_frame_ms_max = 50.0
    }
}
Write-Phase2Json -Path (Join-Path $RunDirectory 'manifest.json') -Value $manifest
if ($ValidateOnly) {
    Write-Output "PHASE2_MOTION_DIRECTORY=$RunDirectory"
    return
}
throw 'motion A/B execution is fail-closed until both release legs prove identical scene/camera/build/adapter/driver/assets/duration/resolution identities and effective present modes'
