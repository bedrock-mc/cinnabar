[CmdletBinding()]
param([switch]$DryRun)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Assert-Basename([string]$Value, [string]$Label) {
    if ([string]::IsNullOrWhiteSpace($Value) -or $Value -in @(".", "..") -or
        $Value.Contains("/") -or $Value.Contains("\")) {
        throw "$Label must be exactly one nonempty basename"
    }
}

function Get-FileSha256Hex([string]$Path) {
    # Hash through .NET instead of Get-FileHash: CI shells can hand this script a
    # PowerShell whose Microsoft.PowerShell.Utility module fails to auto-load, and
    # under Set-StrictMode that unresolved cmdlet aborts the whole fetch.
    $stream = [System.IO.File]::OpenRead([System.IO.Path]::GetFullPath($Path))
    try {
        $algorithm = [System.Security.Cryptography.SHA256]::Create()
        try {
            $digest = $algorithm.ComputeHash($stream)
        } finally {
            $algorithm.Dispose()
        }
    } finally {
        $stream.Dispose()
    }
    return ([System.BitConverter]::ToString($digest) -replace "-", "").ToLowerInvariant()
}

function Get-VerifiedFile([string]$Url, [string]$Path, [long]$Size, [string]$Sha256) {
    $valid = (Test-Path -LiteralPath $Path -PathType Leaf) -and
        (Get-Item -LiteralPath $Path).Length -eq $Size -and
        (Get-FileSha256Hex $Path) -ceq $Sha256
    if ($valid) { return }
    $partial = "$Path.partial-$PID-$([guid]::NewGuid().ToString('N').Substring(0, 8))"
    try {
        Invoke-WebRequest -UseBasicParsing -Uri $Url -OutFile $partial -TimeoutSec 60
        $actualSize = (Get-Item -LiteralPath $partial).Length
        $actualSha = Get-FileSha256Hex $partial
        if ($actualSize -ne $Size -or $actualSha -cne $Sha256) {
            throw "font source size or SHA-256 mismatch for $Url"
        }
        Move-Item -Force -LiteralPath $partial -Destination $Path
    } finally {
        if (Test-Path -LiteralPath $partial) { Remove-Item -Force -LiteralPath $partial }
    }
}

$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot ".."))
$manifestPath = Join-Path $repoRoot "assets\ui-font-source.json"
$manifest = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json
if ([int]$manifest.schema -ne 1 -or [string]$manifest.artifact_policy -cne "local-source-cache") {
    throw "font source manifest policy is invalid"
}
$commit = [string]$manifest.commit
if ($commit -cnotmatch "^[0-9a-f]{40}$") { throw "font source commit is invalid" }
$fontFile = [string]$manifest.font_file
$licenseFile = [string]$manifest.license_file
Assert-Basename $fontFile "font_file"
Assert-Basename $licenseFile "license_file"
$fontUrl = [string]$manifest.font_url
$licenseUrl = [string]$manifest.license_url
foreach ($url in @($fontUrl, $licenseUrl)) {
    if ($url -cnotmatch "^https://raw\.githubusercontent\.com/") { throw "font source URL is not approved HTTPS: $url" }
}
$cache = Join-Path $repoRoot ".local\assets\ui-font\$commit"
$fontPath = Join-Path $cache $fontFile
$licensePath = Join-Path $cache $licenseFile
Write-Output "Manifest: $manifestPath"
Write-Output "Font source: $fontUrl"
Write-Output "License source: $licenseUrl"
Write-Output "Cache: $cache"
if ($DryRun) { return }

New-Item -ItemType Directory -Force -Path $cache | Out-Null
Get-VerifiedFile $fontUrl $fontPath ([long]$manifest.font_size_bytes) ([string]$manifest.font_sha256).ToLowerInvariant()
Get-VerifiedFile $licenseUrl $licensePath ([long]$manifest.license_size_bytes) ([string]$manifest.license_sha256).ToLowerInvariant()
Write-Output "FONT_SOURCE_PATH=$fontPath"
Write-Output "FONT_LICENSE_PATH=$licensePath"
