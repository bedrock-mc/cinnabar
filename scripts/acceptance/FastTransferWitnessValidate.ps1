[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$LogPath,
    [Parameter(Mandatory = $true)][string]$RunMetadataPath,
    [Parameter(Mandatory = $true)][string]$MetricsPath,
    [Parameter(Mandatory = $true)][string]$ScenarioManifestPath,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [Parameter(Mandatory = $true)][string]$ExpectedBuildCommit,
    [Parameter(Mandatory = $true)][string]$ExpectedPregSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedBregSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedCoreSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedAppSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedAssetsSha256,
    [Parameter(Mandatory = $true)][string]$ExpectedRunId,
    [Parameter(Mandatory = $true)][string]$ExpectedBridgeEndpoint,
    [Parameter(Mandatory = $true)][int]$ExpectedCoreProcessId,
    [Parameter(Mandatory = $true)][int]$ExpectedAppProcessId,
    [ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode = 'Fifo'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot 'FastTransferWitnessValidation.ps1')

$result = Assert-FastTransferWitnessEvidence @PSBoundParameters
Write-Output (
    'FAST_TRANSFER_WITNESS_VALID ' +
    "run=$($result.run_id) build=$($result.build_commit) " +
    "post_reset_delta=$($result.post_reset_network_position_delta) " +
    "physics_packets=$($result.terminal_physics_packet_count)"
)
