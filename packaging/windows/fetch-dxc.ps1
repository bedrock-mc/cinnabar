# Places the DX12 shader compiler pinned in dxc.json, beside this script, into a directory: the
# installer payload, or a development build's output beside cinnabar.exe. The release archive is
# cached under <Root>/.local/downloads; the archive and the extracted library must both match their
# pinned SHA-256 before the library is published.
# Usage: fetch-dxc.ps1 -Out <dir> [-Root <checkout>]
param([Parameter(Mandatory)][string]$Out, [string]$Root = "")
$ErrorActionPreference = "Stop"
# Windows PowerShell leaves $PSScriptRoot empty while binding parameter defaults.
if (-not $Root) { $Root = Join-Path $PSScriptRoot "../.." }
$ProgressPreference = "SilentlyContinue"
$pin = Get-Content -Raw -LiteralPath (Join-Path $PSScriptRoot "dxc.json") | ConvertFrom-Json

# Whether the file exists with this lowercase SHA-256; uses .NET, which both PowerShell editions share.
function Test-Sha256([string]$Path, [string]$Expected) {
    if (-not (Test-Path -LiteralPath $Path)) { return $false }
    $sha = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { $digest = -join ($sha.ComputeHash($stream) | ForEach-Object { $_.ToString('x2') }) }
    finally { $stream.Dispose(); $sha.Dispose() }
    return $digest -eq $Expected
}

New-Item -ItemType Directory -Force $Out | Out-Null
$library = Join-Path (Resolve-Path -LiteralPath $Out).Path $pin.library
if (Test-Sha256 $library $pin.library_sha256) { return }

$cache = Join-Path (Resolve-Path -LiteralPath $Root).Path ".local/downloads"
New-Item -ItemType Directory -Force $cache | Out-Null
$archive = Join-Path $cache ([IO.Path]::GetFileName(([Uri]$pin.archive_url).AbsolutePath))
if (-not (Test-Sha256 $archive $pin.archive_sha256)) {
    $download = "$archive.partial"
    Invoke-WebRequest -UseBasicParsing -Uri $pin.archive_url -OutFile $download
    if (-not (Test-Sha256 $download $pin.archive_sha256)) {
        Remove-Item -LiteralPath $download -Force
        throw "$($pin.archive_url) does not match its pinned SHA-256"
    }
    Move-Item -LiteralPath $download -Destination $archive -Force
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
$member = "$($pin.archive_directory)/$($pin.library)"
$extracted = "$library.partial"
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    $entry = $zip.Entries | Where-Object { $_.FullName.Replace('\', '/') -eq $member } | Select-Object -First 1
    if (-not $entry) { throw "$archive holds no $member" }
    [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $extracted, $true)
} finally {
    $zip.Dispose()
}
if (-not (Test-Sha256 $extracted $pin.library_sha256)) {
    Remove-Item -LiteralPath $extracted -Force
    throw "$member in $archive does not match its pinned SHA-256"
}
Move-Item -LiteralPath $extracted -Destination $library -Force
