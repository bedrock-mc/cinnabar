$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Invoke-Builder([string]$Source, [string]$Output) {
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $lines = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $Builder -InputRoot $Source -OutputRoot $Output 2>&1
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = @($lines | ForEach-Object ToString) }
    }
    finally { $ErrorActionPreference = $previousPreference }
}

function New-TestPng([string]$Path, [int]$Width = 16, [int]$Height = 16) {
    Add-Type -AssemblyName System.Drawing
    $parent = Split-Path -Parent $Path
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $bitmap = [Drawing.Bitmap]::new($Width, $Height, [Drawing.Imaging.PixelFormat]::Format32bppArgb)
    try {
        for ($y = 0; $y -lt $Height; $y++) {
            for ($x = 0; $x -lt $Width; $x++) {
                $bitmap.SetPixel($x, $y, [Drawing.Color]::FromArgb(255, 40 + $x, 80 + $y, 120))
            }
        }
        $bitmap.Save($Path, [Drawing.Imaging.ImageFormat]::Png)
    }
    finally { $bitmap.Dispose() }
}

function New-TestPack([string]$Root) {
    New-Item -ItemType Directory -Path (Join-Path $Root 'textures\blocks') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $Root 'manifest.json'), '{"format_version":2,"header":{"name":"fixture","uuid":"11111111-1111-1111-1111-111111111111","version":[1,0,0]},"modules":[{"type":"resources","uuid":"22222222-2222-2222-2222-222222222222","version":[1,0,0]}]}', [Text.UTF8Encoding]::new($false))
    [IO.File]::WriteAllText((Join-Path $Root 'blocks.json'), '{"format_version":"1.1.0","wildflowers":{"textures":"wildflowers"},"pink_petals":{"textures":"pink_petals"},"unrelated":{"textures":"unrelated"}}', [Text.UTF8Encoding]::new($false))
    New-Item -ItemType Directory -Path (Join-Path $Root 'textures') -Force | Out-Null
    [IO.File]::WriteAllText((Join-Path $Root 'textures\terrain_texture.json'), "// pinned Mojang routing permits comments`n" + '{"resource_pack_name":"fixture","texture_name":"atlas.terrain","texture_data":{"wildflowers":{"textures":["textures/blocks/wildflowers","textures/blocks/wildflowers_stem"]},"pink_petals":{"textures":["textures/blocks/pink_petals","textures/blocks/pink_petals_stem"]},"unrelated":{"textures":"textures/blocks/unrelated"}}}', [Text.UTF8Encoding]::new($false))
    foreach ($name in @('wildflowers', 'wildflowers_stem', 'pink_petals', 'pink_petals_stem')) {
        New-TestPng -Path (Join-Path $Root "textures\blocks\$name.png")
    }
}

function Get-TreeIdentity([string]$Root) {
    return @(Get-ChildItem -LiteralPath $Root -File -Recurse | Sort-Object { $_.FullName.Substring($Root.Length) } | ForEach-Object {
        '{0}|{1}' -f $_.FullName.Substring($Root.Length).Replace('\', '/'), (Get-FileHash -Algorithm SHA256 -LiteralPath $_.FullName).Hash.ToLowerInvariant()
    }) -join "`n"
}

$RepoRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$Builder = Join-Path $RepoRoot 'scripts\flowerbed-reference-pack.ps1'
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("rust-mcbe flowerbed pack tests {0}" -f [guid]::NewGuid().ToString('N'))
$Source = Join-Path $TempRoot 'pinned source'
$Output = Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\pack-a'
$OutputAgain = Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\pack-b'

try {
    New-TestPack -Root $Source
    $sourceBefore = Get-TreeIdentity -Root $Source
    $first = Invoke-Builder -Source $Source -Output $Output
    Assert-True ($first.ExitCode -eq 0) "valid builder invocation failed: $($first.Output -join [Environment]::NewLine)"
    $second = Invoke-Builder -Source $Source -Output $OutputAgain
    Assert-True ($second.ExitCode -eq 0) "repeat builder invocation failed: $($second.Output -join [Environment]::NewLine)"
    Assert-True ((Get-TreeIdentity -Root $Output) -ceq (Get-TreeIdentity -Root $OutputAgain)) 'repeated builds were not byte-identical'
    Assert-True ($sourceBefore -ceq (Get-TreeIdentity -Root $Source)) 'builder modified the pinned source pack'

    $relativeFiles = @(Get-ChildItem -LiteralPath $Output -File -Recurse | ForEach-Object { $_.FullName.Substring($Output.Length + 1).Replace('\', '/') } | Sort-Object)
    $expectedFiles = @('blocks.json', 'flowerbed-reference-manifest.json', 'manifest.json', 'textures/blocks/pink_petals.png', 'textures/blocks/pink_petals_stem.png', 'textures/blocks/wildflowers.png', 'textures/blocks/wildflowers_stem.png', 'textures/terrain_texture.json')
    Assert-True (($relativeFiles -join ',') -ceq ($expectedFiles -join ',')) "builder copied more than the minimum pack: $($relativeFiles -join ',')"
    Assert-True (-not (Get-Content -Raw -LiteralPath (Join-Path $Output 'blocks.json')).Contains('unrelated')) 'builder retained unrelated block routing'
    Assert-True (-not (Get-Content -Raw -LiteralPath (Join-Path $Output 'textures\terrain_texture.json')).Contains('unrelated')) 'builder retained unrelated terrain routing'
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $Output 'flowerbed-reference-manifest.json') | ConvertFrom-Json
    Assert-True ($manifest.schema -ceq 'rust-mcbe-flowerbed-reference-pack-v1') 'builder manifest schema changed'
    Assert-True ($manifest.pinned_source.tag -ceq 'v1.26.30.32-preview') 'builder manifest lost the pinned Mojang source tag'
    Assert-True (@($manifest.files).Count -eq 7) 'builder manifest did not identify every source/output file pair'
    foreach ($file in $manifest.files) {
        Assert-True ([string]$file.input_sha256 -match '^[0-9a-f]{64}$') "missing input hash for $($file.path)"
        Assert-True ([string]$file.output_sha256 -match '^[0-9a-f]{64}$') "missing output hash for $($file.path)"
    }

    Add-Type -AssemblyName System.Drawing
    foreach ($name in @('wildflowers', 'pink_petals')) {
        $bitmap = [Drawing.Bitmap]::new((Join-Path $Output "textures\blocks\$name.png"))
        try {
            Assert-True ($bitmap.Width -eq 16 -and $bitmap.Height -eq 16) "$name output dimensions changed"
            $colours = @($bitmap.GetPixel(2, 2).ToArgb(), $bitmap.GetPixel(10, 2).ToArgb(), $bitmap.GetPixel(2, 10).ToArgb(), $bitmap.GetPixel(10, 10).ToArgb() | Sort-Object -Unique)
            Assert-True ($colours.Count -eq 4) "$name did not contain four unique opaque quadrants"
            Assert-True (@($colours | Where-Object { [Drawing.Color]::FromArgb($_).A -eq 255 }).Count -eq 4) "$name quadrants were not opaque"
        }
        finally { $bitmap.Dispose() }
    }
    foreach ($name in @('wildflowers_stem', 'pink_petals_stem')) {
        $bitmap = [Drawing.Bitmap]::new((Join-Path $Output "textures\blocks\$name.png"))
        try {
            Assert-True ($bitmap.Width -eq 16 -and $bitmap.Height -eq 16) "$name output dimensions changed"
            Assert-True ($bitmap.GetPixel(0, 0).ToArgb() -ne $bitmap.GetPixel(1, 0).ToArgb()) "$name did not contain an orientation grid/stripe"
            Assert-True ($bitmap.GetPixel(0, 0).A -eq 255 -and $bitmap.GetPixel(1, 0).A -eq 255) "$name pattern was not opaque"
        }
        finally { $bitmap.Dispose() }
    }

    $outside = Invoke-Builder -Source $Source -Output (Join-Path $TempRoot 'outside local')
    Assert-True ($outside.ExitCode -ne 0) 'builder accepted output outside repository .local'

    $overlapSource = Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\overlap-source'
    New-TestPack -Root $overlapSource
    $overlap = Invoke-Builder -Source $overlapSource -Output (Join-Path $overlapSource 'output')
    Assert-True ($overlap.ExitCode -ne 0) 'builder accepted overlapping source/output roots'

    $escapeTarget = Join-Path $TempRoot 'junction escape target'
    $escapeLink = Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\escape-link'
    New-Item -ItemType Directory -Path $escapeTarget -Force | Out-Null
    $null = New-Item -ItemType Junction -Path $escapeLink -Target $escapeTarget
    $escape = Invoke-Builder -Source $Source -Output (Join-Path $escapeLink 'pack')
    Assert-True ($escape.ExitCode -ne 0) 'builder accepted a reparse-point escape from .local'

    $missingSource = Join-Path $TempRoot 'missing image source'
    New-TestPack -Root $missingSource
    Remove-Item -LiteralPath (Join-Path $missingSource 'textures\blocks\wildflowers.png')
    Assert-True ((Invoke-Builder -Source $missingSource -Output (Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\missing')).ExitCode -ne 0) 'builder accepted a missing image'

    $malformedSource = Join-Path $TempRoot 'malformed image source'
    New-TestPack -Root $malformedSource
    [IO.File]::WriteAllText((Join-Path $malformedSource 'textures\blocks\pink_petals.png'), 'not a png')
    Assert-True ((Invoke-Builder -Source $malformedSource -Output (Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\malformed')).ExitCode -ne 0) 'builder accepted a malformed image'

    $wrongSizeSource = Join-Path $TempRoot 'wrong size source'
    New-TestPack -Root $wrongSizeSource
    New-TestPng -Path (Join-Path $wrongSizeSource 'textures\blocks\pink_petals_stem.png') -Width 15 -Height 16
    Assert-True ((Invoke-Builder -Source $wrongSizeSource -Output (Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests\wrong-size')).ExitCode -ne 0) 'builder accepted a wrong-size image'

    Write-Output 'flowerbed reference pack tests passed'
}
finally {
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath (Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests') -Recurse -Force -ErrorAction SilentlyContinue
}
