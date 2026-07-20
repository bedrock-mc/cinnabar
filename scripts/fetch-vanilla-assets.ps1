[CmdletBinding()]
param(
    [switch]$AcceptEula,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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

function Get-Sha256Hex {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path
    )

    $stream = [System.IO.File]::OpenRead((ConvertTo-ExtendedLengthPath -Path $Path))
    try {
        $sha256 = [System.Security.Cryptography.SHA256]::Create()
        try {
            return [System.BitConverter]::ToString($sha256.ComputeHash($stream)).Replace("-", "").ToLowerInvariant()
        } finally {
            $sha256.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
}

function Expand-ZipArchiveBounded {
    param(
        [Parameter(Mandatory = $true)]
        [string]$ArchivePath,
        [Parameter(Mandatory = $true)]
        [string]$DestinationPath
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

    $archiveStream = [System.IO.File]::OpenRead($ArchivePath)
    try {
        $zip = [System.IO.Compression.ZipArchive]::new(
            $archiveStream,
            [System.IO.Compression.ZipArchiveMode]::Read,
            $false
        )
        try {
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

                $plannedEntries.Add([pscustomobject]@{
                    Entry = $entry
                    Destination = $entryDestination
                    Directory = $isDirectory
                })
            }

            foreach ($planned in $plannedEntries) {
                $extendedDestination = ConvertTo-ExtendedLengthPath -Path ([string]$planned.Destination)
                if ([bool]$planned.Directory) {
                    [System.IO.Directory]::CreateDirectory($extendedDestination) | Out-Null
                    continue
                }

                $parent = [System.IO.Path]::GetDirectoryName([string]$planned.Destination)
                [System.IO.Directory]::CreateDirectory(
                    (ConvertTo-ExtendedLengthPath -Path $parent)
                ) | Out-Null
                $inputStream = $planned.Entry.Open()
                try {
                    $outputStream = [System.IO.FileStream]::new(
                        $extendedDestination,
                        [System.IO.FileMode]::CreateNew,
                        [System.IO.FileAccess]::Write,
                        [System.IO.FileShare]::None
                    )
                    try {
                        $inputStream.CopyTo($outputStream)
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
    Expand-ZipArchiveBounded -ArchivePath $archivePath -DestinationPath $temporaryExtract

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
