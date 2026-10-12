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

$ExpectedBdsSha256 = '19c88569af2e4b7d984e999055a31cbcb0799dacf8bbbf7371eda42f5772a443'
$ExpectedBdsRelease = '1.26.52.3'
$PinnedAxolotlStackCommit = 'c4540512dc47833bb40363da7ad1161110d64b67'
$PinnedProtocolgenCommit = 'dec7334ac3981b0b79146fa53c82af91d70f9f62'
$PinnedJolyneLicenseSha256 = '62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184'
$PinnedAssetSource = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot '..\assets\vanilla-source.json') | ConvertFrom-Json
$PinnedAssetSourceTag = [string]$PinnedAssetSource.tag
$PinnedAssetSourceSha256 = [string]$PinnedAssetSource.sha256
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
$ProjectRootForDependencyResolution = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$PinnedGophertunnelCommit = Get-PinnedGophertunnelCommit -ProjectRoot $ProjectRootForDependencyResolution

if ($env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -eq '1') {
    return
}

Invoke-CinnabarAcceptance @AcceptanceParameters
