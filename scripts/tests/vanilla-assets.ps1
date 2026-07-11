Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$repoRoot = [System.IO.Path]::GetFullPath((Join-Path $PSScriptRoot "..\.."))
$fetcher = Join-Path $repoRoot "scripts\fetch-vanilla-assets.ps1"
$manifestPath = Join-Path $repoRoot "assets\vanilla-source.json"

if (-not (Test-Path -LiteralPath $manifestPath -PathType Leaf)) {
    throw "manifest missing: $manifestPath"
}
if (-not (Test-Path -LiteralPath $fetcher -PathType Leaf)) {
    throw "fetcher missing: $fetcher"
}

$source = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json

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
        [string]$Sha256 = ""
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
    $manifest | ConvertTo-Json | Set-Content -LiteralPath $Path -Encoding UTF8
}

function New-TestZipArchive {
    param(
        [Parameter(Mandatory = $true)]
        [string]$Path,
        [Parameter(Mandatory = $true)]
        [object[]]$Entries
    )

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

$downloadDirectory = Join-Path $repoRoot ".local\assets\downloads"
$cacheDirectory = [System.IO.Path]::GetFullPath(
    (Join-Path $repoRoot ([string]$source.cache_dir))
)
$mutationPaths = @($downloadDirectory, $cacheDirectory)
$existedBefore = @{}
foreach ($path in $mutationPaths) {
    $existedBefore[$path] = Test-Path -LiteralPath $path
}

$childPowerShell = [System.Diagnostics.Process]::GetCurrentProcess().MainModule.FileName
$dryOutput = & $childPowerShell -NoProfile -File $fetcher -AcceptEula -DryRun 2>&1 | Out-String
$dryExit = $LASTEXITCODE
if ($dryExit -ne 0) {
    throw "dry-run failed with exit $dryExit`n$dryOutput"
}
foreach ($needle in @(
    [string]$source.url,
    [string]$source.sha256,
    [string]$source.cache_dir
)) {
    if ($dryOutput -notmatch [regex]::Escape($needle)) {
        throw "dry-run output is missing '$needle'"
    }
}

$savedErrorActionPreference = $ErrorActionPreference
$ErrorActionPreference = "Continue"
$gateOutput = & $childPowerShell -NoProfile -File $fetcher -DryRun 2>&1 | Out-String
$gateExit = $LASTEXITCODE
$ErrorActionPreference = $savedErrorActionPreference
if ($gateExit -eq 0) {
    throw "EULA gate unexpectedly succeeded`n$gateOutput"
}

$bashCandidates = @()
if (-not [string]::IsNullOrWhiteSpace($env:ProgramFiles)) {
    $bashCandidates += Join-Path $env:ProgramFiles "Git\bin\bash.exe"
}
$bashCommand = Get-Command bash -ErrorAction SilentlyContinue
if ($null -ne $bashCommand) {
    $bashCandidates += $bashCommand.Source
}
$bash = $bashCandidates |
    Where-Object { -not [string]::IsNullOrWhiteSpace($_) -and (Test-Path -LiteralPath $_ -PathType Leaf) } |
    Select-Object -First 1

if ($null -eq $bash) {
    throw "Bash executable unavailable for vanilla asset contract tests"
}

$sandboxName = "vanilla-assets-test-$([guid]::NewGuid().ToString('N'))"
$sandboxParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
$sandboxRoot = [System.IO.Path]::GetFullPath((Join-Path $sandboxParent $sandboxName))
$sandboxPrefix = $sandboxParent.TrimEnd([System.IO.Path]::DirectorySeparatorChar) +
    [System.IO.Path]::DirectorySeparatorChar
if (-not $sandboxRoot.StartsWith($sandboxPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
    throw "refusing unsafe fetcher test sandbox: $sandboxRoot"
}

try {
    $sandboxScripts = Join-Path $sandboxRoot "scripts"
    $sandboxAssets = Join-Path $sandboxRoot "assets"
    $sandboxManifest = Join-Path $sandboxAssets "vanilla-source.json"
    New-Item -ItemType Directory -Path $sandboxScripts, $sandboxAssets | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot "scripts\fetch-vanilla-assets.sh") `
        -Destination (Join-Path $sandboxScripts "fetch-vanilla-assets.sh")
    Copy-Item -LiteralPath $fetcher -Destination (Join-Path $sandboxScripts "fetch-vanilla-assets.ps1")

    $sandboxBashFetcher = Join-Path $sandboxScripts "fetch-vanilla-assets.sh"
    $sandboxPowerShellFetcher = Join-Path $sandboxScripts "fetch-vanilla-assets.ps1"
    Write-TestManifest -Template $source -Path $sandboxManifest `
        -Archive ([string]$source.archive) -CacheDirectory ([string]$source.cache_dir)

    $validBash = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
        $sandboxBashFetcher,
        "--accept-eula",
        "--dry-run"
    )
    if ($validBash.ExitCode -ne 0 -or $validBash.Output -notmatch "DRY-RUN:") {
        throw "valid Bash dry-run failed`n$($validBash.Output)"
    }

    $sandboxFailures = @()
    $invalidCache = ".local/assets/../../tracked-dir"
    $cacheDiagnostic = "cache_dir must not contain empty or traversal components: $invalidCache"
    Write-TestManifest -Template $source -Path $sandboxManifest `
        -Archive ([string]$source.archive) -CacheDirectory $invalidCache
    $cacheResult = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
        $sandboxBashFetcher,
        "--accept-eula",
        "--dry-run"
    )
    if ($cacheResult.ExitCode -eq 0) {
        $sandboxFailures += "Bash accepted traversing cache_dir"
    }
    if ($cacheResult.Output -notmatch [regex]::Escape($cacheDiagnostic)) {
        $sandboxFailures += "Bash cache_dir failure omitted exact diagnostic '$cacheDiagnostic': $($cacheResult.Output.Trim())"
    }

    $invalidArchives = @(
        "../escaped.zip",
        "..\escaped.zip",
        "nested/archive.zip",
        "nested\archive.zip",
        "/absolute.zip",
        "C:\absolute.zip",
        "C:drive-relative.zip",
        ".",
        "..",
        ""
    )
    foreach ($invalidArchive in $invalidArchives) {
        Write-TestManifest -Template $source -Path $sandboxManifest `
            -Archive $invalidArchive -CacheDirectory ([string]$source.cache_dir)
        $archiveDiagnostic = "archive must be exactly one nonempty basename"

        $bashArchive = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
            $sandboxBashFetcher,
            "--accept-eula",
            "--dry-run"
        )
        if ($bashArchive.ExitCode -eq 0) {
            $sandboxFailures += "Bash accepted invalid archive '$invalidArchive'"
        }
        if ($bashArchive.Output -notmatch [regex]::Escape($archiveDiagnostic)) {
            $sandboxFailures += "Bash archive failure omitted exact diagnostic '$archiveDiagnostic': $($bashArchive.Output.Trim())"
        }

        $powerShellArchive = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            $sandboxPowerShellFetcher,
            "-AcceptEula",
            "-DryRun"
        )
        if ($powerShellArchive.ExitCode -eq 0) {
            $sandboxFailures += "PowerShell accepted invalid archive '$invalidArchive'"
        }
        if ($powerShellArchive.Output -notmatch [regex]::Escape($archiveDiagnostic)) {
            $sandboxFailures += "PowerShell archive failure omitted exact diagnostic '$archiveDiagnostic': $($powerShellArchive.Output.Trim())"
        }
    }

    $syntheticArchiveName = "synthetic-vanilla.zip"
    $syntheticArchivePath = Join-Path $sandboxRoot ".local\assets\downloads\$syntheticArchiveName"
    $syntheticCacheRelative = ".local/assets/synthetic-vanilla"
    $syntheticCache = Join-Path $sandboxRoot ".local\assets\synthetic-vanilla"
    $longMetadataPath = "metadata/json_schemas/server/entity/1.26.30/NearestPrioritizedAttackableTargetGoalDefinition.json"
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "behavior_pack/"; Content = $null },
        [pscustomobject]@{ Name = "behavior_pack/items/"; Content = $null },
        [pscustomobject]@{ Name = "behavior_pack/items/rabbit.json"; Content = "{}" },
        [pscustomobject]@{ Name = "resource_pack/"; Content = $null },
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
        [pscustomobject]@{ Name = $longMetadataPath; Content = "{}" }
    )
    $syntheticSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $syntheticArchivePath).Hash.ToLowerInvariant()
    Write-TestManifest -Template $source -Path $sandboxManifest `
        -Archive $syntheticArchiveName -CacheDirectory $syntheticCacheRelative -Sha256 $syntheticSha256
    $syntheticResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $sandboxPowerShellFetcher,
        "-AcceptEula"
    )
    if ($syntheticResult.ExitCode -ne 0) {
        $sandboxFailures += "PowerShell failed to extract the synthetic pinned-archive layout: $($syntheticResult.Output.Trim())"
    } else {
        foreach ($relativePath in @(
            "behavior_pack\items\rabbit.json",
            "resource_pack\blocks.json",
            $longMetadataPath.Replace("/", "\")
        )) {
            if (-not (Test-Path -LiteralPath (Join-Path $syntheticCache $relativePath) -PathType Leaf)) {
                $sandboxFailures += "PowerShell extraction omitted '$relativePath'"
            }
        }
    }

    if (Test-Path -LiteralPath $syntheticCache) {
        Remove-Item -Recurse -Force -LiteralPath $syntheticCache
    }
    Remove-Item -Force -LiteralPath $syntheticArchivePath
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
        [pscustomobject]@{ Name = "../escaped.txt"; Content = "must not escape" }
    )
    $traversalSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $syntheticArchivePath).Hash.ToLowerInvariant()
    Write-TestManifest -Template $source -Path $sandboxManifest `
        -Archive $syntheticArchiveName -CacheDirectory $syntheticCacheRelative -Sha256 $traversalSha256
    $traversalResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $sandboxPowerShellFetcher,
        "-AcceptEula"
    )
    if ($traversalResult.ExitCode -eq 0) {
        $sandboxFailures += "PowerShell accepted a traversing ZIP entry"
    }
    if ($traversalResult.Output -notmatch [regex]::Escape("unsafe ZIP entry '../escaped.txt'")) {
        $sandboxFailures += "PowerShell traversal failure omitted the bounded-extraction diagnostic: $($traversalResult.Output.Trim())"
    }
    if (Test-Path -LiteralPath $syntheticCache) {
        $sandboxFailures += "PowerShell published a cache after rejecting a traversing ZIP entry"
    }
    $escapedFiles = @(Get-ChildItem -Force -Recurse -LiteralPath $sandboxRoot -Filter "escaped.txt" -ErrorAction SilentlyContinue)
    if ($escapedFiles.Count -ne 0) {
        $sandboxFailures += "PowerShell wrote outside the extraction root: $($escapedFiles.FullName -join ', ')"
    }

    if ($sandboxFailures.Count -ne 0) {
        throw "fetcher safety contract failures:`n$($sandboxFailures -join "`n")"
    }
} finally {
    if (Test-Path -LiteralPath $sandboxRoot) {
        Remove-Item -Recurse -Force -LiteralPath $sandboxRoot
    }
}

$trackedAssets = & git -C $repoRoot ls-files -- ".local/assets/*"
if ($LASTEXITCODE -ne 0) {
    throw "git ls-files failed with exit $LASTEXITCODE"
}
if (@($trackedAssets | Where-Object { $_ -match "\S" }).Count -ne 0) {
    throw "Mojang cache path is tracked: $($trackedAssets -join ', ')"
}

foreach ($path in $mutationPaths) {
    if (-not $existedBefore[$path] -and (Test-Path -LiteralPath $path)) {
        throw "dry-run created local asset path: $path"
    }
}

Write-Output "vanilla asset contract tests passed"
