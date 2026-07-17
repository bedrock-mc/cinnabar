$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

function Assert-True {
    param(
        [Parameter(Mandatory = $true)][bool]$Condition,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param(
        [Parameter(Mandatory = $true)]$Expected,
        [Parameter(Mandatory = $true)]$Actual,
        [Parameter(Mandatory = $true)][string]$Message
    )
    if ($Expected -cne $Actual) {
        throw "$Message (expected '$Expected', got '$Actual')"
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$manifestPath = Join-Path $repoRoot "assets\cloudburst-data-sources.json"
Assert-True (Test-Path -LiteralPath $manifestPath -PathType Leaf) "Cloudburst source manifest is missing"

$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
Assert-Equal 1 ([int]$manifest.schema) "manifest schema changed"
Assert-Equal "1.26.30" ([string]$manifest.protocol.game_version) "game version changed"
Assert-Equal 1001 ([int]$manifest.protocol.protocol_version) "protocol version changed"
Assert-Equal "local-only" ([string]$manifest.artifact_policy) "artifact policy changed"
Assert-Equal "supplementary-validation-and-generation" ([string]$manifest.intended_use) "intended use changed"
Assert-True (-not [bool]$manifest.redistribution) "Cloudburst payloads must not be redistributed"
Assert-True (-not [bool]$manifest.runtime_fetch) "the client must not fetch Cloudburst data at runtime"

$sources = @($manifest.sources)
Assert-Equal 1 $sources.Count "manifest must pin one Cloudburst snapshot"
$source = $sources[0]
$commit = "fb969c547236d87a17181941cd585a0eb18f7ceb"
Assert-Equal "cloudburstmc-data" ([string]$source.id) "source id changed"
Assert-Equal "https://github.com/CloudburstMC/Data" ([string]$source.repository) "repository changed"
Assert-Equal $commit ([string]$source.commit) "Cloudburst commit changed"
Assert-Equal "cloudburst" ([string]$source.destination) "destination changed"
Assert-Equal "upstream-unspecified" ([string]$source.license_status) "license status changed"
Assert-True (-not [bool]$source.redistribution) "source payloads must not be redistributed"
Assert-True (-not ($source.PSObject.Properties.Name -ccontains "license")) "manifest must not claim an upstream license"

$expectedFiles = @(
    "biome_definitions.json",
    "block_attributes.json",
    "block_palette.nbt",
    "block_properties.json",
    "creative_contents.dat",
    "creative_items.json",
    "entity_identifiers.dat",
    "entity_properties.nbt",
    "item_components.nbt",
    "item_mappings.json",
    "legacy_block_ids.json",
    "legacy_item_ids.json",
    "recipes.json",
    "runtime_item_states.json",
    "stripped_biome_definitions.json"
)
$files = @($source.files)
Assert-Equal $expectedFiles.Count $files.Count "pinned file count changed"
Assert-Equal ($expectedFiles -join ",") ((@($files | ForEach-Object { [string]$_.install_path }) | Sort-Object) -join ",") "pinned file inventory changed"

$totalBytes = 0L
foreach ($file in $files) {
    $path = [string]$file.upstream_path
    Assert-Equal $path ([string]$file.install_path) "install path must preserve upstream filename"
    Assert-Equal "https://raw.githubusercontent.com/CloudburstMC/Data/$commit/$path" ([string]$file.url) "file URL is not commit-pinned"
    Assert-True ([string]$file.sha256 -cmatch "^[0-9a-f]{64}$") "invalid SHA-256 for $path"
    Assert-True ([long]$file.size_bytes -gt 0L) "invalid size for $path"
    Assert-True ([long]$file.size_bytes -le [long]$manifest.limits.max_file_bytes) "file exceeds manifest ceiling: $path"
    $totalBytes += [long]$file.size_bytes
}
Assert-Equal 27523566L $totalBytes "pinned snapshot byte total changed"
Assert-True ($totalBytes -le [long]$manifest.limits.max_total_bytes) "bundle exceeds manifest ceiling"

$makefile = Get-Content -Raw -LiteralPath (Join-Path $repoRoot "Makefile")
Assert-True ($makefile -cmatch "(?m)^cloudburst-data:") "Makefile is missing the Cloudburst acquisition target"
Assert-True ($makefile -cmatch "assets/cloudburst-data-sources\.json") "Makefile does not use the pinned Cloudburst manifest"

$readme = Get-Content -Raw -LiteralPath (Join-Path $repoRoot "README.md")
Assert-True ($readme -cmatch "make cloudburst-data") "README is missing the Cloudburst acquisition command"
Assert-True ($readme -cmatch "upstream license is unspecified") "README must preserve the upstream license-status warning"
Assert-True ($readme -cmatch "must not be committed or\s+redistributed") "README must preserve the redistribution warning"

Write-Output "Cloudburst source manifest tests passed."
