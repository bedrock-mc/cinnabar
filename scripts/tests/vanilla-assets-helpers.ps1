# Shared vanilla-assets contract-test helpers: native capture, whitespace-free
# output matching, SHA-256, manifest writing, and synthetic ZIP fixtures.
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

function Test-OutputContains {
    param(
        [Parameter(Mandatory = $true)]
        [AllowEmptyString()]
        [string]$Output,
        [Parameter(Mandatory = $true)]
        [string]$Needle
    )

    # Console rendering wraps long diagnostics mid-token, so compare the
    # whitespace-free forms instead of the literal rendered text.
    $whitespace = [regex]"\s+"
    return $whitespace.Replace($Output, "").Contains($whitespace.Replace($Needle, ""))
}

function Get-TestSha256Hex {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $stream = [System.IO.File]::OpenRead([System.IO.Path]::GetFullPath($Path))
    try {
        $hasher = [System.Security.Cryptography.SHA256]::Create()
        try {
            return [System.BitConverter]::ToString($hasher.ComputeHash($stream)).Replace("-", "").ToLowerInvariant()
        } finally {
            $hasher.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Write-TestManifest {
    param(
        [Parameter(Mandatory = $true)]
        [psobject]$Template,
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [AllowEmptyString()]
        [string]$Archive,
        [Parameter(Mandatory = $true)]
        [string]$CacheDirectory,
        [string]$Sha256 = "",
        [string]$Url = ""
    )

    $manifest = [ordered]@{}
    foreach ($property in $Template.PSObject.Properties) {
        $manifest[$property.Name] = $property.Value
    }
    $manifest["archive"] = $Archive
    $manifest["cache_dir"] = $CacheDirectory
    if (-not [string]::IsNullOrWhiteSpace($Sha256)) {
        $manifest["sha256"] = $Sha256
    }
    if (-not [string]::IsNullOrWhiteSpace($Url)) {
        $manifest["url"] = $Url
    }
    $manifest | ConvertTo-Json | Set-Content -LiteralPath $Path -Encoding UTF8
}

function New-TestZipArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [object[]]$Entries,
        [switch]$Raw
    )

    if ($Raw) {
        Write-RawTestZipArchive -Path $Path -Entries $Entries
        return
    }

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $parent = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    $stream = [System.IO.File]::Open(
        $Path,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::ReadWrite,
        [System.IO.FileShare]::None
    )
    try {
        $zip = [System.IO.Compression.ZipArchive]::new(
            $stream,
            [System.IO.Compression.ZipArchiveMode]::Create,
            $true
        )
        try {
            foreach ($entrySpec in $Entries) {
                $entry = $zip.CreateEntry([string]$entrySpec.Name)
                if ($null -ne $entrySpec.Content) {
                    $entryStream = $entry.Open()
                    try {
                        $writer = [System.IO.StreamWriter]::new(
                            $entryStream,
                            [System.Text.UTF8Encoding]::new($false),
                            1024,
                            $true
                        )
                        try {
                            $writer.Write([string]$entrySpec.Content)
                        } finally {
                            $writer.Dispose()
                        }
                    } finally {
                        $entryStream.Dispose()
                    }
                }
            }
        } finally {
            $zip.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

# VPA-209 fixtures need ZIP entries that System.IO.Compression cannot emit
# (a Unix-made symlink entry). This minimal stored-entry writer records real
# CRCs so both platform extractors treat the fixture as a well-formed zip.
$script:TestCrcTable = $null

function Get-TestCrc32Table {
    if ($null -eq $script:TestCrcTable) {
        $table = New-Object 'uint32[]' 256
        $poly = [uint64]3988292384   # 0xEDB88320
        for ($i = 0; $i -lt 256; $i++) {
            $c = [uint64]$i
            for ($j = 0; $j -lt 8; $j++) {
                if (($c -band [uint64]1) -ne 0) { $c = ($c -shr 1) -bxor $poly } else { $c = $c -shr 1 }
            }
            $table[$i] = [uint32]$c
        }
        $script:TestCrcTable = $table
    }
    return $script:TestCrcTable
}

function Get-TestCrc32 {
    param([byte[]]$Bytes)
    $table = Get-TestCrc32Table
    $crc = [uint64]4294967295     # 0xFFFFFFFF
    foreach ($b in $Bytes) {
        $index = (($crc -bxor [uint64]$b) -band [uint64]0xFF)
        $crc = (($crc -shr 8) -bxor [uint64]$table[$index])
    }
    return [uint32]($crc -bxor [uint64]4294967295)
}

function Write-RawTestZipArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [object[]]$Entries
    )

    $parent = Split-Path -Parent $Path
    New-Item -ItemType Directory -Force -Path $parent | Out-Null
    $body = New-Object System.IO.MemoryStream
    $bw = New-Object System.IO.BinaryWriter $body
    $central = New-Object System.IO.MemoryStream
    $cw = New-Object System.IO.BinaryWriter $central
    $runningOffset = [int64]0
    foreach ($spec in $Entries) {
        $specObject = [pscustomobject]$spec
        $nameBytes = [System.Text.Encoding]::UTF8.GetBytes([string]$specObject.Name)
        [byte[]]$data = [System.Text.Encoding]::UTF8.GetBytes([string]$specObject.Content)
        if ($null -eq $data -or $data.Length -eq 0) { $data = [byte[]]@() }
        $crc = Get-TestCrc32 -Bytes $data
        $madeByOs = 0
        if ($specObject.PSObject.Properties.Name -contains "MadeByOs") { $madeByOs = [int]$specObject.MadeByOs }
        $externalAttributes = [int64]0
        if ($specObject.PSObject.Properties.Name -contains "ExternalAttributes") { $externalAttributes = [int64]$specObject.ExternalAttributes }

        $localOffset = $runningOffset
        $bw.Write([uint32]0x04034B50); $bw.Write([uint16]20); $bw.Write([uint16]0); $bw.Write([uint16]0)
        $bw.Write([uint16]0); $bw.Write([uint16]0x2821)
        $bw.Write([uint32]$crc); $bw.Write([uint32]$data.Length); $bw.Write([uint32]$data.Length)
        $bw.Write([uint16]$nameBytes.Length); $bw.Write([uint16]0)
        $bw.Write($nameBytes); $bw.Write($data)

        $cw.Write([uint32]0x02014B50)
        $cw.Write([uint16](($madeByOs -shl 8) -bor 20)); $cw.Write([uint16]20)
        $cw.Write([uint16]0); $cw.Write([uint16]0)
        $cw.Write([uint16]0); $cw.Write([uint16]0x2821)
        $cw.Write([uint32]$crc); $cw.Write([uint32]$data.Length); $cw.Write([uint32]$data.Length)
        $cw.Write([uint16]$nameBytes.Length); $cw.Write([uint16]0); $cw.Write([uint16]0)
        $cw.Write([uint16]0); $cw.Write([uint16]0); $cw.Write([uint32]$externalAttributes)
        $cw.Write([uint32]$localOffset); $cw.Write($nameBytes)

        $runningOffset = $localOffset + 30 + $nameBytes.Length + $data.Length
    }
    $cdOffset = [int64]$bw.BaseStream.Position
    $centralBytes = $central.ToArray()
    $bw.Write($centralBytes)
    $bw.Write([uint32]0x06054B50); $bw.Write([uint16]0); $bw.Write([uint16]0)
    $bw.Write([uint16]$Entries.Count); $bw.Write([uint16]$Entries.Count)
    $bw.Write([uint32]$centralBytes.Length); $bw.Write([uint32]$cdOffset); $bw.Write([uint16]0)
    $bw.Flush()
    [System.IO.File]::WriteAllBytes($Path, $body.ToArray())
    $bw.Dispose(); $cw.Dispose(); $body.Dispose(); $central.Dispose()
}

# VPA-209 publish-race witness for the bash extractor's pre-publication
# absence recheck. A concurrent writer creating the cache target between
# extraction and publication must fail the run closed instead of letting
# POSIX `mv dir target` move staging INTO the live directory and corrupting
# the published layout. Injection is deterministic: a PATH-shimmed unzip
# delegates to the real Info-ZIP binary, then creates the publication target
# immediately after extraction returns, inside the fetcher's pre-move window.
# The caller owns fixture construction and manifest pinning; this helper
# writes the shell harness, drives the fetcher, and returns failure strings.
function Test-PublishRaceInjection {
    param(
        [Parameter(Mandatory = $true)]
        [string]$BashPath,
        [Parameter(Mandatory = $true)]
        [string]$ShimRoot,
        [Parameter(Mandatory = $true)]
        [string]$FetcherPath,
        [Parameter(Mandatory = $true)]
        [string]$CacheDirectory,
        [Parameter(Mandatory = $true)]
        [string]$AssetRoot,
        [Parameter(Mandatory = $true)]
        [string]$StagingResidueFilter
    )

    $failures = @()
    New-Item -ItemType Directory -Force -Path $ShimRoot | Out-Null
    # Generated shell fixtures are written byte-exactly with LF endings:
    # POSIX bash parses CR bytes as ordinary characters on every CI host,
    # so Set-Content's platform newline must not touch these bodies.
    $raceLauncherBody = @(
        '#!/usr/bin/env bash'
        '# Publish-race harness launcher: resolves the real unzip before'
        '# shims shadow it, prepends the shim directory to PATH so every'
        '# fetcher unzip call lands in the racing shim, then runs the real'
        '# fetcher with the accepted-EULA flag.'
        'set -u'
        'here="$(cd -- "$(dirname -- "$0")" && pwd)"'
        'real_unzip="$(command -v unzip)" || exit 127'
        'export CINNABAR_TEST_REAL_UNZIP="$real_unzip"'
        'export PATH="$here:$PATH"'
        'if [ "$#" -lt 1 ]; then'
        '    exit 64'
        'fi'
        'fetcher="$1"'
        'shift'
        'exec bash "$fetcher" --accept-eula "$@"'
    ) -join "`n"
    [System.IO.File]::WriteAllText((Join-Path $ShimRoot "run-fetcher-with-race.sh"), $raceLauncherBody + "`n")
    $racingUnzipBody = @(
        '#!/usr/bin/env bash'
        '# Racing unzip shim: delegates every invocation to the real'
        '# binary, then -- only for an extraction run (-d <staging>) --'
        '# simulates the concurrent writer that creates the publication'
        '# target during extraction, before the fetcher publishes.'
        'set -u'
        'real="${CINNABAR_TEST_REAL_UNZIP:-}"'
        'if [ -z "$real" ]; then'
        '    echo "racing unzip shim: real unzip path unavailable" >&2'
        '    exit 69'
        'fi'
        'staging=""'
        'prev=""'
        'for arg in "$@"; do'
        '    if [ "$prev" = "-d" ]; then'
        '        staging="$arg"'
        '    fi'
        '    prev="$arg"'
        'done'
        '"$real" "$@" || exit $?'
        'if [ -n "$staging" ]; then'
        '    cache_dir="${staging%.extracting.*}"'
        '    if [ "$cache_dir" != "$staging" ] && mkdir -p -- "$cache_dir/resource_pack"; then'
        '        printf ''%s\n'' ''{"injected": true}'' > "$cache_dir/resource_pack/blocks.json"'
        '        printf ''%s\n'' ''concurrent writer created the target during extraction'' > "$cache_dir/injected-sentinel.txt"'
        '    fi'
        'fi'
        'exit 0'
    ) -join "`n"
    [System.IO.File]::WriteAllText((Join-Path $ShimRoot "unzip"), $racingUnzipBody + "`n")
    $raceChmodTargets = @(
        (Join-Path $ShimRoot "run-fetcher-with-race.sh"),
        (Join-Path $ShimRoot "unzip")
    ) | ForEach-Object { "'" + $_.Replace('\', '/') + "'" }
    $raceChmod = Invoke-NativeCapture -FilePath $BashPath -ArgumentList @(
        "-c",
        "chmod +x $($raceChmodTargets -join ' ')"
    )
    if ($raceChmod.ExitCode -ne 0) {
        throw "publish-race harness could not mark its shell fixtures executable: $($raceChmod.Output.Trim())"
    }

    $raceResult = Invoke-NativeCapture -FilePath $BashPath -ArgumentList @(
        (Join-Path $ShimRoot "run-fetcher-with-race.sh").Replace('\', '/'),
        $FetcherPath.Replace('\', '/')
    )
    if ($raceResult.ExitCode -eq 0) {
        $failures += "publish-race(bash): unexpectedly succeeded while the cache target appeared during extraction"
    }
    if (-not (Test-OutputContains -Output $raceResult.Output -Needle "cache directory appeared during extraction")) {
        $failures += "publish-race(bash): omitted the appeared-during-extraction diagnostic: $($raceResult.Output.Trim())"
    }
    if (-not (Test-Path -LiteralPath (Join-Path $CacheDirectory "injected-sentinel.txt") -PathType Leaf)) {
        $failures += "publish-race(bash): the concurrently created cache directory did not survive intact"
    }
    $injectedBlocks = Join-Path $CacheDirectory "resource_pack\blocks.json"
    if ((Test-Path -LiteralPath $injectedBlocks -PathType Leaf) -and
        -not ((Get-Content -Raw -LiteralPath $injectedBlocks).Contains("injected"))) {
        $failures += "publish-race(bash): overwrote the concurrently created cache contents"
    }
    $movedIntoCache = @(Get-ChildItem -Force -LiteralPath $CacheDirectory -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -like "synthetic-vanilla*" })
    if ($movedIntoCache.Count -ne 0) {
        $failures += "publish-race(bash): staged extraction was moved INTO the live cache directory: $($movedIntoCache.FullName -join ', ')"
    }
    $raceResidue = @(Get-ChildItem -Force -LiteralPath $AssetRoot -Directory -Filter $StagingResidueFilter -ErrorAction SilentlyContinue)
    if ($raceResidue.Count -ne 0) {
        $failures += "publish-race(bash): left extraction staging behind: $($raceResidue.FullName -join ', ')"
    }
    return $failures
}
