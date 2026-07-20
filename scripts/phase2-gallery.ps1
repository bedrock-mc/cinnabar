[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateSet('Biome', 'LightingAtmosphere', 'Precipitation', 'Celestial', 'Cloud')][string]$Gallery,
    [Parameter(Mandatory = $true)][string]$RunId,
    [Parameter(Mandatory = $true)][string]$BdsDir,
    [Parameter(Mandatory = $true)][string]$Assets,
    [Parameter(Mandatory = $true)][string]$NativeRoot,
    [Parameter(Mandatory = $true)][ValidateSet('Fifo')][string]$PresentMode,
    [int]$DurationSeconds = 180,
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
$null = Resolve-Phase2ContainedPath -ProjectRoot $ProjectRoot -Path $NativeRoot -Scope Phase2
$RunDirectory = New-Phase2RunDirectory -ProjectRoot $ProjectRoot -Kind galleries -RunId $RunId
$kinds = switch ($Gallery) {
    'Biome' { @('biome') }
    'LightingAtmosphere' { @('lighting', 'fog-air', 'fog-water', 'fog-lava') }
    'Precipitation' { @('fog-air') }
    'Celestial' { @('celestial') }
    'Cloud' { @('cloud') }
}
$comparisons = @($kinds | ForEach-Object {
    [pscustomobject][ordered]@{
        kind = $_
        command = @(
            'cargo', 'run', '-p', 'phase2-evidence', '--locked', '--', 'compare',
            '--kind', $_, '--manifest', "<manifest:$_>", '--native', "<native:$_>",
            '--cinnabar', "<cinnabar:$_>", '--out', "<report:$_>"
        )
    }
})
$manifest = [pscustomobject][ordered]@{
    schema = 'rust-mcbe-phase2-gallery-v1'
    status = if ($ValidateOnly) { 'validated' } else { 'pending' }
    gallery = $Gallery
    duration_seconds = $DurationSeconds
    requested_present_mode = 'Fifo'
    require_effective_present_mode_proof = $true
    require_release_build = $true
    comparisons = $comparisons
    performance = New-Phase2PerformanceContract
}
Write-Phase2Json -Path (Join-Path $RunDirectory 'manifest.json') -Value $manifest
if ($ValidateOnly) {
    Write-Output "PHASE2_GALLERY_DIRECTORY=$RunDirectory"
    return
}
throw 'gallery execution is fail-closed until every manifested native/Cinnabar capture path exists and the release/FIFO performance witness is available'
