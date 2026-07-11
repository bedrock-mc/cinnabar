[CmdletBinding()]
param(
    [string]$ManifestPath,
    [string]$DestinationRoot
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-LowerSha256 {
    param([Parameter(Mandatory = $true)][string]$Path)
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Resolve-ContainedRelativePath {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$RelativePath,
        [Parameter(Mandatory = $true)][string]$Label
    )

    if ([string]::IsNullOrWhiteSpace($RelativePath) -or
        [System.IO.Path]::IsPathRooted($RelativePath) -or
        $RelativePath -match "^[A-Za-z]:") {
        throw "$Label must be a nonempty relative path: $RelativePath"
    }

    $normalized = $RelativePath.Replace("\", "/")
    $parts = @($normalized.Split([char]47))
    if ($parts.Count -eq 0) {
        throw "$Label must be a nonempty relative path: $RelativePath"
    }
    foreach ($part in $parts) {
        if ([string]::IsNullOrWhiteSpace($part) -or
            $part -eq "." -or
            $part -eq ".." -or
            $part.Contains(":")) {
            throw "$Label contains an unsafe path component: $RelativePath"
        }
    }

    $rootFull = [System.IO.Path]::GetFullPath($Root)
    $candidate = [System.IO.Path]::GetFullPath((Join-Path $rootFull ($parts -join [System.IO.Path]::DirectorySeparatorChar)))
    $prefix = $rootFull.TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    $comparison = if ([System.IO.Path]::DirectorySeparatorChar -eq [char]92) {
        [System.StringComparison]::OrdinalIgnoreCase
    } else {
        [System.StringComparison]::Ordinal
    }
    if (-not $candidate.StartsWith($prefix, $comparison)) {
        throw "$Label escapes its root: $RelativePath"
    }
    return $candidate
}

function Assert-NoReparsePointPath {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Candidate,
        [Parameter(Mandatory = $true)][string]$Label
    )

    if ([System.IO.Path]::DirectorySeparatorChar -ne [char]92) {
        return
    }
    $rootFull = [System.IO.Path]::GetFullPath($Root).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $candidateFull = [System.IO.Path]::GetFullPath($Candidate).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    )
    $prefix = $rootFull + [System.IO.Path]::DirectorySeparatorChar
    if (-not [string]::Equals($candidateFull, $rootFull, [System.StringComparison]::OrdinalIgnoreCase) -and
        -not $candidateFull.StartsWith($prefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "$Label is outside its guarded root: $candidateFull"
    }

    $current = $candidateFull
    while ($true) {
        $item = Get-Item -Force -LiteralPath $current -ErrorAction SilentlyContinue
        if ($null -ne $item -and
            (($item.Attributes -band [System.IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "$Label contains a reparse point: $current"
        }
        $parent = [System.IO.DirectoryInfo]::new($current).Parent
        if ($null -eq $parent) {
            break
        }
        $current = $parent.FullName
    }
}

function Remove-TemporaryTree {
    param(
        [Parameter(Mandatory = $true)][string]$Root,
        [Parameter(Mandatory = $true)][string]$Path
    )
    if (-not (Test-Path -LiteralPath $Path)) {
        return
    }
    Assert-NoReparsePointPath -Root $Root -Candidate $Path -Label "temporary tree cleanup"
    [System.IO.Directory]::Delete($Path, $true)
}

function Assert-ExpectedHash {
    param(
        [Parameter(Mandatory = $true)][string]$Expected,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($Expected -cnotmatch "^[0-9a-f]{64}$") {
        throw "$Label must contain one lowercase SHA-256 value"
    }
}

function ConvertTo-ExactInt64 {
    param(
        [Parameter(Mandatory = $true)][object]$Value,
        [Parameter(Mandatory = $true)][string]$Label
    )
    if ($Value -is [string] -or $Value -is [bool]) {
        throw "$Label must be an integer"
    }
    try {
        $decimalValue = [decimal]$Value
        $integerValue = [long]$Value
    } catch {
        throw "$Label must be a signed 64-bit integer"
    }
    if ($decimalValue -ne [decimal]$integerValue) {
        throw "$Label must be an integer"
    }
    return $integerValue
}

function Get-RemainingDownloadMilliseconds {
    param(
        [Parameter(Mandatory = $true)][System.Diagnostics.Stopwatch]$Deadline,
        [Parameter(Mandatory = $true)][int]$TimeoutMilliseconds,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $remaining = [long]$TimeoutMilliseconds - $Deadline.ElapsedMilliseconds
    if ($remaining -le 0) {
        throw "$Label overall download deadline exceeded after $($Deadline.ElapsedMilliseconds) ms"
    }
    return [int][Math]::Min($remaining, [int]::MaxValue)
}

function Copy-StreamBounded {
    param(
        [Parameter(Mandatory = $true)][System.IO.Stream]$InputStream,
        [Parameter(Mandatory = $true)][string]$PartialPath,
        [Parameter(Mandatory = $true)][long]$ExpectedBytes,
        [Parameter(Mandatory = $true)][long]$MaximumBytes,
        [Parameter(Mandatory = $true)][int]$BufferBytes,
        [Parameter(Mandatory = $true)][System.Diagnostics.Stopwatch]$Deadline,
        [Parameter(Mandatory = $true)][int]$TimeoutMilliseconds,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $output = [System.IO.FileStream]::new(
        $PartialPath,
        [System.IO.FileMode]::CreateNew,
        [System.IO.FileAccess]::Write,
        [System.IO.FileShare]::None,
        $BufferBytes,
        [System.IO.FileOptions]::WriteThrough
    )
    try {
        $buffer = New-Object byte[] $BufferBytes
        $total = 0L
        while ($true) {
            $remaining = Get-RemainingDownloadMilliseconds -Deadline $Deadline `
                -TimeoutMilliseconds $TimeoutMilliseconds -Label $Label
            if ($InputStream.CanTimeout) {
                $InputStream.ReadTimeout = [Math]::Max(1, $remaining)
            }
            try {
                $read = $InputStream.Read($buffer, 0, $buffer.Length)
            } catch [System.IO.IOException] {
                if ($Deadline.ElapsedMilliseconds -ge $TimeoutMilliseconds) {
                    throw "$Label overall download deadline exceeded after $($Deadline.ElapsedMilliseconds) ms"
                }
                throw
            }
            $null = Get-RemainingDownloadMilliseconds -Deadline $Deadline `
                -TimeoutMilliseconds $TimeoutMilliseconds -Label $Label
            if ($read -le 0) {
                break
            }
            if ($total -gt ([long]::MaxValue - [long]$read)) {
                throw "$Label byte count overflow"
            }
            $total += [long]$read
            if ($total -gt $ExpectedBytes -or $total -gt $MaximumBytes) {
                throw "$Label byte ceiling exceeded: expected $ExpectedBytes bytes, received more than $total"
            }
            $output.Write($buffer, 0, $read)
        }
        $output.Flush()
    } finally {
        $output.Dispose()
    }
    if ($total -ne $ExpectedBytes) {
        throw "$Label size mismatch: expected $ExpectedBytes bytes, got $total"
    }
}

function Copy-SourceToPartial {
    param(
        [Parameter(Mandatory = $true)][string]$Url,
        [Parameter(Mandatory = $true)][string]$PartialPath,
        [Parameter(Mandatory = $true)][long]$ExpectedBytes,
        [Parameter(Mandatory = $true)][long]$MaximumBytes,
        [Parameter(Mandatory = $true)][int]$BufferBytes,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds,
        [Parameter(Mandatory = $true)][string]$Label
    )

    try {
        $uri = [System.Uri]$Url
    } catch {
        throw "source URL is invalid: $Url"
    }
    if (-not $uri.IsAbsoluteUri) {
        throw "source URL must be absolute: $Url"
    }
    # One stopwatch spans response establishment and every response-body read;
    # per-read timeouts are reduced to the remaining overall deadline.
    $timeoutMilliseconds = $TimeoutSeconds * 1000
    $deadline = [System.Diagnostics.Stopwatch]::StartNew()

    if ($uri.Scheme -eq [System.Uri]::UriSchemeFile) {
        if (-not (Test-Path -LiteralPath $uri.LocalPath -PathType Leaf)) {
            throw "local source URL does not exist: $Url"
        }
        $input = [System.IO.File]::OpenRead($uri.LocalPath)
        try {
            if ($input.Length -gt $ExpectedBytes -or $input.Length -gt $MaximumBytes) {
                throw "$Label byte ceiling exceeded by local source length: expected $ExpectedBytes bytes, got $($input.Length)"
            }
            if ($input.Length -ne $ExpectedBytes) {
                throw "$Label size mismatch: expected $ExpectedBytes bytes, got $($input.Length)"
            }
            Copy-StreamBounded -InputStream $input -PartialPath $PartialPath `
                -ExpectedBytes $ExpectedBytes -MaximumBytes $MaximumBytes `
                -BufferBytes $BufferBytes -Deadline $deadline `
                -TimeoutMilliseconds $timeoutMilliseconds -Label $Label
        } finally {
            $input.Dispose()
        }
        return
    }
    if ($uri.Scheme -ne [System.Uri]::UriSchemeHttps) {
        throw "source URL must use HTTPS or file: $Url"
    }

    $request = [System.Net.HttpWebRequest]::Create($uri)
    $request.Method = "GET"
    $request.AllowAutoRedirect = $true
    $request.MaximumAutomaticRedirections = 5
    $request.Timeout = $timeoutMilliseconds
    $request.ReadWriteTimeout = $timeoutMilliseconds
    $request.UserAgent = "rust-mcbe-block-data-acquirer/1"
    $response = $null
    try {
        $response = [System.Net.HttpWebResponse]$request.GetResponse()
        $null = Get-RemainingDownloadMilliseconds -Deadline $deadline `
            -TimeoutMilliseconds $timeoutMilliseconds -Label $Label
        $contentLength = [long]$response.ContentLength
        if ($contentLength -ge 0) {
            if ($contentLength -gt $ExpectedBytes -or $contentLength -gt $MaximumBytes) {
                throw "$Label ContentLength byte ceiling exceeded: expected $ExpectedBytes bytes, got $contentLength"
            }
            if ($contentLength -ne $ExpectedBytes) {
                throw "$Label ContentLength size mismatch: expected $ExpectedBytes bytes, got $contentLength"
            }
        }
        $input = $response.GetResponseStream()
        try {
            Copy-StreamBounded -InputStream $input -PartialPath $PartialPath `
                -ExpectedBytes $ExpectedBytes -MaximumBytes $MaximumBytes `
                -BufferBytes $BufferBytes -Deadline $deadline `
                -TimeoutMilliseconds $timeoutMilliseconds -Label $Label
        } finally {
            $input.Dispose()
        }
    } finally {
        if ($null -ne $response) {
            $response.Dispose()
        }
    }
}

function Get-VerifiedCachedFile {
    param(
        [Parameter(Mandatory = $true)][psobject]$Source,
        [Parameter(Mandatory = $true)][psobject]$File,
        [Parameter(Mandatory = $true)][int]$Index,
        [Parameter(Mandatory = $true)][string]$CacheRoot,
        [Parameter(Mandatory = $true)][psobject]$Limits
    )

    $sourceId = [string]$Source.id
    $installPath = [string]$File.install_path
    $expectedHash = ([string]$File.sha256).ToLowerInvariant()
    $expectedBytes = [long]$File.size_bytes
    $maximumBytes = [long]$Limits.max_file_bytes
    Assert-ExpectedHash -Expected $expectedHash -Label "source '$sourceId' file '$installPath' sha256"

    $sourceCache = Resolve-ContainedRelativePath -Root $CacheRoot -RelativePath $sourceId -Label "source cache id"
    Assert-NoReparsePointPath -Root $CacheRoot -Candidate $sourceCache -Label "source cache"
    New-Item -ItemType Directory -Force -Path $sourceCache | Out-Null
    Assert-NoReparsePointPath -Root $CacheRoot -Candidate $sourceCache -Label "source cache"
    $basename = [System.IO.Path]::GetFileName($installPath.Replace("/", [System.IO.Path]::DirectorySeparatorChar))
    $cacheName = "{0:D2}-{1}" -f $Index, $basename
    $cachePath = Resolve-ContainedRelativePath -Root $sourceCache -RelativePath $cacheName -Label "cache filename"
    Assert-NoReparsePointPath -Root $CacheRoot -Candidate $cachePath -Label "cached source file"

    if (Test-Path -LiteralPath $cachePath -PathType Leaf) {
        $cachedBytes = (Get-Item -Force -LiteralPath $cachePath).Length
        if ($cachedBytes -gt $expectedBytes -or $cachedBytes -gt $maximumBytes) {
            throw "cached byte ceiling exceeded for '$sourceId/$installPath': expected $expectedBytes bytes, got $cachedBytes"
        }
        if ($cachedBytes -ne $expectedBytes) {
            throw "cached size mismatch for '$sourceId/$installPath': expected $expectedBytes bytes, got $cachedBytes"
        }
        $cachedHash = Get-LowerSha256 -Path $cachePath
        if ($cachedHash -ne $expectedHash) {
            throw "cached SHA-256 mismatch for '$sourceId/$installPath': expected $expectedHash, got $cachedHash"
        }
        return $cachePath
    }
    if (Test-Path -LiteralPath $cachePath) {
        throw "cache path exists but is not a regular file: $cachePath"
    }

    $partialPath = "$cachePath.partial-$PID-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    Assert-NoReparsePointPath -Root $CacheRoot -Candidate $partialPath -Label "partial download"
    try {
        Copy-SourceToPartial -Url ([string]$File.url) -PartialPath $partialPath `
            -ExpectedBytes $expectedBytes -MaximumBytes $maximumBytes `
            -BufferBytes ([int]$Limits.download_buffer_bytes) `
            -TimeoutSeconds ([int]$Limits.request_timeout_seconds) `
            -Label "source '$sourceId/$installPath'"
        $actualHash = Get-LowerSha256 -Path $partialPath
        if ($actualHash -ne $expectedHash) {
            throw "SHA-256 mismatch for '$sourceId/$installPath': expected $expectedHash, got $actualHash"
        }
        Assert-NoReparsePointPath -Root $CacheRoot -Candidate $cachePath -Label "cached source file"
        Move-Item -LiteralPath $partialPath -Destination $cachePath
    } finally {
        if (Test-Path -LiteralPath $partialPath) {
            Assert-NoReparsePointPath -Root $CacheRoot -Candidate $partialPath -Label "partial download cleanup"
            Remove-Item -Force -LiteralPath $partialPath
        }
    }
    return $cachePath
}

function Assert-PmmpProtocolMetadata {
    param(
        [Parameter(Mandatory = $true)][string]$SourceRoot,
        [Parameter(Mandatory = $true)][psobject]$Protocol
    )

    $protocolInfoPath = Join-Path $SourceRoot "protocol_info.json"
    if (-not (Test-Path -LiteralPath $protocolInfoPath -PathType Leaf)) {
        throw "PMMP source is missing protocol_info.json: $SourceRoot"
    }
    try {
        $info = Get-Content -Raw -LiteralPath $protocolInfoPath | ConvertFrom-Json
        $actualVersion = "$([int]$info.version.major).$([int]$info.version.minor).$([int]$info.version.patch)"
        $actualProtocol = [int]$info.version.protocol_version
    } catch {
        throw "PMMP protocol metadata is malformed: $($_.Exception.Message)"
    }

    $expectedVersion = [string]$Protocol.game_version
    if ($actualVersion -cne $expectedVersion) {
        throw "game version metadata mismatch: expected $expectedVersion, got $actualVersion"
    }
    $expectedProtocol = [int]$Protocol.protocol_version
    if ($actualProtocol -ne $expectedProtocol) {
        throw "protocol metadata mismatch: expected $expectedProtocol, got $actualProtocol"
    }
}

function Assert-InstalledSource {
    param(
        [Parameter(Mandatory = $true)][psobject]$Source,
        [Parameter(Mandatory = $true)][string]$BundleRoot,
        [Parameter(Mandatory = $true)][string]$SourceRoot,
        [Parameter(Mandatory = $true)][psobject]$Protocol,
        [Parameter(Mandatory = $true)][psobject]$Limits
    )

    Assert-NoReparsePointPath -Root $BundleRoot -Candidate $SourceRoot -Label "installed source"
    foreach ($file in @($Source.files)) {
        $installPath = [string]$file.install_path
        $installedPath = Resolve-ContainedRelativePath -Root $SourceRoot -RelativePath $installPath -Label "installed file"
        Assert-NoReparsePointPath -Root $BundleRoot -Candidate $installedPath -Label "installed file"
        if (-not (Test-Path -LiteralPath $installedPath -PathType Leaf)) {
            throw "installed file is missing for '$($Source.id)/$installPath': $installedPath"
        }
        $expectedHash = ([string]$file.sha256).ToLowerInvariant()
        $expectedBytes = [long]$file.size_bytes
        Assert-ExpectedHash -Expected $expectedHash -Label "source '$($Source.id)' file '$installPath' sha256"
        $actualBytes = (Get-Item -Force -LiteralPath $installedPath).Length
        if ($actualBytes -gt $expectedBytes -or $actualBytes -gt [long]$Limits.max_file_bytes) {
            throw "installed byte ceiling exceeded for '$($Source.id)/$installPath': expected $expectedBytes bytes, got $actualBytes"
        }
        if ($actualBytes -ne $expectedBytes) {
            throw "installed size mismatch for '$($Source.id)/$installPath': expected $expectedBytes bytes, got $actualBytes"
        }
        $actualHash = Get-LowerSha256 -Path $installedPath
        if ($actualHash -ne $expectedHash) {
            throw "installed SHA-256 mismatch for '$($Source.id)/$installPath': expected $expectedHash, got $actualHash"
        }
    }
    if ([string]$Source.id -eq "pmmp-bedrock-data") {
        Assert-PmmpProtocolMetadata -SourceRoot $SourceRoot -Protocol $Protocol
    }
}

function Assert-InstalledBundle {
    param(
        [Parameter(Mandatory = $true)][psobject]$Manifest,
        [Parameter(Mandatory = $true)][string]$BundleRoot
    )

    Assert-NoReparsePointPath -Root $BundleRoot -Candidate $BundleRoot -Label "installed bundle"
    $expectedEntries = @{}
    foreach ($source in @($Manifest.sources)) {
        $sourceRelative = ([string]$source.destination).Replace("\", "/").Trim([char]47)
        $current = ""
        foreach ($component in @($sourceRelative.Split([char]47))) {
            $current = if ([string]::IsNullOrEmpty($current)) { $component } else { "$current/$component" }
            $expectedEntries[$current] = "directory"
        }
        foreach ($file in @($source.files)) {
            $fileRelative = ([string]$file.install_path).Replace("\", "/").Trim([char]47)
            $fileComponents = @($fileRelative.Split([char]47))
            $current = $sourceRelative
            for ($index = 0; $index -lt $fileComponents.Count; $index++) {
                $current = "$current/$($fileComponents[$index])"
                $expectedEntries[$current] = if ($index -eq ($fileComponents.Count - 1)) {
                    "file"
                } else {
                    "directory"
                }
            }
        }
    }

    $bundlePrefix = [System.IO.Path]::GetFullPath($BundleRoot).TrimEnd(
        [System.IO.Path]::DirectorySeparatorChar,
        [System.IO.Path]::AltDirectorySeparatorChar
    ) + [System.IO.Path]::DirectorySeparatorChar
    $pendingDirectories = [System.Collections.Generic.Stack[string]]::new()
    $pendingDirectories.Push($BundleRoot)
    $actualEntryCount = 0
    while ($pendingDirectories.Count -gt 0) {
        $directory = $pendingDirectories.Pop()
        foreach ($entry in @(Get-ChildItem -Force -LiteralPath $directory)) {
            Assert-NoReparsePointPath -Root $BundleRoot -Candidate $entry.FullName -Label "installed bundle entry"
            $relative = $entry.FullName.Substring($bundlePrefix.Length).Replace("\", "/")
            $kind = if ($entry.PSIsContainer) { "directory" } else { "file" }
            if (-not $expectedEntries.ContainsKey($relative)) {
                throw "installed bundle contains unexpected entry: $relative"
            }
            if ([string]$expectedEntries[$relative] -cne $kind) {
                throw "installed bundle entry kind mismatch for '$relative': expected $($expectedEntries[$relative]), got $kind"
            }
            $actualEntryCount++
            if ($entry.PSIsContainer) {
                $pendingDirectories.Push($entry.FullName)
            }
        }
    }
    if ($actualEntryCount -ne $expectedEntries.Count) {
        throw "installed bundle entry count mismatch: expected $($expectedEntries.Count), got $actualEntryCount"
    }

    foreach ($source in @($Manifest.sources)) {
        $sourceRoot = Resolve-ContainedRelativePath -Root $BundleRoot -RelativePath ([string]$source.destination) -Label "installed source destination"
        Assert-NoReparsePointPath -Root $BundleRoot -Candidate $sourceRoot -Label "installed source"
        if (-not (Test-Path -LiteralPath $sourceRoot -PathType Container)) {
            throw "installed source is missing: $sourceRoot"
        }
        Assert-InstalledSource -Source $source -BundleRoot $BundleRoot -SourceRoot $sourceRoot `
            -Protocol $Manifest.protocol -Limits $Manifest.limits
    }
}

function Write-ResolvedSourcePaths {
    param(
        [Parameter(Mandatory = $true)][psobject]$Manifest,
        [Parameter(Mandatory = $true)][string]$BundleRoot,
        [switch]$AlreadyVerified
    )
    foreach ($source in @($Manifest.sources)) {
        $sourceRoot = Resolve-ContainedRelativePath -Root $BundleRoot -RelativePath ([string]$source.destination) -Label "resolved source destination"
        if ($AlreadyVerified) {
            Write-Output "$($source.id) already verified: $sourceRoot"
        }
        Write-Output "SOURCE_PATH $($source.id)=$sourceRoot"
    }
}

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
if ([string]::IsNullOrWhiteSpace($ManifestPath)) {
    $ManifestPath = Join-Path $repoRoot "assets\block-data-sources.json"
}
$ManifestPath = [System.IO.Path]::GetFullPath($ManifestPath)
if ([string]::IsNullOrWhiteSpace($DestinationRoot)) {
    $DestinationRoot = Join-Path $repoRoot ".local\assets\block-data"
}
$DestinationRoot = [System.IO.Path]::GetFullPath($DestinationRoot)

if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf)) {
    throw "source manifest is missing: $ManifestPath"
}
$concurrentWinner = $false
try {
    $manifest = Get-Content -Raw -LiteralPath $ManifestPath | ConvertFrom-Json
} catch {
    throw "source manifest is malformed: $($_.Exception.Message)"
}
if ([int]$manifest.schema -ne 1) {
    throw "unsupported source manifest schema: $($manifest.schema)"
}
if ([string]$manifest.artifact_policy -cne "local-only") {
    throw "source manifest must declare artifact_policy 'local-only'"
}
$expectedVersion = [string]$manifest.protocol.game_version
if ($expectedVersion -cnotmatch "^\d+\.\d+\.\d+$") {
    throw "source manifest game_version is invalid: $expectedVersion"
}
$expectedProtocol = [int]$manifest.protocol.protocol_version
if ($expectedProtocol -le 0) {
    throw "source manifest protocol_version must be positive"
}

$maxSources = ConvertTo-ExactInt64 -Value $manifest.limits.max_sources -Label "limits.max_sources"
$maxFilesPerSource = ConvertTo-ExactInt64 -Value $manifest.limits.max_files_per_source -Label "limits.max_files_per_source"
$maxFileBytes = ConvertTo-ExactInt64 -Value $manifest.limits.max_file_bytes -Label "limits.max_file_bytes"
$maxTotalBytes = ConvertTo-ExactInt64 -Value $manifest.limits.max_total_bytes -Label "limits.max_total_bytes"
$downloadBufferBytes = ConvertTo-ExactInt64 -Value $manifest.limits.download_buffer_bytes -Label "limits.download_buffer_bytes"
$requestTimeoutSeconds = ConvertTo-ExactInt64 -Value $manifest.limits.request_timeout_seconds -Label "limits.request_timeout_seconds"
if ($maxSources -lt 1 -or $maxSources -gt 64) {
    throw "limits.max_sources must be between 1 and 64"
}
if ($maxFilesPerSource -lt 1 -or $maxFilesPerSource -gt 1024) {
    throw "limits.max_files_per_source must be between 1 and 1024"
}
if ($maxFileBytes -lt 1 -or $maxFileBytes -gt 1073741824L) {
    throw "limits.max_file_bytes must be between 1 and 1073741824"
}
if ($maxTotalBytes -lt $maxFileBytes -or $maxTotalBytes -gt 4294967296L) {
    throw "limits.max_total_bytes must be at least max_file_bytes and at most 4294967296"
}
if ($downloadBufferBytes -lt 4096 -or $downloadBufferBytes -gt 1048576) {
    throw "limits.download_buffer_bytes must be between 4096 and 1048576"
}
if ($requestTimeoutSeconds -lt 1 -or $requestTimeoutSeconds -gt 300) {
    throw "limits.request_timeout_seconds must be between 1 and 300"
}

$sources = @($manifest.sources)
if ($sources.Count -eq 0 -or $sources.Count -gt $maxSources) {
    throw "source manifest must contain between 1 and $maxSources sources"
}
$seenIds = @{}
$seenDestinations = @{}
$totalBytes = 0L
foreach ($source in $sources) {
    $sourceId = [string]$source.id
    if ($sourceId -cnotmatch "^[a-z0-9][a-z0-9-]{0,63}$") {
        throw "source id is invalid: $sourceId"
    }
    if ($seenIds.ContainsKey($sourceId)) {
        throw "duplicate source id: $sourceId"
    }
    $seenIds[$sourceId] = $true

    $destination = [string]$source.destination
    $destinationPath = Resolve-ContainedRelativePath -Root $DestinationRoot -RelativePath $destination -Label "source '$sourceId' destination"
    $destinationIdentity = if ([System.IO.Path]::DirectorySeparatorChar -eq [char]92) {
        $destinationPath.ToLowerInvariant()
    } else {
        $destinationPath
    }
    if ($seenDestinations.ContainsKey($destinationIdentity)) {
        throw "duplicate source destination: $destination"
    }
    $seenDestinations[$destinationIdentity] = $true

    $files = @($source.files)
    if ($files.Count -eq 0 -or $files.Count -gt $maxFilesPerSource) {
        throw "source '$sourceId' must contain between 1 and $maxFilesPerSource files"
    }
    $seenFiles = @{}
    foreach ($file in $files) {
        $installPath = [string]$file.install_path
        $resolved = Resolve-ContainedRelativePath -Root $destinationPath -RelativePath $installPath -Label "source '$sourceId' install_path"
        $identity = if ([System.IO.Path]::DirectorySeparatorChar -eq [char]92) {
            $resolved.ToLowerInvariant()
        } else {
            $resolved
        }
        if ($seenFiles.ContainsKey($identity)) {
            throw "source '$sourceId' contains duplicate install_path '$installPath'"
        }
        $seenFiles[$identity] = $true
        Assert-ExpectedHash -Expected ([string]$file.sha256) -Label "source '$sourceId' file '$installPath' sha256"
        if ([string]::IsNullOrWhiteSpace([string]$file.url)) {
            throw "source '$sourceId' file '$installPath' is missing url"
        }
        $sizeBytes = ConvertTo-ExactInt64 -Value $file.size_bytes -Label "source '$sourceId' file '$installPath' size_bytes"
        if ($sizeBytes -le 0) {
            throw "source '$sourceId' file '$installPath' size_bytes must be positive"
        }
        if ($sizeBytes -gt $maxFileBytes) {
            throw "source '$sourceId' file '$installPath' size_bytes exceeds max_file_bytes"
        }
        if ($totalBytes -gt ($maxTotalBytes - $sizeBytes)) {
            throw "source bundle size_bytes exceeds max_total_bytes"
        }
        $totalBytes += $sizeBytes
    }
}

$destinationParent = Split-Path -Parent $DestinationRoot
Assert-NoReparsePointPath -Root $DestinationRoot -Candidate $DestinationRoot -Label "destination bundle"
New-Item -ItemType Directory -Force -Path $destinationParent | Out-Null
Assert-NoReparsePointPath -Root $DestinationRoot -Candidate $DestinationRoot -Label "destination bundle"
if (Test-Path -LiteralPath $DestinationRoot -PathType Leaf) {
    throw "destination root exists as a file: $DestinationRoot"
}

Write-Output "Manifest: $ManifestPath"
Write-Output "Protocol: Bedrock $expectedVersion / $expectedProtocol"
Write-Output "Destination root: $DestinationRoot"

if (Test-Path -LiteralPath $DestinationRoot -PathType Container) {
    Assert-InstalledBundle -Manifest $manifest -BundleRoot $DestinationRoot
    Write-ResolvedSourcePaths -Manifest $manifest -BundleRoot $DestinationRoot -AlreadyVerified
    return
}

$cacheRoot = "$DestinationRoot.downloads"
$temporaryBundle = "$DestinationRoot.installing-$PID-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
Assert-NoReparsePointPath -Root $cacheRoot -Candidate $cacheRoot -Label "download cache"
if (Test-Path -LiteralPath $cacheRoot -PathType Leaf) {
    throw "download cache exists as a file: $cacheRoot"
}
New-Item -ItemType Directory -Force -Path $cacheRoot | Out-Null
Assert-NoReparsePointPath -Root $cacheRoot -Candidate $cacheRoot -Label "download cache"
Assert-NoReparsePointPath -Root $temporaryBundle -Candidate $temporaryBundle -Label "temporary bundle"
if (Test-Path -LiteralPath $temporaryBundle) {
    throw "temporary bundle path already exists: $temporaryBundle"
}

try {
    New-Item -ItemType Directory -Path $temporaryBundle | Out-Null
    Assert-NoReparsePointPath -Root $temporaryBundle -Candidate $temporaryBundle -Label "temporary bundle"
    foreach ($source in $sources) {
        $sourceRoot = Resolve-ContainedRelativePath -Root $temporaryBundle -RelativePath ([string]$source.destination) -Label "staged source destination"
        Assert-NoReparsePointPath -Root $temporaryBundle -Candidate $sourceRoot -Label "staged source"
        New-Item -ItemType Directory -Force -Path $sourceRoot | Out-Null
        $fileIndex = 0
        foreach ($file in @($source.files)) {
            $cachedPath = Get-VerifiedCachedFile -Source $source -File $file -Index $fileIndex `
                -CacheRoot $cacheRoot -Limits $manifest.limits
            $installedPath = Resolve-ContainedRelativePath -Root $sourceRoot -RelativePath ([string]$file.install_path) -Label "staged install path"
            Assert-NoReparsePointPath -Root $temporaryBundle -Candidate $installedPath -Label "staged install file"
            New-Item -ItemType Directory -Force -Path (Split-Path -Parent $installedPath) | Out-Null
            Copy-Item -LiteralPath $cachedPath -Destination $installedPath
            $fileIndex++
        }
        Assert-InstalledSource -Source $source -BundleRoot $temporaryBundle -SourceRoot $sourceRoot `
            -Protocol $manifest.protocol -Limits $manifest.limits
    }
    Assert-InstalledBundle -Manifest $manifest -BundleRoot $temporaryBundle
    Assert-NoReparsePointPath -Root $temporaryBundle -Candidate $temporaryBundle -Label "staged bundle"
    Assert-NoReparsePointPath -Root $DestinationRoot -Candidate $DestinationRoot -Label "destination bundle"
    try {
        # Directory.Move is a same-volume atomic rename whose destination must
        # not exist. Unlike Move-Item, it can never nest this staging directory
        # inside a concurrently published destination directory.
        [System.IO.Directory]::Move($temporaryBundle, $DestinationRoot)
    } catch {
        if (-not (Test-Path -LiteralPath $DestinationRoot -PathType Container)) {
            throw
        }
        try {
            Assert-InstalledBundle -Manifest $manifest -BundleRoot $DestinationRoot
        } catch {
            throw "concurrent publisher created an invalid destination bundle: $($_.Exception.Message)"
        }
        $concurrentWinner = $true
        Remove-TemporaryTree -Root $temporaryBundle -Path $temporaryBundle
    }
} catch {
    Remove-TemporaryTree -Root $temporaryBundle -Path $temporaryBundle
    throw
}

Assert-InstalledBundle -Manifest $manifest -BundleRoot $DestinationRoot
if ($concurrentWinner) {
    Write-Output "Concurrent publisher won with a verified bundle: $DestinationRoot"
}
Write-ResolvedSourcePaths -Manifest $manifest -BundleRoot $DestinationRoot
