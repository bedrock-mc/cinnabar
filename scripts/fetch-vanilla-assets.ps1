[CmdletBinding()]
param(
    [switch]$AcceptEula,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

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
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $archivePath).Hash.ToLowerInvariant()
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
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $partialPath).Hash.ToLowerInvariant()
    if ($actual -ne $expectedSha256) {
        Remove-Item -Force -LiteralPath $partialPath
        throw "SHA-256 mismatch: expected $expectedSha256, got $actual"
    }
    Move-Item -LiteralPath $partialPath -Destination $archivePath
    Write-Output "Verified archive SHA-256: $actual"
}

try {
    New-Item -ItemType Directory -Path $temporaryExtract | Out-Null
    Expand-Archive -LiteralPath $archivePath -DestinationPath $temporaryExtract

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
        Remove-Item -Recurse -Force -LiteralPath $temporaryExtract
    }
} catch {
    if (Test-Path -LiteralPath $temporaryExtract) {
        Remove-Item -Recurse -Force -LiteralPath $temporaryExtract
    }
    throw
}

if (-not (Test-Path -LiteralPath $normalizedSource -PathType Leaf)) {
    throw "normalized source was not published: $normalizedSource"
}
Write-Output "Vanilla source ready: $normalizedSource"
