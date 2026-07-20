$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Invoke-Builder([string]$Source, [string]$Output, [string]$SourceIdentity) {
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $Builder, '-InputRoot', $Source, '-OutputRoot', $Output)
        if (-not [string]::IsNullOrWhiteSpace($SourceIdentity)) {
            $arguments += @('-SourceIdentityPath', $SourceIdentity)
        }
        $lines = & powershell.exe @arguments 2>&1
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = @($lines | ForEach-Object ToString) }
    }
    finally { $ErrorActionPreference = $previousPreference }
}

function Write-TestSourceIdentity([string]$Root, [string]$Path, [string]$Tag = 'test-fixture-v1', [string]$ArchiveSha256 = ('a' * 64)) {
    $relativeFiles = @(
        'manifest.json',
        'blocks.json',
        'textures\terrain_texture.json',
        'textures\blocks\wildflowers.png',
        'textures\blocks\wildflowers_stem.png',
        'textures\blocks\pink_petals.png',
        'textures\blocks\pink_petals_stem.png'
    )
    $identity = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-flowerbed-source-identity-v1'
        tag = $Tag
        archive_sha256 = $ArchiveSha256
        files = @($relativeFiles | ForEach-Object {
            [pscustomobject][ordered]@{
                path = $_.Replace('\', '/')
                sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Root $_)).Hash.ToLowerInvariant()
            }
        })
    }
    [IO.File]::WriteAllText($Path, ($identity | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
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
$SourceIdentity = Join-Path $TempRoot 'test-source-identity.json'
$LocalTestParent = Join-Path $RepoRoot '.local\flowerbed-reference-pack-tests'
$OwnedRoot = Join-Path $LocalTestParent ("run-{0}" -f [guid]::NewGuid().ToString('N'))
$PreExistingSibling = Join-Path $LocalTestParent ("pre-existing-sibling-{0}" -f [guid]::NewGuid().ToString('N'))
$SiblingMarker = Join-Path $PreExistingSibling 'preserve.txt'
$Output = Join-Path $OwnedRoot 'pack-a'
$OutputAgain = Join-Path $OwnedRoot 'pack-b'
$ownedRootRemoved = $false

try {
    New-TestPack -Root $Source
    Write-TestSourceIdentity -Root $Source -Path $SourceIdentity
    New-Item -ItemType Directory -Path $OwnedRoot, $PreExistingSibling -Force | Out-Null
    [IO.File]::WriteAllText($SiblingMarker, 'must survive owned cleanup', [Text.UTF8Encoding]::new($false))
    $sourceBefore = Get-TreeIdentity -Root $Source
    $first = Invoke-Builder -Source $Source -Output $Output -SourceIdentity $SourceIdentity
    Assert-True ($first.ExitCode -eq 0) "valid builder invocation failed: $($first.Output -join [Environment]::NewLine)"
    $second = Invoke-Builder -Source $Source -Output $OutputAgain -SourceIdentity $SourceIdentity
    Assert-True ($second.ExitCode -eq 0) "repeat builder invocation failed: $($second.Output -join [Environment]::NewLine)"
    Assert-True ((Get-TreeIdentity -Root $Output) -ceq (Get-TreeIdentity -Root $OutputAgain)) 'repeated builds were not byte-identical'
    Assert-True ($sourceBefore -ceq (Get-TreeIdentity -Root $Source)) 'builder modified the pinned source pack'

    $relativeFiles = @(Get-ChildItem -LiteralPath $Output -File -Recurse | ForEach-Object { $_.FullName.Substring($Output.Length + 1).Replace('\', '/') } | Sort-Object)
    $expectedFiles = @('blocks.json', 'flowerbed-reference-manifest.json', 'manifest.json', 'textures/blocks/pink_petals.png', 'textures/blocks/pink_petals_stem.png', 'textures/blocks/wildflowers.png', 'textures/blocks/wildflowers_stem.png', 'textures/terrain_texture.json')
    Assert-True (($relativeFiles -join ',') -ceq ($expectedFiles -join ',')) "builder copied more than the minimum pack: $($relativeFiles -join ',')"
    Assert-True (-not (Get-Content -Raw -LiteralPath (Join-Path $Output 'blocks.json')).Contains('unrelated')) 'builder retained unrelated block routing'
    Assert-True (-not (Get-Content -Raw -LiteralPath (Join-Path $Output 'textures\terrain_texture.json')).Contains('unrelated')) 'builder retained unrelated terrain routing'
    $manifest = Get-Content -Raw -LiteralPath (Join-Path $Output 'flowerbed-reference-manifest.json') | ConvertFrom-Json
    Assert-True ($manifest.schema -ceq 'rust-mcbe-flowerbed-reference-pack-v2') 'builder manifest schema changed'
    Assert-True ($manifest.source_identity.schema -ceq 'rust-mcbe-flowerbed-source-identity-v1') 'builder manifest lost verified source identity schema'
    Assert-True ($manifest.source_identity.tag -ceq 'test-fixture-v1') 'builder manifest did not preserve the verified fixture source tag'
    Assert-True ($manifest.source_identity.archive_sha256 -ceq ('a' * 64)) 'builder manifest did not preserve the verified fixture archive identity'
    Assert-True ([string]$manifest.source_identity.identity_sha256 -match '^[0-9a-f]{64}$') 'builder manifest lost canonical verified source identity hash'
    Assert-True (@($manifest.files).Count -eq 7) 'builder manifest did not identify every source/output file pair'
    foreach ($file in $manifest.files) {
        Assert-True ([string]$file.input_sha256 -match '^[0-9a-f]{64}$') "missing input hash for $($file.path)"
        Assert-True ([string]$file.output_sha256 -match '^[0-9a-f]{64}$') "missing output hash for $($file.path)"
        $actualSourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $Source ([string]$file.path).Replace('/', '\'))).Hash.ToLowerInvariant()
        Assert-True ([string]$file.input_sha256 -ceq $actualSourceHash) "manifest source hash was not bound to actual content for $($file.path)"
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

    $outside = Invoke-Builder -Source $Source -Output (Join-Path $TempRoot 'outside local') -SourceIdentity $SourceIdentity
    Assert-True ($outside.ExitCode -ne 0) 'builder accepted output outside repository .local'

    $unverified = Invoke-Builder -Source $Source -Output (Join-Path $OwnedRoot 'unverified') -SourceIdentity ''
    Assert-True ($unverified.ExitCode -ne 0) 'builder labeled arbitrary synthetic input with the default pinned source identity'

    $wrongIdentity = Join-Path $TempRoot 'wrong-source-identity.json'
    Write-TestSourceIdentity -Root $Source -Path $wrongIdentity
    $wrongIdentityDocument = Get-Content -Raw -LiteralPath $wrongIdentity | ConvertFrom-Json
    $wrongIdentityDocument.files[0].sha256 = '0' * 64
    [IO.File]::WriteAllText($wrongIdentity, ($wrongIdentityDocument | ConvertTo-Json -Depth 6), [Text.UTF8Encoding]::new($false))
    $identityMismatch = Invoke-Builder -Source $Source -Output (Join-Path $OwnedRoot 'identity-mismatch') -SourceIdentity $wrongIdentity
    Assert-True ($identityMismatch.ExitCode -ne 0) 'builder accepted source content that did not match the claimed identity hashes'

    $falsePinnedIdentity = Join-Path $TempRoot 'false-pinned-source-identity.json'
    Write-TestSourceIdentity `
        -Root $Source `
        -Path $falsePinnedIdentity `
        -Tag 'v1.26.30.32-preview' `
        -ArchiveSha256 '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
    $falsePinned = Invoke-Builder -Source $Source -Output (Join-Path $OwnedRoot 'false-pinned') -SourceIdentity $falsePinnedIdentity
    Assert-True ($falsePinned.ExitCode -ne 0) 'custom identity relabeled synthetic content with the reserved pinned Mojang identity'

    $overlapSource = Join-Path $OwnedRoot 'overlap-source'
    $overlapIdentity = Join-Path $TempRoot 'overlap-source-identity.json'
    New-TestPack -Root $overlapSource
    Write-TestSourceIdentity -Root $overlapSource -Path $overlapIdentity
    $overlap = Invoke-Builder -Source $overlapSource -Output (Join-Path $overlapSource 'output') -SourceIdentity $overlapIdentity
    Assert-True ($overlap.ExitCode -ne 0) 'builder accepted overlapping source/output roots'

    $escapeTarget = Join-Path $TempRoot 'junction escape target'
    $escapeLink = Join-Path $OwnedRoot 'escape-link'
    New-Item -ItemType Directory -Path $escapeTarget -Force | Out-Null
    $null = New-Item -ItemType Junction -Path $escapeLink -Target $escapeTarget
    $escape = Invoke-Builder -Source $Source -Output (Join-Path $escapeLink 'pack') -SourceIdentity $SourceIdentity
    Assert-True ($escape.ExitCode -ne 0) 'builder accepted a reparse-point escape from .local'

    $missingSource = Join-Path $TempRoot 'missing image source'
    $missingIdentity = Join-Path $TempRoot 'missing-source-identity.json'
    New-TestPack -Root $missingSource
    Write-TestSourceIdentity -Root $missingSource -Path $missingIdentity
    Remove-Item -LiteralPath (Join-Path $missingSource 'textures\blocks\wildflowers.png')
    Assert-True ((Invoke-Builder -Source $missingSource -Output (Join-Path $OwnedRoot 'missing') -SourceIdentity $missingIdentity).ExitCode -ne 0) 'builder accepted a missing image'

    $malformedSource = Join-Path $TempRoot 'malformed image source'
    $malformedIdentity = Join-Path $TempRoot 'malformed-source-identity.json'
    New-TestPack -Root $malformedSource
    [IO.File]::WriteAllText((Join-Path $malformedSource 'textures\blocks\pink_petals.png'), 'not a png')
    Write-TestSourceIdentity -Root $malformedSource -Path $malformedIdentity
    Assert-True ((Invoke-Builder -Source $malformedSource -Output (Join-Path $OwnedRoot 'malformed') -SourceIdentity $malformedIdentity).ExitCode -ne 0) 'builder accepted a malformed image'

    $wrongSizeSource = Join-Path $TempRoot 'wrong size source'
    $wrongSizeIdentity = Join-Path $TempRoot 'wrong-size-source-identity.json'
    New-TestPack -Root $wrongSizeSource
    New-TestPng -Path (Join-Path $wrongSizeSource 'textures\blocks\pink_petals_stem.png') -Width 15 -Height 16
    Write-TestSourceIdentity -Root $wrongSizeSource -Path $wrongSizeIdentity
    Assert-True ((Invoke-Builder -Source $wrongSizeSource -Output (Join-Path $OwnedRoot 'wrong-size') -SourceIdentity $wrongSizeIdentity).ExitCode -ne 0) 'builder accepted a wrong-size image'

    $concurrentScript = {
        param($BuilderPath, $SourcePath, $OutputPath, $IdentityPath)
        $lines = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $BuilderPath -InputRoot $SourcePath -OutputRoot $OutputPath -SourceIdentityPath $IdentityPath 2>&1
        [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = @($lines | ForEach-Object ToString) }
    }
    $concurrentJobs = @(
        Start-Job -ScriptBlock $concurrentScript -ArgumentList $Builder, $Source, (Join-Path $OwnedRoot 'concurrent-a'), $SourceIdentity
        Start-Job -ScriptBlock $concurrentScript -ArgumentList $Builder, $Source, (Join-Path $OwnedRoot 'concurrent-b'), $SourceIdentity
    )
    try {
        $concurrentResults = @($concurrentJobs | Receive-Job -Wait)
        Assert-True ($concurrentResults.Count -eq 2) 'concurrent builder test lost a process result'
        foreach ($result in $concurrentResults) {
            Assert-True ([int]$result.ExitCode -eq 0) "concurrent builder invocation failed: $(@($result.Output) -join [Environment]::NewLine)"
        }
        Assert-True ((Get-TreeIdentity -Root (Join-Path $OwnedRoot 'concurrent-a')) -ceq (Get-TreeIdentity -Root (Join-Path $OwnedRoot 'concurrent-b'))) 'concurrent isolated builds were not byte-identical'
    }
    finally { $concurrentJobs | Remove-Job -Force -ErrorAction SilentlyContinue }

    $rerunOwnedRoot = Join-Path $LocalTestParent ("run-{0}" -f [guid]::NewGuid().ToString('N'))
    try {
        New-Item -ItemType Directory -Path $rerunOwnedRoot | Out-Null
        $rerun = Invoke-Builder -Source $Source -Output (Join-Path $rerunOwnedRoot 'pack') -SourceIdentity $SourceIdentity
        Assert-True ($rerun.ExitCode -eq 0) "independent rerun failed: $($rerun.Output -join [Environment]::NewLine)"
    }
    finally { Remove-Item -LiteralPath $rerunOwnedRoot -Recurse -Force -ErrorAction SilentlyContinue }
    Assert-True (Test-Path -LiteralPath $SiblingMarker -PathType Leaf) 'independent rerun cleanup removed a pre-existing sibling'

    Remove-Item -LiteralPath $OwnedRoot -Recurse -Force
    $ownedRootRemoved = $true
    Assert-True (Test-Path -LiteralPath $SiblingMarker -PathType Leaf) 'per-run owned cleanup removed a pre-existing sibling'

    Write-Output 'flowerbed reference pack tests passed'
}
finally {
    Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction SilentlyContinue
    if (-not $ownedRootRemoved) {
        Remove-Item -LiteralPath $OwnedRoot -Recurse -Force -ErrorAction SilentlyContinue
    }
    Remove-Item -LiteralPath $PreExistingSibling -Recurse -Force -ErrorAction SilentlyContinue
}
