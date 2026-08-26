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

$ExpectedGophertunnelCommit = '76f1a7016d418277fed9087e39c01cec1c7fd465'
$ExpectedGophertunnelVersion = 'v1.25.3-0.20260816124832-76f1a7016d41'
$ExpectedBdsSha256 = 'e7775e636b9fdcbc354823d92d0c22c12738a2141d12557d856744293d258372'
$ExpectedBdsRelease = '1.26.40.8'
$PinnedAxolotlStackCommit = 'c4540512dc47833bb40363da7ad1161110d64b67'
$PinnedProtocolgenCommit = '870bb549c701a0c03472c66441449c4b70a8454a'
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
$ProjectRootForDependencyResolution = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$PinnedGophertunnelCommit = Get-PinnedGophertunnelCommit `
    -ProjectRoot $ProjectRootForDependencyResolution `
    -ExpectedVersion $ExpectedGophertunnelVersion `
    -ExpectedCommit $ExpectedGophertunnelCommit

if ($env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -eq '1') {
    return
}

Invoke-CinnabarAcceptance @AcceptanceParameters
