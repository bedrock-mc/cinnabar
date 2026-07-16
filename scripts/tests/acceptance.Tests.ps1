. (Join-Path $PSScriptRoot 'acceptance\Assertions.ps1')
. (Join-Path $PSScriptRoot 'acceptance\Fixtures.ps1')

$AcceptanceScript = Join-Path $ProjectRoot 'scripts\acceptance.ps1'
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("rust-mcbe acceptance tests {0}" -f [guid]::NewGuid().ToString('N'))
$BdsDir = Join-Path $TempRoot 'bds source'
$MetricsOut = Join-Path $TempRoot 'metrics output\metrics.json'
$Assets = Join-Path $TempRoot 'vanilla assets with spaces.mcpack'
$CrossCropAssets = Join-Path $TempRoot 'compiled cross crop assets.mcbea'
$AquaticAssets = Join-Path $TempRoot 'compiled aquatic assets.mcbea'
$SlabStairAssets = Join-Path $TempRoot 'compiled slab stair assets.mcbea'
$BlockRegistry = Join-Path $ProjectRoot 'crates\assets\data\block-registry-v1001.bin'
$PrebuiltClient = Join-Path $TempRoot 'opaque base client\bedrock-client.exe'
$DryRunDirectory = Join-Path $ProjectRoot '.local\acceptance\dry-run'
$testFailure = $null
$tempRootCleanupFailure = $null

try {
    . (Join-Path $PSScriptRoot 'acceptance\Paths.Tests.ps1')
    . (Join-Path $PSScriptRoot 'acceptance\Galleries.Tests.ps1')
    . (Join-Path $PSScriptRoot 'acceptance\Orchestration.Tests.ps1')
    . (Join-Path $PSScriptRoot 'acceptance\Markers.Tests.ps1')
    . (Join-Path $PSScriptRoot 'acceptance\Metrics.Tests.ps1')
}
catch {
    $testFailure = $_
}
finally {
    try {
        if (Test-Path -LiteralPath $TempRoot) {
            Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction Stop
        }
        if (Test-Path -LiteralPath $TempRoot) {
            throw "acceptance test temporary directory still exists after cleanup: $TempRoot"
        }
    }
    catch {
        $tempRootCleanupFailure = $_
    }
}

if ($null -ne $testFailure) {
    if ($null -ne $tempRootCleanupFailure) {
        Write-Warning "temporary-directory cleanup also failed: $($tempRootCleanupFailure.Exception.Message)"
    }
    throw $testFailure
}
if ($null -ne $tempRootCleanupFailure) {
    throw $tempRootCleanupFailure
}

Write-Output 'acceptance.ps1 dry-run tests: PASS'
