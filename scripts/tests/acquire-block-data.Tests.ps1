Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$manifestPath = Join-Path $repoRoot "assets\block-data-sources.json"
$acquirerPath = Join-Path $repoRoot "scripts\acquire-block-data.ps1"
$noticesPath = Join-Path $repoRoot "THIRD_PARTY_NOTICES.md"

function Assert-True {
    param(
        [Parameter(Mandatory = $true)]
        [bool]$Condition,
        [Parameter(Mandatory = $true)]
        [string]$Message
    )
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param(
        [AllowNull()]
        [object]$Expected,
        [AllowNull()]
        [object]$Actual,
        [Parameter(Mandatory = $true)]
        [string]$Message
    )
    if ($Expected -cne $Actual) {
        throw "$Message`nexpected: $Expected`nactual:   $Actual"
    }
}

function Invoke-NativeCapture {
    param(
        [Parameter(Mandatory = $true)]
        [string]$FilePath,
        [string[]]$ArgumentList = @()
    )

    $savedErrorActionPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = "Continue"
        $output = & $FilePath @ArgumentList 2>&1 | Out-String
        $exitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $savedErrorActionPreference
    }
    [pscustomobject]@{
        ExitCode = $exitCode
        Output = $output
    }
}

function Get-FileSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Get-Utf8Sha256 {
    param([Parameter(Mandatory = $true)][string]$Text)
    $bytes = [System.Text.UTF8Encoding]::new($false).GetBytes($Text)
    $algorithm = [System.Security.Cryptography.SHA256]::Create()
    try {
        $hash = $algorithm.ComputeHash($bytes)
    } finally {
        $algorithm.Dispose()
    }
    return (($hash | ForEach-Object { $_.ToString("x2") }) -join "")
}

function Test-StringContains {
    param(
        [Parameter(Mandatory = $true)][string]$Value,
        [Parameter(Mandatory = $true)][string]$Needle,
        [System.StringComparison]$Comparison = [System.StringComparison]::Ordinal
    )
    return $Value.IndexOf($Needle, $Comparison) -ge 0
}

function Get-NoticeSection {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Notice,
        [Parameter(Mandatory = $true)]
        [string]$Identifier
    )

    $normalized = $Notice.Replace("`r`n", "`n")
    $begin = "<!-- BEGIN $Identifier -->`n"
    $end = "<!-- END $Identifier -->"
    $start = $normalized.IndexOf($begin, [System.StringComparison]::Ordinal)
    Assert-True ($start -ge 0) "notice is missing begin marker '$begin'"
    $start += $begin.Length
    $finish = $normalized.IndexOf($end, $start, [System.StringComparison]::Ordinal)
    Assert-True ($finish -ge $start) "notice is missing end marker '$end'"
    return $normalized.Substring($start, $finish - $start)
}

function ConvertTo-SourceMap {
    param([Parameter(Mandatory = $true)][object[]]$Sources)
    $result = @{}
    foreach ($source in $Sources) {
        $id = [string]$source.id
        Assert-True (-not [string]::IsNullOrWhiteSpace($id)) "manifest contains a source without an id"
        Assert-True (-not $result.ContainsKey($id)) "manifest contains duplicate source id '$id'"
        $result[$id] = $source
    }
    return $result
}

function ConvertTo-FileMap {
    param([Parameter(Mandatory = $true)][object[]]$Files)
    $result = @{}
    foreach ($file in $Files) {
        $path = [string]$file.install_path
        Assert-True (-not [string]::IsNullOrWhiteSpace($path)) "manifest contains a file without install_path"
        Assert-True (-not $result.ContainsKey($path)) "manifest contains duplicate install_path '$path'"
        $result[$path] = $file
    }
    return $result
}

function Write-JsonFile {
    param(
        [Parameter(Mandatory = $true)][object]$Value,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $Value | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $Path -Encoding UTF8
}

function Copy-ObjectThroughJson {
    param([Parameter(Mandatory = $true)][object]$Value)
    return ($Value | ConvertTo-Json -Depth 20 | ConvertFrom-Json)
}

function Get-TemporaryAcquisitionArtifacts {
    param([Parameter(Mandatory = $true)][string]$Root)
    if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
        return @()
    }
    return @(Get-ChildItem -LiteralPath $Root -Force -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match "\.partial-|\.installing-" })
}

function Remove-TestJunction {
    param([Parameter(Mandatory = $true)][string]$Path)
    if (Test-Path -LiteralPath $Path) {
        [System.IO.Directory]::Delete($Path, $false)
    }
}

function New-SyntheticSourceManifest {
    param(
        [Parameter(Mandatory = $true)][psobject]$Template,
        [Parameter(Mandatory = $true)][string]$FixtureRoot,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $copy = Copy-ObjectThroughJson -Value $Template
    foreach ($source in @($copy.sources)) {
        $sourceRoot = Join-Path $FixtureRoot ([string]$source.id)
        New-Item -ItemType Directory -Force -Path $sourceRoot | Out-Null
        foreach ($file in @($source.files)) {
            $fixturePath = Join-Path $sourceRoot ([string]$file.install_path)
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $fixturePath) | Out-Null
            if ([string]$source.id -eq "pmmp-bedrock-data" -and
                [string]$file.install_path -eq "protocol_info.json") {
                $fixture = [ordered]@{
                    version = [ordered]@{
                        major = 1
                        minor = 26
                        patch = 30
                        revision = 31
                        beta = $false
                        protocol_version = 1001
                    }
                }
                Write-JsonFile -Value $fixture -Path $fixturePath
            } else {
                [System.IO.File]::WriteAllText(
                    $fixturePath,
                    "fixture:$($source.id):$($file.install_path)`n",
                    [System.Text.UTF8Encoding]::new($false)
                )
            }
            $file.url = ([System.Uri]::new($fixturePath)).AbsoluteUri
            $file.sha256 = Get-FileSha256 -Path $fixturePath
            $file.size_bytes = (Get-Item -LiteralPath $fixturePath).Length
        }
    }
    Write-JsonFile -Value $copy -Path $Path
    return $copy
}

function Set-SyntheticProtocolInfo {
    param(
        [Parameter(Mandatory = $true)][psobject]$Manifest,
        [Parameter(Mandatory = $true)][string]$ManifestPath,
        [Parameter(Mandatory = $true)][string]$FixtureRoot,
        [Parameter(Mandatory = $true)][int]$Patch,
        [Parameter(Mandatory = $true)][int]$Protocol
    )

    $source = @($Manifest.sources | Where-Object { $_.id -eq "pmmp-bedrock-data" })[0]
    $file = @($source.files | Where-Object { $_.install_path -eq "protocol_info.json" })[0]
    $fixturePath = Join-Path (Join-Path $FixtureRoot "pmmp-bedrock-data") "protocol_info.json"
    $fixture = [ordered]@{
        version = [ordered]@{
            major = 1
            minor = 26
            patch = $Patch
            revision = 31
            beta = $false
            protocol_version = $Protocol
        }
    }
    Write-JsonFile -Value $fixture -Path $fixturePath
    $file.sha256 = Get-FileSha256 -Path $fixturePath
    $file.size_bytes = (Get-Item -LiteralPath $fixturePath).Length
    Write-JsonFile -Value $Manifest -Path $ManifestPath
}

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "source manifest missing: $manifestPath"
}
if (-not (Test-Path -LiteralPath $acquirerPath -PathType Leaf)) {
    throw "acquisition script missing: $acquirerPath"
}
if (-not (Test-Path -LiteralPath $noticesPath -PathType Leaf)) {
    throw "third-party notice missing: $noticesPath"
}

$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
Assert-Equal 1 ([int]$manifest.schema) "source manifest schema changed"
Assert-Equal "1.26.30" ([string]$manifest.protocol.game_version) "source game version changed"
Assert-Equal 1001 ([int]$manifest.protocol.protocol_version) "source protocol changed"
Assert-Equal "local-only" ([string]$manifest.artifact_policy) "source artifact policy changed"
Assert-Equal 16 ([int]$manifest.limits.max_sources) "source-count ceiling changed"
Assert-Equal 64 ([int]$manifest.limits.max_files_per_source) "per-source file ceiling changed"
Assert-Equal 8388608 ([long]$manifest.limits.max_file_bytes) "per-file byte ceiling changed"
Assert-Equal 12582912 ([long]$manifest.limits.max_total_bytes) "bundle byte ceiling changed"
Assert-Equal 65536 ([int]$manifest.limits.download_buffer_bytes) "download buffer bound changed"
Assert-Equal 30 ([int]$manifest.limits.request_timeout_seconds) "request timeout changed"

$sources = ConvertTo-SourceMap -Sources @($manifest.sources)
foreach ($id in @(
    "pmmp-bedrock-data",
    "prismarinejs-minecraft-data",
    "axolotl-stack",
    "dragonfly"
)) {
    Assert-True $sources.ContainsKey($id) "manifest is missing source '$id'"
}

$pmmp = $sources["pmmp-bedrock-data"]
Assert-Equal "https://github.com/pmmp/BedrockData" ([string]$pmmp.repository) "PMMP repository changed"
Assert-Equal "6.7.0+bedrock-1.26.30" ([string]$pmmp.tag) "PMMP tag changed"
Assert-Equal "bdb44a48fb6beffb6e9f6864f06d2232eb62b6a3" ([string]$pmmp.commit) "PMMP commit changed"
Assert-Equal "CC0-1.0" ([string]$pmmp.license.spdx) "PMMP license changed"
Assert-Equal "license-file" ([string]$pmmp.license.evidence_kind) "PMMP license evidence changed"
Assert-True ([bool]$pmmp.license.standalone_file) "PMMP standalone license marker changed"
Assert-Equal "LICENSE" ([string]$pmmp.license.evidence_path) "PMMP license path changed"
Assert-Equal "a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499" ([string]$pmmp.license.evidence_sha256) "PMMP license hash changed"
$pmmpFiles = ConvertTo-FileMap -Files @($pmmp.files)
$expectedPmmpFiles = [ordered]@{
    "protocol_info.json" = [pscustomobject]@{ Sha256 = "901742e774763282d1595d292fb2580bb714c55258fa5e5c0b95a562afe2c238"; Size = 9118L }
    "canonical_block_states.nbt" = [pscustomobject]@{ Sha256 = "d10d06a8af1fa062350c6919bc52d8980003ab745bb94920a6b0d9caad49d040"; Size = 2365370L }
    "block_state_meta_map.json" = [pscustomobject]@{ Sha256 = "2987cbdf1a2ed2291e5130c4817abcdd3c2e1eed483a94370432bba4b13ad101"; Size = 137947L }
    "block_properties_table.json" = [pscustomobject]@{ Sha256 = "c9eb2a1b7751ba874ddeb04237d2a0013121a1bf03e1d5c75a78a08bae020abd"; Size = 351816L }
    "biome_definitions.json" = [pscustomobject]@{ Sha256 = "1e0e9e0ae95992fb90269a48590eb9b16c262512960e985d793bcd63de511aa2"; Size = 48614L }
    "LICENSE" = [pscustomobject]@{ Sha256 = "a2010f343487d3f7618affe54f789f5487602331c0a8d03f49e9a7c547cf0499"; Size = 7048L }
}
Assert-Equal $expectedPmmpFiles.Count $pmmpFiles.Count "PMMP file inventory changed"
foreach ($entry in $expectedPmmpFiles.GetEnumerator()) {
    Assert-True $pmmpFiles.ContainsKey($entry.Key) "PMMP file '$($entry.Key)' is missing"
    Assert-Equal $entry.Value.Sha256 ([string]$pmmpFiles[$entry.Key].sha256) "PMMP file hash changed for '$($entry.Key)'"
    Assert-Equal $entry.Value.Size ([long]$pmmpFiles[$entry.Key].size_bytes) "PMMP file size changed for '$($entry.Key)'"
}

$prismarine = $sources["prismarinejs-minecraft-data"]
Assert-Equal "https://github.com/PrismarineJS/minecraft-data" ([string]$prismarine.repository) "Prismarine repository changed"
Assert-Equal "6ec59288287e4045331eaa47ee8fb104278f6b98" ([string]$prismarine.commit) "Prismarine commit changed"
Assert-Equal "MIT" ([string]$prismarine.license.spdx) "Prismarine license changed"
Assert-Equal "readme-license-declaration" ([string]$prismarine.license.evidence_kind) "Prismarine license evidence exception changed"
Assert-True (-not [bool]$prismarine.license.standalone_file) "Prismarine unexpectedly claims a standalone license file"
Assert-Equal "README.md" ([string]$prismarine.license.evidence_path) "Prismarine license evidence path changed"
Assert-Equal "c0b6a32a38ac3070c908eba107b25e14385a4d8534ada08780956748da589561" ([string]$prismarine.license.evidence_sha256) "Prismarine README license evidence hash changed"
$prismarineFiles = ConvertTo-FileMap -Files @($prismarine.files)
$expectedPrismarineFiles = [ordered]@{
    "blockStates.json" = [pscustomobject]@{ Sha256 = "c0a94f5a32597aff028918e152c76280c1823a7840fdf73cd98d7b44814ea041"; Size = 5501763L }
    "blocks.json" = [pscustomobject]@{ Sha256 = "12ff90b5094006b42d87ca7c296ed1bef0e1c2d6d67498aea85b6ece9408b494"; Size = 588070L }
    "blockCollisionShapes.json" = [pscustomobject]@{ Sha256 = "72a7410456a1f5f556e8c91c07e1d1f61aea5d2fb555f2c0e33eba825247aa90"; Size = 118344L }
    "README.md" = [pscustomobject]@{ Sha256 = "c0b6a32a38ac3070c908eba107b25e14385a4d8534ada08780956748da589561"; Size = 9893L }
}
Assert-Equal $expectedPrismarineFiles.Count $prismarineFiles.Count "Prismarine file inventory changed"
foreach ($entry in $expectedPrismarineFiles.GetEnumerator()) {
    Assert-True $prismarineFiles.ContainsKey($entry.Key) "Prismarine file '$($entry.Key)' is missing"
    Assert-Equal $entry.Value.Sha256 ([string]$prismarineFiles[$entry.Key].sha256) "Prismarine file hash changed for '$($entry.Key)'"
    Assert-Equal $entry.Value.Size ([long]$prismarineFiles[$entry.Key].size_bytes) "Prismarine file size changed for '$($entry.Key)'"
}

$axolotl = $sources["axolotl-stack"]
Assert-Equal "https://github.com/axolotl-stack/axolotl-stack" ([string]$axolotl.repository) "Axolotl repository changed"
Assert-Equal "6f6806e821a579c183c44d786f76d9b358a2b825" ([string]$axolotl.commit) "Axolotl commit changed"
Assert-Equal "MIT" ([string]$axolotl.license.spdx) "Axolotl license changed"
Assert-Equal "62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184" ([string]$axolotl.license.evidence_sha256) "Axolotl license hash changed"
Assert-Equal "Copyright (c) 2025 Jeremy Ianne" ([string]$axolotl.license.copyright) "Axolotl copyright changed"
Assert-Equal 1068L ([long]$axolotl.files[0].size_bytes) "Axolotl license size changed"

$dragonfly = $sources["dragonfly"]
Assert-Equal "https://github.com/df-mc/dragonfly" ([string]$dragonfly.repository) "Dragonfly repository changed"
Assert-Equal "b85c56ffea6b306798a935f14cc941c76618be52" ([string]$dragonfly.commit) "Dragonfly commit changed"
Assert-Equal "MIT" ([string]$dragonfly.license.spdx) "Dragonfly license changed"
Assert-Equal "9b0866098f4b7bfadafa43adec71dae35968053ceaea0487fb4b23c46cc72755" ([string]$dragonfly.license.evidence_sha256) "Dragonfly license hash changed"
Assert-Equal "Copyright (c) 2019 Dragonfly Tech" ([string]$dragonfly.license.copyright) "Dragonfly copyright changed"
Assert-Equal 1071L ([long]$dragonfly.files[0].size_bytes) "Dragonfly license size changed"

$acquirerSource = Get-Content -Raw -LiteralPath $acquirerPath
Assert-True (-not (Test-StringContains -Value $acquirerSource -Needle "Invoke-WebRequest")) "acquirer still uses unbounded Invoke-WebRequest"
foreach ($streamingMarker in @(
    "ContentLength",
    "GetResponseStream",
    "ReadWriteTimeout",
    "byte ceiling",
    "System.Diagnostics.Stopwatch",
    "ElapsedMilliseconds",
    "ReadTimeout",
    "overall download deadline exceeded"
)) {
    Assert-True (Test-StringContains -Value $acquirerSource -Needle $streamingMarker) "bounded download implementation is missing '$streamingMarker'"
}
Assert-True (Test-StringContains -Value $acquirerSource -Needle "[System.IO.Directory]::Move") "bundle publication does not use destination-must-not-exist directory rename semantics"
Assert-True (-not (Test-StringContains -Value $acquirerSource -Needle "Move-Item -LiteralPath `$temporaryBundle")) "bundle publication still uses nesting-prone Move-Item"
Assert-True (-not (Test-StringContains -Value $acquirerSource -Needle "destination root appeared before atomic publication")) "bundle publication still has a check-then-rename race"
Assert-True (Test-StringContains -Value $acquirerSource -Needle "concurrent publisher") "bundle publication does not handle a concurrent winner explicitly"

$notice = Get-Content -Raw -LiteralPath $noticesPath
foreach ($marker in @(
    [string]$pmmp.repository,
    [string]$pmmp.commit,
    [string]$pmmp.tag,
    [string]$prismarine.repository,
    [string]$prismarine.commit,
    [string]$axolotl.repository,
    [string]$axolotl.commit,
    [string]$dragonfly.repository,
    [string]$dragonfly.commit,
    "CC0-1.0",
    "PrismarineJS/minecraft-data contributors",
    [string]$axolotl.license.copyright,
    [string]$dragonfly.license.copyright
)) {
    Assert-True (Test-StringContains -Value $notice -Needle $marker) "third-party notice is missing '$marker'"
}

$pmmpNotice = Get-NoticeSection -Notice $notice -Identifier "PMMP-BEDROCKDATA-CC0"
$axolotlNotice = Get-NoticeSection -Notice $notice -Identifier "AXOLOTL-STACK-MIT"
$dragonflyNotice = Get-NoticeSection -Notice $notice -Identifier "DRAGONFLY-MIT"
Assert-Equal ([string]$pmmp.license.evidence_sha256) (Get-Utf8Sha256 -Text $pmmpNotice) "notice does not contain the exact full PMMP CC0 text"
Assert-Equal ([string]$axolotl.license.evidence_sha256) (Get-Utf8Sha256 -Text $axolotlNotice) "notice does not contain the exact full Axolotl MIT text"
Assert-Equal ([string]$dragonfly.license.evidence_sha256) (Get-Utf8Sha256 -Text $dragonflyNotice) "notice does not contain the exact full Dragonfly MIT text"
$prismarineNotice = Get-NoticeSection -Notice $notice -Identifier "PRISMARINEJS-MINECRAFT-DATA-MIT"
foreach ($clause in @(
    "Copyright PrismarineJS/minecraft-data contributors",
    "Permission is hereby granted, free of charge, to any person obtaining a copy",
    "The above copyright notice and this permission notice shall be included in all",
    'THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR',
    "OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE"
)) {
    Assert-True (Test-StringContains -Value $prismarineNotice -Needle $clause) "Prismarine MIT notice is missing '$clause'"
}
Assert-True (Test-StringContains -Value $notice -Needle "does not contain a standalone LICENSE, LICENCE, COPYING, or NOTICE file") "Prismarine license-file exception is not documented"
Assert-True (Test-StringContains -Value $notice -Needle ([string]$prismarine.license.evidence_sha256)) "Prismarine README evidence hash is not documented"
Assert-True ("payload.partial-1234-abcdef" -match "\.partial-") "temporary-file matcher does not recognize actual partial names"
Assert-True ("block-data.installing-1234-abcdef" -match "\.installing-") "temporary-file matcher does not recognize actual installing names"

$ignoredProbe = ".local/assets/block-data/contract-probe"
& git -C $repoRoot check-ignore --quiet -- $ignoredProbe
Assert-Equal 0 $LASTEXITCODE "block-data destination is not ignored"
$trackedBlockData = @(& git -C $repoRoot ls-files -- ".local/assets/block-data/*")
Assert-Equal 0 $LASTEXITCODE "git ls-files failed for block-data destination"
Assert-Equal 0 @($trackedBlockData | Where-Object { $_ -match "\S" }).Count "ignored block-data payload is tracked"

$childPowerShell = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
$sandboxRoot = Join-Path ([System.IO.Path]::GetTempPath()) ("rust-mcbe-block-data-test-" + [guid]::NewGuid().ToString("N"))
$sandboxParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$sandboxRoot = [System.IO.Path]::GetFullPath($sandboxRoot)
$sandboxPrefix = $sandboxParent.TrimEnd([System.IO.Path]::DirectorySeparatorChar) + [System.IO.Path]::DirectorySeparatorChar
Assert-True $sandboxRoot.StartsWith($sandboxPrefix, [System.StringComparison]::OrdinalIgnoreCase) "refusing unsafe test sandbox '$sandboxRoot'"

try {
    New-Item -ItemType Directory -Path $sandboxRoot | Out-Null
    $functionStart = $acquirerSource.IndexOf("function Get-LowerSha256", [System.StringComparison]::Ordinal)
    $functionEnd = $acquirerSource.IndexOf("`$repoRoot =", $functionStart, [System.StringComparison]::Ordinal)
    Assert-True ($functionStart -ge 0 -and $functionEnd -gt $functionStart) "could not isolate acquisition functions for deadline unit test"
    . ([scriptblock]::Create($acquirerSource.Substring($functionStart, $functionEnd - $functionStart)))
    Add-Type -TypeDefinition @"
using System;
using System.IO;
using System.Threading;

public sealed class SlowReadMemoryStream : MemoryStream {
    private readonly int delayMilliseconds;

    public SlowReadMemoryStream(byte[] bytes, int delayMilliseconds) : base(bytes) {
        this.delayMilliseconds = delayMilliseconds;
    }

    public override int Read(byte[] buffer, int offset, int count) {
        Thread.Sleep(delayMilliseconds);
        return base.Read(buffer, offset, count);
    }
}
"@
    $deadlineOutput = Join-Path $sandboxRoot "deadline-output.bin"
    $slowStream = [SlowReadMemoryStream]::new([byte[]]@(42), 100)
    $deadlineStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $deadlineError = $null
    try {
        Copy-StreamBounded -InputStream $slowStream -PartialPath $deadlineOutput `
            -ExpectedBytes 1 -MaximumBytes 1 -BufferBytes 4096 `
            -Deadline $deadlineStopwatch -TimeoutMilliseconds 10 -Label "deadline fixture"
    } catch {
        $deadlineError = $_.Exception.Message
    } finally {
        $slowStream.Dispose()
        if (Test-Path -LiteralPath $deadlineOutput) {
            Remove-Item -LiteralPath $deadlineOutput -Force
        }
    }
    Assert-True ($null -ne $deadlineError) "slow response body did not exceed the overall deadline"
    Assert-True (Test-StringContains -Value $deadlineError -Needle "overall download deadline exceeded" -Comparison OrdinalIgnoreCase) "slow-body deadline failure omitted its diagnostic: $deadlineError"

    $fixtureRoot = Join-Path $sandboxRoot "upstream fixtures"
    $syntheticManifestPath = Join-Path $sandboxRoot "synthetic-sources.json"
    New-Item -ItemType Directory -Path $fixtureRoot | Out-Null
    $synthetic = New-SyntheticSourceManifest -Template $manifest -FixtureRoot $fixtureRoot -Path $syntheticManifestPath

    $validDestination = Join-Path $sandboxRoot "valid destination"
    $noticeHashBefore = Get-FileSha256 -Path $noticesPath
    $validArguments = @(
        "-NoProfile",
        "-File", $acquirerPath,
        "-ManifestPath", $syntheticManifestPath,
        "-DestinationRoot", $validDestination
    )
    $first = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList $validArguments
    Assert-Equal 0 $first.ExitCode "valid acquisition failed: $($first.Output.Trim())"
    foreach ($expectedPath in @(
        (Join-Path $validDestination "pmmp"),
        (Join-Path $validDestination "prismarine"),
        (Join-Path $validDestination "licenses\axolotl-stack"),
        (Join-Path $validDestination "licenses\dragonfly")
    )) {
        Assert-True (Test-Path -LiteralPath $expectedPath -PathType Container) "acquisition did not publish '$expectedPath'"
        Assert-True (Test-StringContains -Value $first.Output -Needle ([System.IO.Path]::GetFullPath($expectedPath)) -Comparison OrdinalIgnoreCase) "acquisition did not print resolved path '$expectedPath'"
    }
    $validCacheRoot = "$validDestination.downloads"
    Assert-True (Test-Path -LiteralPath $validCacheRoot -PathType Container) "acquisition did not create a sibling download cache"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $validDestination ".downloads"))) "download cache was published inside the final bundle"
    Assert-Equal $noticeHashBefore (Get-FileSha256 -Path $noticesPath) "acquisition silently rewrote THIRD_PARTY_NOTICES.md"
    $publishedFiles = @(Get-ChildItem -LiteralPath $validDestination -File -Recurse)
    Assert-True ($publishedFiles.Count -gt 0) "valid acquisition published no files"
    $before = @{}
    foreach ($file in $publishedFiles) {
        $before[$file.FullName] = "$($file.LastWriteTimeUtc.Ticks):$(Get-FileSha256 -Path $file.FullName)"
    }

    $second = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList $validArguments
    Assert-Equal 0 $second.ExitCode "idempotent acquisition failed: $($second.Output.Trim())"
    Assert-True (Test-StringContains -Value $second.Output -Needle "already verified" -Comparison OrdinalIgnoreCase) "idempotent acquisition did not report verified destinations"
    foreach ($file in $publishedFiles) {
        $current = Get-Item -LiteralPath $file.FullName
        Assert-Equal $before[$file.FullName] "$($current.LastWriteTimeUtc.Ticks):$(Get-FileSha256 -Path $current.FullName)" "idempotent acquisition rewrote '$($file.FullName)'"
    }
    Assert-Equal $noticeHashBefore (Get-FileSha256 -Path $noticesPath) "idempotent acquisition silently rewrote THIRD_PARTY_NOTICES.md"
    $temporaryArtifacts = @(Get-TemporaryAcquisitionArtifacts -Root $sandboxRoot)
    Assert-Equal 0 $temporaryArtifacts.Count "acquisition left temporary artifacts: $(@($temporaryArtifacts | ForEach-Object { $_.FullName }) -join ', ')"

    $tamperedPublished = Join-Path $validDestination "pmmp\protocol_info.json"
    $tamperedBytes = [System.IO.File]::ReadAllBytes($tamperedPublished)
    $tamperedBytes[0] = $tamperedBytes[0] -bxor 1
    [System.IO.File]::WriteAllBytes($tamperedPublished, $tamperedBytes)
    $tampered = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList $validArguments
    Assert-True ($tampered.ExitCode -ne 0) "acquisition accepted a tampered installed file"
    Assert-True (Test-StringContains -Value $tampered.Output -Needle "installed SHA-256 mismatch" -Comparison OrdinalIgnoreCase) "tampered install failure omitted the expected diagnostic: $($tampered.Output.Trim())"

    $badBytesManifestPath = Join-Path $sandboxRoot "bad-bytes-sources.json"
    $badBytes = New-SyntheticSourceManifest -Template $manifest -FixtureRoot (Join-Path $sandboxRoot "bad bytes fixtures") -Path $badBytesManifestPath
    $badPmmp = @($badBytes.sources | Where-Object { $_.id -eq "pmmp-bedrock-data" })[0]
    $badFile = @($badPmmp.files | Where-Object { $_.install_path -eq "biome_definitions.json" })[0]
    $badFilePath = ([System.Uri]::new([string]$badFile.url)).LocalPath
    $badFileBytes = [System.IO.File]::ReadAllBytes($badFilePath)
    $badFileBytes[0] = $badFileBytes[0] -bxor 1
    [System.IO.File]::WriteAllBytes($badFilePath, $badFileBytes)
    $badBytesDestination = Join-Path $sandboxRoot "bad bytes destination"
    $badBytesResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile", "-File", $acquirerPath,
        "-ManifestPath", $badBytesManifestPath,
        "-DestinationRoot", $badBytesDestination
    )
    Assert-True ($badBytesResult.ExitCode -ne 0) "acquisition accepted source bytes with the wrong SHA-256"
    Assert-True (Test-StringContains -Value $badBytesResult.Output -Needle "SHA-256 mismatch" -Comparison OrdinalIgnoreCase) "bad source failure omitted the expected diagnostic: $($badBytesResult.Output.Trim())"
    Assert-True (-not (Test-Path -LiteralPath $badBytesDestination)) "bad source bytes published a final bundle"
    $badBytesTemporary = @(Get-TemporaryAcquisitionArtifacts -Root $sandboxRoot)
    Assert-Equal 0 $badBytesTemporary.Count "bad source failure left temporary artifacts"

    $lateFailureRoot = Join-Path $sandboxRoot "late source failure"
    $lateFixtureRoot = Join-Path $lateFailureRoot "fixtures"
    $lateManifestPath = Join-Path $lateFailureRoot "sources.json"
    New-Item -ItemType Directory -Force -Path $lateFixtureRoot | Out-Null
    $lateManifest = New-SyntheticSourceManifest -Template $manifest -FixtureRoot $lateFixtureRoot -Path $lateManifestPath
    $latePrismarine = @($lateManifest.sources | Where-Object { $_.id -eq "prismarinejs-minecraft-data" })[0]
    $lateFile = @($latePrismarine.files | Where-Object { $_.install_path -eq "README.md" })[0]
    $lateFilePath = ([System.Uri]::new([string]$lateFile.url)).LocalPath
    $lateBytes = [System.IO.File]::ReadAllBytes($lateFilePath)
    $lateBytes[0] = $lateBytes[0] -bxor 1
    [System.IO.File]::WriteAllBytes($lateFilePath, $lateBytes)
    $lateDestination = Join-Path $lateFailureRoot "destination"
    $lateResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile", "-File", $acquirerPath,
        "-ManifestPath", $lateManifestPath,
        "-DestinationRoot", $lateDestination
    )
    Assert-True ($lateResult.ExitCode -ne 0) "acquisition accepted a bad later Prismarine source"
    Assert-True (Test-StringContains -Value $lateResult.Output -Needle "SHA-256 mismatch" -Comparison OrdinalIgnoreCase) "later-source failure omitted its hash diagnostic: $($lateResult.Output.Trim())"
    Assert-True (-not (Test-Path -LiteralPath $lateDestination)) "later-source failure published a partial final bundle"
    Assert-Equal 0 @(Get-TemporaryAcquisitionArtifacts -Root $lateFailureRoot).Count "later-source failure left temporary artifacts"

    $oversizedRoot = Join-Path $sandboxRoot "oversized source"
    $oversizedFixtures = Join-Path $oversizedRoot "fixtures"
    $oversizedManifestPath = Join-Path $oversizedRoot "sources.json"
    New-Item -ItemType Directory -Force -Path $oversizedFixtures | Out-Null
    $oversizedManifest = New-SyntheticSourceManifest -Template $manifest -FixtureRoot $oversizedFixtures -Path $oversizedManifestPath
    $oversizedPmmp = @($oversizedManifest.sources | Where-Object { $_.id -eq "pmmp-bedrock-data" })[0]
    $oversizedFile = @($oversizedPmmp.files | Where-Object { $_.install_path -eq "protocol_info.json" })[0]
    $oversizedFilePath = ([System.Uri]::new([string]$oversizedFile.url)).LocalPath
    [System.IO.File]::AppendAllText($oversizedFilePath, "x", [System.Text.UTF8Encoding]::new($false))
    $oversizedDestination = Join-Path $oversizedRoot "destination"
    $oversizedResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile", "-File", $acquirerPath,
        "-ManifestPath", $oversizedManifestPath,
        "-DestinationRoot", $oversizedDestination
    )
    Assert-True ($oversizedResult.ExitCode -ne 0) "acquisition accepted a source larger than size_bytes"
    Assert-True (Test-StringContains -Value $oversizedResult.Output -Needle "byte ceiling" -Comparison OrdinalIgnoreCase) "oversized-source failure omitted its byte-ceiling diagnostic: $($oversizedResult.Output.Trim())"
    Assert-True (-not (Test-Path -LiteralPath $oversizedDestination)) "oversized source published a final bundle"
    Assert-Equal 0 @(Get-TemporaryAcquisitionArtifacts -Root $oversizedRoot).Count "oversized-source failure left temporary artifacts"

    $invalidSizeRoot = Join-Path $sandboxRoot "invalid manifest size"
    $invalidSizeFixtures = Join-Path $invalidSizeRoot "fixtures"
    $invalidSizeManifestPath = Join-Path $invalidSizeRoot "sources.json"
    New-Item -ItemType Directory -Force -Path $invalidSizeFixtures | Out-Null
    $invalidSizeManifest = New-SyntheticSourceManifest -Template $manifest -FixtureRoot $invalidSizeFixtures -Path $invalidSizeManifestPath
    $invalidSizeManifest.sources[0].files[0].size_bytes = [long]$invalidSizeManifest.limits.max_file_bytes + 1L
    Write-JsonFile -Value $invalidSizeManifest -Path $invalidSizeManifestPath
    $invalidSizeDestination = Join-Path $invalidSizeRoot "destination"
    $invalidSizeResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile", "-File", $acquirerPath,
        "-ManifestPath", $invalidSizeManifestPath,
        "-DestinationRoot", $invalidSizeDestination
    )
    Assert-True ($invalidSizeResult.ExitCode -ne 0) "acquisition accepted size_bytes above max_file_bytes"
    Assert-True (Test-StringContains -Value $invalidSizeResult.Output -Needle "size_bytes exceeds max_file_bytes" -Comparison OrdinalIgnoreCase) "invalid-size failure omitted its bound diagnostic: $($invalidSizeResult.Output.Trim())"
    Assert-True (-not (Test-Path -LiteralPath $invalidSizeDestination)) "invalid size manifest created a final bundle"

    foreach ($metadataCase in @(
        [pscustomobject]@{ Name = "version"; Patch = 31; Protocol = 1001; Expected = "game version metadata mismatch" },
        [pscustomobject]@{ Name = "protocol"; Patch = 30; Protocol = 999; Expected = "protocol metadata mismatch" }
    )) {
        $caseRoot = Join-Path $sandboxRoot ("bad " + $metadataCase.Name)
        $caseFixtures = Join-Path $caseRoot "fixtures"
        $caseManifestPath = Join-Path $caseRoot "sources.json"
        New-Item -ItemType Directory -Force -Path $caseFixtures | Out-Null
        $caseManifest = New-SyntheticSourceManifest -Template $manifest -FixtureRoot $caseFixtures -Path $caseManifestPath
        Set-SyntheticProtocolInfo -Manifest $caseManifest -ManifestPath $caseManifestPath -FixtureRoot $caseFixtures -Patch $metadataCase.Patch -Protocol $metadataCase.Protocol
        $caseDestination = Join-Path $caseRoot "destination"
        $caseResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
            "-NoProfile", "-File", $acquirerPath,
            "-ManifestPath", $caseManifestPath,
            "-DestinationRoot", $caseDestination
        )
        Assert-True ($caseResult.ExitCode -ne 0) "acquisition accepted mismatched $($metadataCase.Name) metadata"
        Assert-True (Test-StringContains -Value $caseResult.Output -Needle ([string]$metadataCase.Expected) -Comparison OrdinalIgnoreCase) "mismatched $($metadataCase.Name) failure omitted '$($metadataCase.Expected)': $($caseResult.Output.Trim())"
        Assert-True (-not (Test-Path -LiteralPath $caseDestination)) "mismatched $($metadataCase.Name) metadata published a final bundle"
        Assert-Equal 0 @(Get-TemporaryAcquisitionArtifacts -Root $caseRoot).Count "mismatched $($metadataCase.Name) metadata left temporary artifacts"
    }

    $raceRoot = Join-Path $sandboxRoot "publish-race"
    $raceFixtures = Join-Path $raceRoot "fixtures"
    $raceManifestPath = Join-Path $raceRoot "sources.json"
    New-Item -ItemType Directory -Force -Path $raceFixtures | Out-Null
    $null = New-SyntheticSourceManifest -Template $manifest -FixtureRoot $raceFixtures -Path $raceManifestPath
    $winnerBundle = Join-Path $raceRoot "winner"
    $winnerSeed = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile", "-File", $acquirerPath,
        "-ManifestPath", $raceManifestPath,
        "-DestinationRoot", $winnerBundle
    )
    Assert-Equal 0 $winnerSeed.ExitCode "could not seed a valid concurrent-winner bundle: $($winnerSeed.Output.Trim())"

    $raceDestination = Join-Path $raceRoot "destination"
    $watcher = [System.IO.FileSystemWatcher]::new($raceRoot, "destination.installing-*")
    $watcher.IncludeSubdirectories = $false
    $watcher.EnableRaisingEvents = $true
    $raceJob = Start-Job -ScriptBlock {
        param($PowerShellPath, $ScriptPath, $SourceManifest, $Destination)
        $savedErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $output = & $PowerShellPath -NoProfile -File $ScriptPath `
                -ManifestPath $SourceManifest -DestinationRoot $Destination 2>&1 | Out-String
            $exitCode = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $savedErrorActionPreference
        }
        [pscustomobject]@{ ExitCode = $exitCode; Output = $output }
    } -ArgumentList $childPowerShell, $acquirerPath, $raceManifestPath, $raceDestination
    try {
        $stageCreated = $watcher.WaitForChanged([System.IO.WatcherChangeTypes]::Created, 10000)
        Assert-True (-not $stageCreated.TimedOut) "race fixture did not observe creation of the staging directory"
        [System.IO.Directory]::Move($winnerBundle, $raceDestination)
        $completedRaceJob = Wait-Job -Job $raceJob -Timeout 30
        Assert-True ($null -ne $completedRaceJob) "race fixture acquisition did not finish"
        $raceResult = Receive-Job -Job $raceJob
    } finally {
        $watcher.Dispose()
        if ($raceJob.State -notin @("Completed", "Failed", "Stopped")) {
            Stop-Job -Job $raceJob
        }
        Remove-Job -Job $raceJob -Force
    }
    Assert-Equal 0 ([int]$raceResult.ExitCode) "concurrent-winner acquisition failed: $([string]$raceResult.Output)"
    Assert-True (Test-StringContains -Value ([string]$raceResult.Output) -Needle "concurrent publisher" -Comparison OrdinalIgnoreCase) "concurrent-winner acquisition did not report its outcome"
    Assert-True (Test-Path -LiteralPath $raceDestination -PathType Container) "concurrent winner bundle is missing"
    Assert-Equal "licenses,pmmp,prismarine" (@(Get-ChildItem -LiteralPath $raceDestination -Force | Sort-Object Name | ForEach-Object { $_.Name }) -join ",") "concurrent winner bundle contains unexpected top-level entries"
    Assert-Equal 0 @(Get-TemporaryAcquisitionArtifacts -Root $raceRoot).Count "concurrent publication left a staging or partial artifact"
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $raceDestination -Force -Recurse | Where-Object { $_.Name -match "\.installing-" }).Count "staging directory was nested inside the winning bundle"

    $unexpectedEntry = Join-Path $raceDestination "unexpected-entry.txt"
    [System.IO.File]::WriteAllText($unexpectedEntry, "unexpected", [System.Text.UTF8Encoding]::new($false))
    $unexpectedResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile", "-File", $acquirerPath,
        "-ManifestPath", $raceManifestPath,
        "-DestinationRoot", $raceDestination
    )
    Assert-True ($unexpectedResult.ExitCode -ne 0) "installed bundle validation accepted an unexpected entry"
    Assert-True (Test-StringContains -Value $unexpectedResult.Output -Needle "unexpected entry" -Comparison OrdinalIgnoreCase) "unexpected-entry rejection omitted its diagnostic: $($unexpectedResult.Output.Trim())"
    Remove-Item -LiteralPath $unexpectedEntry -Force

    if ([System.IO.Path]::DirectorySeparatorChar -eq [char]92) {
        $junctionProbeTarget = Join-Path $sandboxRoot "junction probe target"
        $junctionProbe = Join-Path $sandboxRoot "junction probe"
        New-Item -ItemType Directory -Path $junctionProbeTarget | Out-Null
        $junctionAvailable = $false
        try {
            New-Item -ItemType Junction -Path $junctionProbe -Target $junctionProbeTarget -ErrorAction Stop | Out-Null
            $junctionAvailable = $true
        } catch {
            Write-Output "SKIP: Windows junction contract unavailable: $($_.Exception.Message)"
        } finally {
            if ($junctionAvailable) {
                Remove-TestJunction -Path $junctionProbe
            }
        }

        if ($junctionAvailable) {
            $ancestorJunctionTarget = Join-Path $sandboxRoot "ancestor junction target"
            $ancestorJunction = Join-Path $sandboxRoot "ancestor junction"
            $ancestorDestination = Join-Path $ancestorJunction "new parent\destination"
            New-Item -ItemType Directory -Path $ancestorJunctionTarget | Out-Null
            New-Item -ItemType Junction -Path $ancestorJunction -Target $ancestorJunctionTarget | Out-Null
            try {
                $ancestorJunctionResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
                    "-NoProfile", "-File", $acquirerPath,
                    "-ManifestPath", $syntheticManifestPath,
                    "-DestinationRoot", $ancestorDestination
                )
            } finally {
                Remove-TestJunction -Path $ancestorJunction
            }
            Assert-True ($ancestorJunctionResult.ExitCode -ne 0) "acquisition accepted a junction above its nonexistent destination root"
            Assert-True (Test-StringContains -Value $ancestorJunctionResult.Output -Needle "reparse point" -Comparison OrdinalIgnoreCase) "ancestor-junction failure omitted its diagnostic: $($ancestorJunctionResult.Output.Trim())"
            Assert-Equal 0 @(Get-ChildItem -LiteralPath $ancestorJunctionTarget -Force).Count "ancestor junction target was mutated"

            $destinationJunctionRoot = Join-Path $sandboxRoot "destination junction case"
            $destinationJunctionTarget = Join-Path $sandboxRoot "destination junction target"
            $destinationJunction = Join-Path $destinationJunctionRoot "pmmp"
            New-Item -ItemType Directory -Path $destinationJunctionRoot, $destinationJunctionTarget | Out-Null
            New-Item -ItemType Junction -Path $destinationJunction -Target $destinationJunctionTarget | Out-Null
            try {
                $destinationJunctionResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
                    "-NoProfile", "-File", $acquirerPath,
                    "-ManifestPath", $syntheticManifestPath,
                    "-DestinationRoot", $destinationJunctionRoot
                )
            } finally {
                Remove-TestJunction -Path $destinationJunction
            }
            Assert-True ($destinationJunctionResult.ExitCode -ne 0) "acquisition accepted a destination ancestor junction"
            Assert-True (Test-StringContains -Value $destinationJunctionResult.Output -Needle "reparse point" -Comparison OrdinalIgnoreCase) "destination-junction failure omitted its diagnostic: $($destinationJunctionResult.Output.Trim())"
            Assert-Equal 0 @(Get-ChildItem -LiteralPath $destinationJunctionTarget -Force).Count "destination junction target was mutated"

            $cacheJunctionCase = Join-Path $sandboxRoot "cache junction case"
            $cacheJunctionDestination = Join-Path $cacheJunctionCase "destination"
            $cacheJunctionRoot = "$cacheJunctionDestination.downloads"
            $cacheJunctionTarget = Join-Path $cacheJunctionCase "redirect target"
            $cacheSourceJunction = Join-Path $cacheJunctionRoot "pmmp-bedrock-data"
            New-Item -ItemType Directory -Path $cacheJunctionCase, $cacheJunctionRoot, $cacheJunctionTarget | Out-Null
            New-Item -ItemType Junction -Path $cacheSourceJunction -Target $cacheJunctionTarget | Out-Null
            try {
                $cacheJunctionResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
                    "-NoProfile", "-File", $acquirerPath,
                    "-ManifestPath", $syntheticManifestPath,
                    "-DestinationRoot", $cacheJunctionDestination
                )
            } finally {
                Remove-TestJunction -Path $cacheSourceJunction
            }
            Assert-True ($cacheJunctionResult.ExitCode -ne 0) "acquisition accepted a cache ancestor junction"
            Assert-True (Test-StringContains -Value $cacheJunctionResult.Output -Needle "reparse point" -Comparison OrdinalIgnoreCase) "cache-junction failure omitted its diagnostic: $($cacheJunctionResult.Output.Trim())"
            Assert-Equal 0 @(Get-ChildItem -LiteralPath $cacheJunctionTarget -Force).Count "cache junction target was mutated"
            Assert-True (-not (Test-Path -LiteralPath $cacheJunctionDestination)) "cache junction failure published a final bundle"
        }
    }
} finally {
    if (Test-Path -LiteralPath $sandboxRoot) {
        Remove-Item -LiteralPath $sandboxRoot -Recurse -Force
    }
}

Write-Output "block data acquisition contract tests passed"
