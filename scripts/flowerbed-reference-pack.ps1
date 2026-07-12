[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$InputRoot,
    [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$OutputRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PinnedAssetSourceTag = 'v1.26.30.32-preview'
$PinnedAssetSourceSha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'

function Test-PathWithin([string]$Candidate, [string]$Parent) {
    $candidateFull = [IO.Path]::GetFullPath($Candidate).TrimEnd('\')
    $parentFull = [IO.Path]::GetFullPath($Parent).TrimEnd('\')
    return $candidateFull.StartsWith($parentFull + '\', [StringComparison]::OrdinalIgnoreCase)
}

function Assert-NoReparsePath([string]$Path, [string]$StopAt, [string]$Label) {
    $current = [IO.Path]::GetFullPath($Path).TrimEnd('\')
    $stop = [IO.Path]::GetFullPath($StopAt).TrimEnd('\')
    while ($current.Length -ge $stop.Length) {
        if (Test-Path -LiteralPath $current) {
            $item = Get-Item -LiteralPath $current -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "$Label must not traverse a reparse point: $current"
            }
        }
        if ($current -ceq $stop) { return }
        $parent = Split-Path -Parent $current
        if ([string]::IsNullOrEmpty($parent) -or $parent -ceq $current) { break }
        $current = $parent.TrimEnd('\')
    }
    throw "$Label is not rooted beneath its required safety boundary: $Path"
}

function Get-LowerSha256([string]$Path) {
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$localRoot = Join-Path $repoRoot '.local'
New-Item -ItemType Directory -Path $localRoot -Force | Out-Null
$localRoot = (Resolve-Path -LiteralPath $localRoot).Path
if ((Get-Item -LiteralPath $localRoot -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) {
    throw "repository .local must not be a reparse point: $localRoot"
}
if (-not (Test-Path -LiteralPath $InputRoot -PathType Container)) {
    throw "input resource-pack root does not exist: $InputRoot"
}
$inputFull = (Resolve-Path -LiteralPath $InputRoot).Path.TrimEnd('\')
$outputFull = [IO.Path]::GetFullPath($OutputRoot).TrimEnd('\')
if (-not (Test-PathWithin -Candidate $outputFull -Parent $localRoot)) {
    throw "output root must resolve beneath repository .local: $outputFull"
}
Assert-NoReparsePath -Path $outputFull -StopAt $localRoot -Label 'output root'
if ($inputFull -ceq $outputFull -or (Test-PathWithin -Candidate $outputFull -Parent $inputFull) -or (Test-PathWithin -Candidate $inputFull -Parent $outputFull)) {
    throw 'input and output resource-pack roots must not overlap'
}
if (Test-Path -LiteralPath $outputFull) {
    throw "output root already exists: $outputFull"
}

$relativeFiles = @(
    'manifest.json',
    'blocks.json',
    'textures\terrain_texture.json',
    'textures\blocks\wildflowers.png',
    'textures\blocks\wildflowers_stem.png',
    'textures\blocks\pink_petals.png',
    'textures\blocks\pink_petals_stem.png'
)
foreach ($relative in $relativeFiles) {
    $sourcePath = Join-Path $inputFull $relative
    if (-not (Test-Path -LiteralPath $sourcePath -PathType Leaf)) {
        throw "required source pack file is missing: $relative"
    }
    Assert-NoReparsePath -Path $sourcePath -StopAt $inputFull -Label "source file $relative"
}
foreach ($jsonRelative in @('manifest.json', 'blocks.json')) {
    try { $null = Get-Content -Raw -LiteralPath (Join-Path $inputFull $jsonRelative) | ConvertFrom-Json }
    catch { throw "source pack JSON is malformed: $jsonRelative ($($_.Exception.Message))" }
}
$blocksRouting = Get-Content -Raw -LiteralPath (Join-Path $inputFull 'blocks.json')
$terrainRouting = Get-Content -Raw -LiteralPath (Join-Path $inputFull 'textures\terrain_texture.json')
foreach ($requiredToken in @('wildflowers', 'pink_petals')) {
    if (-not $blocksRouting.Contains($requiredToken)) {
        throw "source blocks routing is missing required token: $requiredToken"
    }
}
foreach ($requiredToken in @('wildflowers', 'wildflowers_stem', 'pink_petals', 'pink_petals_stem')) {
    if (-not $terrainRouting.Contains($requiredToken)) {
        throw "source terrain routing is missing required token: $requiredToken"
    }
}

Add-Type -AssemblyName System.Drawing
$imageNames = @('wildflowers', 'wildflowers_stem', 'pink_petals', 'pink_petals_stem')
foreach ($name in $imageNames) {
    $path = Join-Path $inputFull "textures\blocks\$name.png"
    try { $bitmap = [Drawing.Bitmap]::new($path) }
    catch { throw "source image is malformed: textures/blocks/$name.png ($($_.Exception.Message))" }
    try {
        if ($bitmap.Width -ne 16 -or $bitmap.Height -ne 16) {
            throw "source image must be exactly 16x16: textures/blocks/$name.png is $($bitmap.Width)x$($bitmap.Height)"
        }
    }
    finally { $bitmap.Dispose() }
}

$temporaryRoot = Join-Path $localRoot ('.flowerbed-reference-pack.partial-{0}-{1}' -f $PID, [guid]::NewGuid().ToString('N'))
try {
    New-Item -ItemType Directory -Path $temporaryRoot | Out-Null
    foreach ($relative in @('manifest.json')) {
        $destination = Join-Path $temporaryRoot $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath (Join-Path $inputFull $relative) -Destination $destination
    }
    [IO.File]::WriteAllText(
        (Join-Path $temporaryRoot 'blocks.json'),
        '{"format_version":"1.1.0","wildflowers":{"textures":"wildflowers"},"pink_petals":{"textures":"pink_petals"}}',
        [Text.UTF8Encoding]::new($false)
    )
    New-Item -ItemType Directory -Path (Join-Path $temporaryRoot 'textures') -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $temporaryRoot 'textures\terrain_texture.json'),
        '{"resource_pack_name":"flowerbed_reference","texture_name":"atlas.terrain","texture_data":{"wildflowers":{"textures":["textures/blocks/wildflowers","textures/blocks/wildflowers_stem"]},"pink_petals":{"textures":["textures/blocks/pink_petals","textures/blocks/pink_petals_stem"]}}}',
        [Text.UTF8Encoding]::new($false)
    )

    $flowerPalettes = @{
        wildflowers = @(
            [Drawing.Color]::FromArgb(255, 230, 40, 55), [Drawing.Color]::FromArgb(255, 40, 210, 80),
            [Drawing.Color]::FromArgb(255, 45, 90, 235), [Drawing.Color]::FromArgb(255, 245, 205, 35)
        )
        pink_petals = @(
            [Drawing.Color]::FromArgb(255, 235, 55, 190), [Drawing.Color]::FromArgb(255, 35, 220, 220),
            [Drawing.Color]::FromArgb(255, 245, 125, 35), [Drawing.Color]::FromArgb(255, 125, 55, 235)
        )
    }
    foreach ($name in @('wildflowers', 'pink_petals')) {
        $bitmap = [Drawing.Bitmap]::new(16, 16, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $palette = $flowerPalettes[$name]
            for ($y = 0; $y -lt 16; $y++) {
                for ($x = 0; $x -lt 16; $x++) {
                    $quadrant = 0
                    if ($y -ge 8) { $quadrant += 2 }
                    if ($x -ge 8) { $quadrant += 1 }
                    $bitmap.SetPixel($x, $y, $palette[$quadrant])
                }
            }
            $destination = Join-Path $temporaryRoot "textures\blocks\$name.png"
            New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
            $bitmap.Save($destination, [Drawing.Imaging.ImageFormat]::Png)
        }
        finally { $bitmap.Dispose() }
    }
    foreach ($name in @('wildflowers_stem', 'pink_petals_stem')) {
        $bitmap = [Drawing.Bitmap]::new(16, 16, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
        try {
            $primary = if ($name -ceq 'wildflowers_stem') { [Drawing.Color]::FromArgb(255, 20, 245, 110) } else { [Drawing.Color]::FromArgb(255, 245, 40, 150) }
            $secondary = if ($name -ceq 'wildflowers_stem') { [Drawing.Color]::FromArgb(255, 5, 70, 240) } else { [Drawing.Color]::FromArgb(255, 245, 220, 20) }
            $grid = if ($name -ceq 'wildflowers_stem') { [Drawing.Color]::FromArgb(255, 250, 250, 250) } else { [Drawing.Color]::FromArgb(255, 20, 20, 20) }
            for ($y = 0; $y -lt 16; $y++) {
                for ($x = 0; $x -lt 16; $x++) {
                    $colour = if (($x % 4) -eq 0) { $grid } elseif ((($x + $y) % 2) -eq 0) { $primary } else { $secondary }
                    $bitmap.SetPixel($x, $y, $colour)
                }
            }
            $destination = Join-Path $temporaryRoot "textures\blocks\$name.png"
            $bitmap.Save($destination, [Drawing.Imaging.ImageFormat]::Png)
        }
        finally { $bitmap.Dispose() }
    }

    $fileEvidence = @($relativeFiles | ForEach-Object {
        $portablePath = $_.Replace('\', '/')
        [pscustomobject][ordered]@{
            path = $portablePath
            generated = $portablePath.EndsWith('.png', [StringComparison]::Ordinal)
            input_sha256 = Get-LowerSha256 -Path (Join-Path $inputFull $_)
            output_sha256 = Get-LowerSha256 -Path (Join-Path $temporaryRoot $_)
        }
    })
    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-flowerbed-reference-pack-v1'
        pinned_source = [pscustomobject][ordered]@{
            tag = $PinnedAssetSourceTag
            archive_sha256 = $PinnedAssetSourceSha256
        }
        generated_filenames = @('wildflowers.png', 'wildflowers_stem.png', 'pink_petals.png', 'pink_petals_stem.png')
        files = $fileEvidence
    }
    [IO.File]::WriteAllText(
        (Join-Path $temporaryRoot 'flowerbed-reference-manifest.json'),
        ($manifest | ConvertTo-Json -Depth 8),
        [Text.UTF8Encoding]::new($false)
    )
    New-Item -ItemType Directory -Path (Split-Path -Parent $outputFull) -Force | Out-Null
    Move-Item -LiteralPath $temporaryRoot -Destination $outputFull
    $temporaryRoot = $null
    Write-Output "FLOWERBED_REFERENCE_PACK=$outputFull"
    Write-Output "FLOWERBED_REFERENCE_MANIFEST_SHA256=$(Get-LowerSha256 -Path (Join-Path $outputFull 'flowerbed-reference-manifest.json'))"
}
finally {
    if ($null -ne $temporaryRoot -and (Test-Path -LiteralPath $temporaryRoot)) {
        Remove-Item -LiteralPath $temporaryRoot -Recurse -Force
    }
}
