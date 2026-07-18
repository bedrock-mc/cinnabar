[CmdletBinding()]
param(
    [switch]$DryRun,
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$DurationSeconds,
    [Parameter(Mandatory = $true)]
    [string]$BdsDir,
    [string]$BdsRuntimeDirectory,
    [Parameter(Mandatory = $true)]
    [string]$MetricsOut,
    [string]$Assets,
    [ValidateSet('None', 'Front', 'Back', 'LeafGalleryFront', 'LeafGalleryBack', 'CrossCropGalleryFront', 'CrossCropGalleryBack', 'AquaticGalleryFront', 'AquaticGalleryBack', 'WaterGalleryFront', 'WaterGalleryBack', 'FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite', 'SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite', 'VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')]
    [string]$VisualFixturePose = 'None',
    [switch]$FullViewTeleportGate,
    [switch]$LeafForestBaseline,
    [switch]$LeafForestFullView,
    [string]$ClientExecutable,
    [switch]$SkipClientBuild,
    [switch]$UseVsync,
    [switch]$NoVsync,
    [string]$SteadyResourceTrigger
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$AcceptanceParameters = @{} + $PSBoundParameters

$PinnedGophertunnelCommit = 'bbe6cfdeed39713c2b20103a1294e609d5841615'
$PinnedValentineForkCommit = '6cd8087fc3f0b500e41708a8afc94a0fa3291525'
$PinnedValentineUpstreamCommit = '6f6806e821a579c183c44d786f76d9b358a2b825'
$PinnedValentineLicenseSha256 = '62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184'
$PinnedAssetSourceTag = 'v1.26.30.32-preview'
$PinnedAssetSourceSha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
$LeafStateSuffix = '["persistent_bit"=true,"update_bit"=false]'
$LeafForestOffsetChunks = 65
$LeafForestMutationZOffset = 12
$LeafForestLoadAreaName = 'rust_mcbe_leaf_forest'
$script:AcceptanceEntryRoot = $PSScriptRoot
$LeafForestLoadAreaSettleMilliseconds = 8000


. (Join-Path $PSScriptRoot 'acceptance\Load.ps1')
foreach ($libraryPath in Get-AcceptanceLibraryPaths -EntryPath $PSCommandPath) {
    . $libraryPath
}

if ($env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -eq '1') {
    return
}

Invoke-CinnabarAcceptance @AcceptanceParameters
