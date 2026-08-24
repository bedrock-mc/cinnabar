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
