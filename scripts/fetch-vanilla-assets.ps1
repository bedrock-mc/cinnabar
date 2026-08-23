[CmdletBinding()]
param(
    [switch]$AcceptEula,
    [switch]$DryRun,

    # VPA-209 additive test overrides. They may only TIGHTEN a bound (a
    # nonzero value greater than the built-in constant is refused), so they
    # can never relax production safety; production callers omit them.
    [long]$MaxArchiveEntriesOverride = 0,
    [long]$MaxExpandedFileBytesOverride = 0,
    [long]$MaxTotalExpandedBytesOverride = 0,
    [double]$MaxPerEntryCompressionRatioOverride = 0,
    [double]$MaxAggregateCompressionRatioOverride = 0
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# ---------------------------------------------------------------------------
# VPA-209 provisional extraction bounds.
#
# PROVISIONAL-UNLESS-MEASURED: these constants carry explicit headroom over
# the pinned pack inventory measured locally from the exact pinned
# bedrock-samples v1.26.30.32-preview-full artifact (payload stays outside
# git): 21,493 entries (620 directory + 20,873 file), 297,074,336 bytes
# total expanded, largest single file 3,460,610 bytes, largest legitimate
# per-entry compression ratio about 120.5 (a .tga texture), aggregate
# compression ratio 1.99. Raise a bound only after re-measuring a newer
# pinned inventory; never loosen them to admit an unmeasured archive.
# ---------------------------------------------------------------------------
$script:DefaultMaxArchiveEntries = [long]65536
$script:DefaultMaxExpandedFileBytes = [long]67108864          # 64 MiB
$script:DefaultMaxTotalExpandedBytes = [long]1073741824       # 1 GiB
$script:MinRatioSampleCompressedBytes = [long]4096
$script:DefaultMaxPerEntryCompressionRatio = [double]500
$script:DefaultMaxAggregateCompressionRatio = [double]100
$script:ExtractionCopyBufferBytes = 1048576

# Staging reclamation policy for runs interrupted by process death (Ctrl+C
# between pipeline stops, kill, power loss). Provisional-unless-measured.
$script:StaleStagingMaxAgeSeconds = [long]86400               # 24 hours
$script:StaleStagingMaxRemaining = 4

function Resolve-TightenedLimit {
    param(
        [Parameter(Mandatory = $true)]
        [long]$Default,
        [Parameter(Mandatory = $true)]
        [long]$Override,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ($Override -lt 0) {
        throw "-$Name must not be negative"
    }
    if ($Override -eq 0) {
        return $Default
    }
    if ($Override -gt $Default) {
        throw "-$Name $Override exceeds the built-in maximum $Default; overrides may only tighten bounds"
    }
    return $Override
}

function Resolve-TightenedRatioLimit {
    param(
        [Parameter(Mandatory = $true)]
        [double]$Default,
        [Parameter(Mandatory = $true)]
        [double]$Override,
        [Parameter(Mandatory = $true)]
        [string]$Name
    )

    if ($Override -lt 0) {
        throw "-$Name must not be negative"
    }
    if ($Override -eq 0) {
        return $Default
    }
    if ($Override -gt $Default) {
        throw "-$Name $Override exceeds the built-in maximum $Default; overrides may only tighten bounds"
    }
    return $Override
}

function ConvertTo-ExtendedLengthPath {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $fullPath = [System.IO.Path]::GetFullPath($Path)
    if ([System.IO.Path]::DirectorySeparatorChar -ne [char]92 -or
        $fullPath.StartsWith("\\?\", [System.StringComparison]::Ordinal)) {
        return $fullPath
    }
    if ($fullPath.StartsWith("\\", [System.StringComparison]::Ordinal)) {
        return "\\?\UNC\$($fullPath.Substring(2))"
    }
    return "\\?\$fullPath"
}

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    # Windows PowerShell 5.1 exports Get-FileHash as a *function* from
    # Microsoft.PowerShell.Utility.psm1, not as a binary cmdlet, so it is the
    # only command in this script that needs that script module to auto-load.
    # A runner where auto-loading does not happen still resolves every cmdlet
    # from the pre-loaded snap-in, which is why a fetch can download the whole
    # archive and only then fail with CommandNotFoundException on Get-FileHash.
    # Hash through the same .NET primitive the cmdlet itself streams over, so
    # pinned verification keeps identical strength without that dependency.
    $stream = [System.IO.File]::Open(
        (ConvertTo-ExtendedLengthPath -Path $Path),
        [System.IO.FileMode]::Open,
        [System.IO.FileAccess]::Read,
        [System.IO.FileShare]::Read
    )
    try {
        $hasher = [System.Security.Cryptography.SHA256]::Create()
        try {
            $digest = $hasher.ComputeHash($stream)
        } finally {
            $hasher.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
    if ($null -eq $digest -or $digest.Length -ne 32) {
        throw "SHA-256 digest computation failed for $Path"
    }
    return [System.BitConverter]::ToString($digest).Replace("-", "").ToLowerInvariant()
}

function Remove-ExtractionTree {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $extendedPath = ConvertTo-ExtendedLengthPath -Path $Path
    if ([System.IO.Directory]::Exists($extendedPath)) {
        [System.IO.Directory]::Delete($extendedPath, $true)
    }
}

function Remove-StaleExtractionStaging {
    # VPA-209: reclaim staging directories abandoned by interrupted runs
    # (process kill, power loss). Only siblings of the cache path whose name
    # continues this script's own ".extracting" marker are considered; they
    # are reclaimed when older than MaxAgeSeconds, or when more than
    # MaxRemaining fresher leftovers exist (oldest deleted first).
    param(
        [Parameter(Mandatory = $true)]
        [string]$CachePath,
        [Parameter(Mandatory = $true)]
        [string]$CacheParent,
        [Parameter(Mandatory = $true)]
        [long]$MaxAgeSeconds,
        [Parameter(Mandatory = $true)]
        [int]$MaxRemaining
    )

    if (-not [System.IO.Directory]::Exists($CacheParent)) {
        return
    }
    $prefix = (Split-Path -Leaf $CachePath) + ".extracting"
    $candidates = [System.Collections.Generic.List[System.IO.DirectoryInfo]]::new()
    foreach ($child in [System.IO.Directory]::EnumerateDirectories($CacheParent)) {
        $leaf = [System.IO.Path]::GetFileName($child)
        if ($leaf.StartsWith($prefix, [System.StringComparison]::Ordinal)) {
            $candidates.Add([System.IO.DirectoryInfo]::new($child))
        }
    }
    if ($candidates.Count -eq 0) {
        return
    }

    $ordered = @($candidates | Sort-Object -Property LastWriteTimeUtc -Descending)
    $utcNow = [System.DateTime]::UtcNow
    $kept = 0
    $reclaimed = 0
    foreach ($candidate in $ordered) {
        $ageSeconds = ($utcNow - $candidate.LastWriteTimeUtc).TotalSeconds
        if ($kept -lt $MaxRemaining -and $ageSeconds -le [double]$MaxAgeSeconds) {
            $kept++
            continue
        }
        Remove-ExtractionTree -Path $candidate.FullName
        $reclaimed++
    }
    if ($reclaimed -gt 0) {
        Write-Output "Reclaimed $reclaimed stale extraction staging director(y/ies)"
    }
}

function Expand-ZipArchiveBounded {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath,
        [Parameter(Mandatory = $true)]
        [string]$DestinationPath,
        [Parameter(Mandatory = $true)]
        [long]$MaxArchiveEntries,
        [Parameter(Mandatory = $true)]
        [long]$MaxExpandedFileBytes,
        [Parameter(Mandatory = $true)]
        [long]$MaxTotalExpandedBytes,
        [Parameter(Mandatory = $true)]
        [long]$MinRatioSampleCompressedBytes,
        [Parameter(Mandatory = $true)]
        [double]$MaxPerEntryCompressionRatio,
        [Parameter(Mandatory = $true)]
        [double]$MaxAggregateCompressionRatio
    )

    Add-Type -AssemblyName System.IO.Compression
    Add-Type -AssemblyName System.IO.Compression.FileSystem

    $destinationRoot = [System.IO.Path]::GetFullPath($DestinationPath)
    $destinationPrefix = $destinationRoot.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    $pathComparison = if ([System.IO.Path]::DirectorySeparatorChar -eq [char]92) {
        [System.StringComparison]::OrdinalIgnoreCase
    } else {
        [System.StringComparison]::Ordinal
    }
    $invalidFileNameCharacters = [System.IO.Path]::GetInvalidFileNameChars()
    $nodes = @{}
    $plannedEntries = [System.Collections.Generic.List[object]]::new()
    $declaredTotalExpanded = [long]0
    $declaredTotalCompressed = [long]0

    $archiveStream = [System.IO.File]::OpenRead($ArchivePath)
    try {
        $zip = [System.IO.Compression.ZipArchive]::new(
            $archiveStream,
            [System.IO.Compression.ZipArchiveMode]::Read,
            $false
        )
        try {
            if ($zip.Entries.Count -gt $MaxArchiveEntries) {
                throw "archive entry count $($zip.Entries.Count) exceeds the maximum $MaxArchiveEntries"
            }
            foreach ($entry in $zip.Entries) {
                $rawName = [string]$entry.FullName
                if ([string]::IsNullOrWhiteSpace($rawName) -or
                    $rawName.IndexOf([char]0) -ge 0) {
                    throw "unsafe ZIP entry '$rawName': path is empty or contains a null character"
                }
                if ($rawName.StartsWith("/", [System.StringComparison]::Ordinal) -or
                    $rawName.StartsWith("\", [System.StringComparison]::Ordinal)) {
                    throw "unsafe ZIP entry '$rawName': absolute and UNC paths are not allowed"
                }

                $normalizedName = $rawName.Replace("\", "/")
                if ($normalizedName.Contains("//")) {
                    throw "unsafe ZIP entry '$rawName': empty path components are not allowed"
                }
                $isDirectory = $normalizedName.EndsWith("/", [System.StringComparison]::Ordinal)
                if ($isDirectory -and $entry.Length -ne 0) {
                    throw "unsafe ZIP entry '$rawName': directory entries must be empty"
                }
                $trimmedName = $normalizedName.TrimEnd([char]47)
                if ([string]::IsNullOrWhiteSpace($trimmedName)) {
                    throw "unsafe ZIP entry '$rawName': path is empty"
                }

                $parts = $trimmedName.Split([char]47)
                foreach ($part in $parts) {
                    if ([string]::IsNullOrEmpty($part)) {
                        throw "unsafe ZIP entry '$rawName': empty path components are not allowed"
                    }
                    if ($part -eq "." -or $part -eq "..") {
                        throw "unsafe ZIP entry '$rawName': traversal components are not allowed"
                    }
                    if ($part.Contains(":")) {
                        throw "unsafe ZIP entry '$rawName': drive and alternate-stream paths are not allowed"
                    }
                    if ($part.IndexOfAny($invalidFileNameCharacters) -ge 0 -or
                        $part.EndsWith(" ", [System.StringComparison]::Ordinal) -or
                        $part.EndsWith(".", [System.StringComparison]::Ordinal)) {
                        throw "unsafe ZIP entry '$rawName': invalid filename component '$part'"
                    }
                    $deviceBaseName = $part.Split([char]46)[0]
                    if ($deviceBaseName -match "^(?i:CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])$") {
                        throw "unsafe ZIP entry '$rawName': reserved filename component '$part'"
                    }
                }

                $relativePath = $parts -join [string][System.IO.Path]::DirectorySeparatorChar
                $entryDestination = [System.IO.Path]::GetFullPath(
                    (Join-Path $destinationRoot $relativePath)
                )
                if (-not $entryDestination.StartsWith($destinationPrefix, $pathComparison)) {
                    throw "unsafe ZIP entry '$rawName': path escapes the extraction root"
                }

                $currentPath = ""
                for ($index = 0; $index -lt $parts.Count; $index++) {
                    $currentPath = if ($index -eq 0) {
                        $parts[$index]
                    } else {
                        "$currentPath/$($parts[$index])"
                    }
                    $isLeaf = $index -eq ($parts.Count - 1)
                    $kind = if ($isLeaf -and -not $isDirectory) { "file" } else { "directory" }
                    if ($nodes.ContainsKey($currentPath)) {
                        $node = $nodes[$currentPath]
                        if (-not [string]::Equals(
                            [string]$node.Path,
                            $currentPath,
                            [System.StringComparison]::Ordinal
                        ) -or [string]$node.Kind -ne $kind) {
                            throw "unsafe ZIP entry '$rawName': ZIP entry path collision at '$currentPath'"
                        }
                        if ($isLeaf) {
                            if ($kind -eq "file" -or [bool]$node.Explicit) {
                                throw "unsafe ZIP entry '$rawName': duplicate ZIP entry path '$currentPath'"
                            }
                            $node.Explicit = $true
                        }
                    } else {
                        $nodes[$currentPath] = [pscustomobject]@{
                            Path = $currentPath
                            Kind = $kind
                            Explicit = $isLeaf
                        }
                    }
                }

                # VPA-209: reject link entries before anything is written.
                # System.IO.Compression never materializes symlinks, so the
                # high Unix mode bits are the only signal; S_IFLNK is 0xA000.
                $externalAttributes = [int]$entry.ExternalAttributes
                if ((($externalAttributes -shr 16) -band 0xF000) -eq 0xA000) {
                    throw "unsafe ZIP entry '$rawName': link entries are not allowed"
                }

                # VPA-209: declared expanded bounds and per-entry bomb ratio,
                # checked against the central directory before any bytes are
                # written. The runtime copy below re-enforces both byte caps
                # against ACTUAL output, because a hostile archive can lie in
                # its central directory.
                if (-not $isDirectory) {
                    $declaredExpanded = [long]$entry.Length
                    $declaredCompressed = [long]$entry.CompressedLength
                    if ($declaredExpanded -gt $MaxExpandedFileBytes) {
                        throw "ZIP entry '$rawName' declared expanded size $declaredExpanded exceeds the maximum $MaxExpandedFileBytes bytes"
                    }
                    if ($declaredCompressed -ge $MinRatioSampleCompressedBytes -and
                        [double]$declaredExpanded -gt ($MaxPerEntryCompressionRatio * [double]$declaredCompressed)) {
                        throw (
                            "ZIP entry '{0}' compression ratio {1}:{2} exceeds the per-entry maximum {3}" -f
                                $rawName,
                                $declaredExpanded,
                                $declaredCompressed,
                                $MaxPerEntryCompressionRatio
                        )
                    }
                }
                $declaredTotalExpanded += [long]$entry.Length
                if ($declaredTotalExpanded -gt $MaxTotalExpandedBytes) {
                    throw "archive total declared expanded size $declaredTotalExpanded exceeds the maximum $MaxTotalExpandedBytes bytes"
                }
                $declaredTotalCompressed += [long]$entry.CompressedLength

                $plannedEntries.Add([pscustomobject]@{
                    Entry = $entry
                    Destination = $entryDestination
                    Directory = $isDirectory
                    RawName = $rawName
                })
            }

            # VPA-209: aggregate bomb ratio over the whole central directory.
            # A weighted average can never exceed the largest per-entry ratio,
            # so this guard only fires for distributed bombs whose individual
            # entries each stay under the per-entry threshold.
            if ($declaredTotalCompressed -ge $MinRatioSampleCompressedBytes -and
                [double]$declaredTotalExpanded -gt ($MaxAggregateCompressionRatio * [double]$declaredTotalCompressed)) {
                throw (
                    "archive aggregate compression ratio {0}:{1} exceeds the aggregate maximum {2}" -f
                        $declaredTotalExpanded,
                        $declaredTotalCompressed,
                        $MaxAggregateCompressionRatio
                )
            }

            # VPA-209: publishing tens of thousands of small files makes
            # repeated CreateDirectory calls measurable; remember which
            # directories were already ensured this run.
            $ensuredDirectories =
                [System.Collections.Generic.HashSet[string]]::new(
                    [System.StringComparer]::OrdinalIgnoreCase
                )
            foreach ($node in $nodes.Values) {
                if ([string]$node.Kind -ne "directory") {
                    continue
                }
                $nodePath = [string]$node.Path
                $absoluteNode = Join-Path $destinationRoot $nodePath
                if (-not $ensuredDirectories.Add($absoluteNode)) {
                    continue
                }
                [System.IO.Directory]::CreateDirectory(
                    (ConvertTo-ExtendedLengthPath -Path $absoluteNode)
                ) | Out-Null
            }

            $copyBuffer = New-Object byte[] $ExtractionCopyBufferBytes
            $totalWritten = [long]0
            foreach ($planned in $plannedEntries) {
                $extendedDestination = ConvertTo-ExtendedLengthPath -Path ([string]$planned.Destination)
                if ([bool]$planned.Directory) {
                    continue
                }

                $parent = [System.IO.Path]::GetDirectoryName([string]$planned.Destination)
                if ($ensuredDirectories.Add($parent)) {
                    [System.IO.Directory]::CreateDirectory(
                        (ConvertTo-ExtendedLengthPath -Path $parent)
                    ) | Out-Null
                }
                $inputStream = $planned.Entry.Open()
                try {
                    $outputStream = [System.IO.FileStream]::new(
                        $extendedDestination,
                        [System.IO.FileMode]::CreateNew,
                        [System.IO.FileAccess]::Write,
                        [System.IO.FileShare]::None
                    )
                    try {
                        $fileWritten = [long]0
                        while ($true) {
                            $read = $inputStream.Read($copyBuffer, 0, $copyBuffer.Length)
                            if ($read -le 0) {
                                break
                            }
                            $outputStream.Write($copyBuffer, 0, $read)
                            $fileWritten += [long]$read
                            $totalWritten += [long]$read
                            if ($fileWritten -gt $MaxExpandedFileBytes) {
                                throw "ZIP entry '$([string]$planned.RawName)' expanded size exceeded the maximum $MaxExpandedFileBytes bytes during extraction"
                            }
                            if ($totalWritten -gt $MaxTotalExpandedBytes) {
                                throw "archive total expanded size exceeded the maximum $MaxTotalExpandedBytes bytes during extraction"
                            }
                        }
                    } finally {
                        $outputStream.Dispose()
                    }
                } finally {
                    $inputStream.Dispose()
                }
            }
        } finally {
            $zip.Dispose()
        }
    } finally {
        $archiveStream.Dispose()
    }
}

if (-not $AcceptEula) {
    Write-Error "Refusing to fetch Mojang assets without the explicit -AcceptEula flag."
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifestPath = Join-Path $repoRoot "assets\vanilla-source.json"
if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "vanilla source manifest is missing: $manifestPath"
}

# VPA-209: resolve effective bounds. Overrides may only tighten.
$maxArchiveEntries = Resolve-TightenedLimit -Default $script:DefaultMaxArchiveEntries `
    -Override $MaxArchiveEntriesOverride -Name "MaxArchiveEntriesOverride"
$maxExpandedFileBytes = Resolve-TightenedLimit -Default $script:DefaultMaxExpandedFileBytes `
    -Override $MaxExpandedFileBytesOverride -Name "MaxExpandedFileBytesOverride"
$maxTotalExpandedBytes = Resolve-TightenedLimit -Default $script:DefaultMaxTotalExpandedBytes `
    -Override $MaxTotalExpandedBytesOverride -Name "MaxTotalExpandedBytesOverride"
$maxPerEntryCompressionRatio = Resolve-TightenedRatioLimit `
    -Default $script:DefaultMaxPerEntryCompressionRatio `
    -Override $MaxPerEntryCompressionRatioOverride -Name "MaxPerEntryCompressionRatioOverride"
$maxAggregateCompressionRatio = Resolve-TightenedRatioLimit `
    -Default $script:DefaultMaxAggregateCompressionRatio `
    -Override $MaxAggregateCompressionRatioOverride -Name "MaxAggregateCompressionRatioOverride"


$source = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
foreach ($property in @("url", "sha256", "artifact_policy", "cache_dir")) {
    if (-not ($source.PSObject.Properties.Name -contains $property) -or
        [string]::IsNullOrWhiteSpace([string]$source.$property)) {
        throw "vanilla source manifest is missing '$property'"
    }
}
if (-not ($source.PSObject.Properties.Name -contains "archive")) {
    throw "vanilla source manifest is missing 'archive'"
}
$archive = [string]$source.archive
if ([string]::IsNullOrEmpty($archive) -or
    $archive -eq "." -or
    $archive -eq ".." -or
    $archive.Contains("/") -or
    $archive.Contains("\") -or
    $archive -match "^[A-Za-z]:" -or
    [System.IO.Path]::IsPathRooted($archive)) {
    throw "archive must be exactly one nonempty basename"
}
if ([int]$source.schema -ne 1) {
    throw "unsupported vanilla source manifest schema: $($source.schema)"
}
if ([string]$source.artifact_policy -ne "local-only") {
    throw "vanilla source manifest must declare artifact_policy 'local-only'"
}

$assetRoot = [System.IO.Path]::GetFullPath((Join-Path $repoRoot ".local\assets"))
$cacheRelative = ([string]$source.cache_dir).Replace("/", [System.IO.Path]::DirectorySeparatorChar)
$cachePath = [System.IO.Path]::GetFullPath((Join-Path $repoRoot $cacheRelative))
$assetPrefix = $assetRoot.TrimEnd([System.IO.Path]::DirectorySeparatorChar) +
    [System.IO.Path]::DirectorySeparatorChar
$pathComparison = if ([System.IO.Path]::DirectorySeparatorChar -eq [char]92) {
    [System.StringComparison]::OrdinalIgnoreCase
} else {
    [System.StringComparison]::Ordinal
}
if (-not $cachePath.StartsWith($assetPrefix, $pathComparison)) {
    throw "cache_dir must stay below .local/assets: $($source.cache_dir)"
}

$downloadDirectory = Join-Path $assetRoot "downloads"
$archivePath = Join-Path $downloadDirectory $archive
$partialPath = "$archivePath.partial"
$cacheParent = Split-Path -Parent $cachePath
$temporaryExtract = "$cachePath.extracting-$PID-$([guid]::NewGuid().ToString('N'))"
$normalizedSource = Join-Path $cachePath "resource_pack\blocks.json"
$expectedSha256 = ([string]$source.sha256).ToLowerInvariant()

Write-Output "Manifest: $manifestPath"
Write-Output "Source URL: $($source.url)"
Write-Output "Expected SHA-256: $expectedSha256"
Write-Output "Partial download: $partialPath"
Write-Output "Verified archive: $archivePath"
Write-Output "Temporary extraction: $temporaryExtract"
Write-Output "Cache directory: $($source.cache_dir) -> $cachePath"
Write-Output "Normalized source: $normalizedSource"

if ($DryRun) {
    Write-Output "DRY-RUN: download, verify, extract, normalize, and atomically publish only to the paths above."
    return
}

Remove-StaleExtractionStaging -CachePath $cachePath -CacheParent $cacheParent `
    -MaxAgeSeconds $script:StaleStagingMaxAgeSeconds -MaxRemaining $script:StaleStagingMaxRemaining

if (Test-Path -LiteralPath $normalizedSource -PathType Leaf) {
    Write-Output "Vanilla source is already available: $normalizedSource"
    return
}
if (Test-Path -LiteralPath $cachePath) {
    throw "cache directory exists without resource_pack/blocks.json: $cachePath"
}

New-Item -ItemType Directory -Force -Path $downloadDirectory, $cacheParent | Out-Null

$archiveVerified = $false
if (Test-Path -LiteralPath $archivePath -PathType Leaf) {
    $actual = Get-Sha256Hex -Path $archivePath
    if ($actual -eq $expectedSha256) {
        $archiveVerified = $true
        Write-Output "Using verified archive: $archivePath"
    } else {
        Remove-Item -Force -LiteralPath $archivePath
    }
}

if (-not $archiveVerified) {
    if (Test-Path -LiteralPath $partialPath) {
        Remove-Item -Force -LiteralPath $partialPath
    }
    Write-Output "Downloading $($source.url)"
    Invoke-WebRequest -UseBasicParsing -Uri ([string]$source.url) -OutFile $partialPath
    $actual = Get-Sha256Hex -Path $partialPath
    if ($actual -ne $expectedSha256) {
        Remove-Item -Force -LiteralPath $partialPath
        throw "SHA-256 mismatch: expected $expectedSha256, got $actual"
    }
    Move-Item -LiteralPath $partialPath -Destination $archivePath
    Write-Output "Verified archive SHA-256: $actual"
}

try {
    New-Item -ItemType Directory -Path $temporaryExtract | Out-Null
    Expand-ZipArchiveBounded -ArchivePath $archivePath -DestinationPath $temporaryExtract `
        -MaxArchiveEntries $maxArchiveEntries `
        -MaxExpandedFileBytes $maxExpandedFileBytes `
        -MaxTotalExpandedBytes $maxTotalExpandedBytes `
        -MinRatioSampleCompressedBytes $script:MinRatioSampleCompressedBytes `
        -MaxPerEntryCompressionRatio $maxPerEntryCompressionRatio `
        -MaxAggregateCompressionRatio $maxAggregateCompressionRatio

    # VPA-209: completeness gate before publication. The staged tree must
    # already contain the normalized source; nothing partial is ever moved.
    $directSource = Join-Path $temporaryExtract "resource_pack\blocks.json"
    if (Test-Path -LiteralPath $directSource -PathType Leaf) {
        $normalizedRoot = $temporaryExtract
    } else {
        $topLevel = @(Get-ChildItem -Force -LiteralPath $temporaryExtract)
        if ($topLevel.Count -ne 1 -or -not $topLevel[0].PSIsContainer) {
            throw "archive must contain exactly one top-level directory"
        }
        $normalizedRoot = $topLevel[0].FullName
        $nestedSource = Join-Path $normalizedRoot "resource_pack\blocks.json"
        if (-not (Test-Path -LiteralPath $nestedSource -PathType Leaf)) {
            throw "archive is missing resource_pack/blocks.json"
        }
    }

    # VPA-209: publish by a same-volume atomic rename into an absent target.
    # A failed or interrupted run discards staging above and leaves any
    # previous published tree untouched.
    if (Test-Path -LiteralPath $cachePath) {
        throw "cache directory appeared during extraction: $cachePath"
    }
    Move-Item -LiteralPath $normalizedRoot -Destination $cachePath
    if (Test-Path -LiteralPath $temporaryExtract) {
        Remove-ExtractionTree -Path $temporaryExtract
    }
} catch {
    Remove-ExtractionTree -Path $temporaryExtract
    throw
}

if (-not (Test-Path -LiteralPath $normalizedSource -PathType Leaf)) {
    throw "normalized source was not published: $normalizedSource"
}
Write-Output "Vanilla source ready: $normalizedSource"
