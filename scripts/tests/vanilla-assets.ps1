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
$bashFetcherSource = Get-Content -Raw -LiteralPath (Join-Path $repoRoot "scripts\fetch-vanilla-assets.sh")
if ($bashFetcherSource -match '(?m)^\s*declare\s+-[^\r\n]*l') { throw 'Bash fetcher uses the Bash-4-only lowercase variable attribute' }
if (-not $bashFetcherSource.Contains("LC_ALL=C tr '[:upper:]' '[:lower:]'")) { throw 'Bash fetcher lacks portable deterministic reserved-name normalization' }

$source = Get-Content -Raw -LiteralPath $manifestPath | ConvertFrom-Json

. (Join-Path $PSScriptRoot 'vanilla-assets/OutputMatching.Tests.ps1')

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

# Serves the synthetic download origin for the hermetic fetch fixtures.
$originServerJob = $null

try {
    $sandboxScripts = Join-Path $sandboxRoot "scripts"
    $sandboxAssets = Join-Path $sandboxRoot "assets"
    $sandboxManifest = Join-Path $sandboxAssets "vanilla-source.json"
    New-Item -ItemType Directory -Path $sandboxScripts, $sandboxAssets | Out-Null
    Copy-Item -LiteralPath (Join-Path $repoRoot "scripts\fetch-vanilla-assets.sh") `
        -Destination (Join-Path $sandboxScripts "fetch-vanilla-assets.sh")
    Copy-Item -LiteralPath (Join-Path $repoRoot "scripts\rename-directory-no-replace.c") `
        -Destination (Join-Path $sandboxScripts "rename-directory-no-replace.c")
    Copy-Item -LiteralPath $fetcher -Destination (Join-Path $sandboxScripts "fetch-vanilla-assets.ps1")

    $sandboxBashFetcher = Join-Path $sandboxScripts "fetch-vanilla-assets.sh"
    $sandboxPowerShellFetcher = Join-Path $sandboxScripts "fetch-vanilla-assets.ps1"
    $sandboxNoFileHashWrapper = Join-Path $sandboxScripts "invoke-without-get-file-hash.ps1"
    @'
param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Fetcher,
    [switch]$AcceptEula,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
# Windows PowerShell 5.1 exports Get-FileHash as an auto-loadable script
# function while PowerShell 7+ ships it as a compiled cmdlet that no session
# can remove uniformly. A shadowing function fails loudly on either host —
# functions outrank cmdlets during command resolution, so every possible
# Get-FileHash call inside the fetcher throws — proving the fetcher hashes
# pinned digests through its own .NET primitives. Session module state is
# left untouched: newer hosts resolve their base cmdlets through automatic
# module loading and must keep it enabled.
function Get-FileHash {
    throw "test precondition violated: the fetcher must hash without Get-FileHash"
}
if ((Get-Command Get-FileHash -ErrorAction SilentlyContinue).CommandType -ne
        [System.Management.Automation.CommandTypes]::Function) {
    throw "test precondition failed: the Get-FileHash shadow is not in effect"
}
try {
    & $Fetcher -AcceptEula:$AcceptEula -DryRun:$DryRun
} catch {
    [Console]::Error.WriteLine($_)
    exit 1
}
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
'@ | Set-Content -LiteralPath $sandboxNoFileHashWrapper -Encoding UTF8
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
    $syntheticSha256 = Get-TestSha256Hex -Path $syntheticArchivePath
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
    $traversalSha256 = Get-TestSha256Hex -Path $syntheticArchivePath
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

    # Pinned SHA-256 verification must fail closed without depending on the
    # Microsoft.PowerShell.Utility script module auto-loading Get-FileHash.
    # The download origin is served over a loopback HTTP stub instead of a
    # file:// URL: Windows PowerShell 5.1 downloads file:// through
    # WebRequest, while PowerShell 7+ (the host the Linux/macOS CI lanes
    # run under) refuses that scheme outright. A raw TCP listener needs no
    # platform URL reservations, stays fully hermetic (fixtures only, no
    # external network), and exercises the same http download path every
    # host uses for the real pinned source.
    if (Test-Path -LiteralPath $syntheticCache) {
        Remove-Item -Recurse -Force -LiteralPath $syntheticCache
    }
    if (Test-Path -LiteralPath $syntheticArchivePath) {
        Remove-Item -Force -LiteralPath $syntheticArchivePath
    }
    $originArchive = Join-Path $sandboxRoot "origin\pinned-source.zip"
    New-Item -ItemType Directory -Force -Path (Split-Path -Parent $originArchive) | Out-Null
    New-TestZipArchive -Path $originArchive -Entries @(
        [pscustomobject]@{ Name = "resource_pack/"; Content = $null },
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" }
    )
    $originPortFile = Join-Path $sandboxRoot "origin-port.txt"
    if (Test-Path -LiteralPath $originPortFile) {
        Remove-Item -Force -LiteralPath $originPortFile
    }
    $originServerJob = Start-Job -ScriptBlock {
        param([string]$ArchivePath, [string]$PortFile)
        $listener = [System.Net.Sockets.TcpListener]::new(
            [System.Net.IPAddress]::Loopback,
            0
        )
        $listener.Start()
        try {
            [System.IO.File]::WriteAllText(
                $PortFile,
                [string](([System.Net.IPEndPoint]$listener.LocalEndpoint).Port)
            )
            while ($true) {
                $client = $listener.AcceptTcpClient()
                try {
                    $stream = $client.GetStream()
                    $reader = [System.IO.StreamReader]::new($stream)
                    $requestLine = $reader.ReadLine()
                    while (-not [string]::IsNullOrEmpty($reader.ReadLine())) { }
                    if ($null -ne $requestLine -and $requestLine.StartsWith("GET ")) {
                        $body = [System.IO.File]::ReadAllBytes($ArchivePath)
                        $header = "HTTP/1.0 200 OK`r`nContent-Length: $($body.Length)`r`nConnection: close`r`n`r`n"
                        $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
                        $stream.Write($headerBytes, 0, $headerBytes.Length)
                        $stream.Write($body, 0, $body.Length)
                    } else {
                        $notFound = [System.Text.Encoding]::ASCII.GetBytes(
                            "HTTP/1.0 404 Not Found`r`nContent-Length: 0`r`n`r`n"
                        )
                        $stream.Write($notFound, 0, $notFound.Length)
                    }
                } finally {
                    $client.Dispose()
                }
            }
        } catch {
            # The harness stops this job between fixtures; accept-loop aborts land here.
        } finally {
            $listener.Stop()
        }
    } -ArgumentList $originArchive, $originPortFile

    $originReadyDeadline = [DateTime]::UtcNow.AddSeconds(10)
    while (-not (Test-Path -LiteralPath $originPortFile) -and [DateTime]::UtcNow -lt $originReadyDeadline) {
        Start-Sleep -Milliseconds 50
    }
    if (-not (Test-Path -LiteralPath $originPortFile)) {
        throw "the loopback origin server did not report its port"
    }
    $originPort = [int](Get-Content -Raw -LiteralPath $originPortFile)
    $originUrl = "http://127.0.0.1:$originPort/pinned-source.zip"
    $originSha256 = Get-TestSha256Hex -Path $originArchive
    $wrongSha256 = "0" * 64

    Write-TestManifest -Template $source -Path $sandboxManifest `
        -Archive $syntheticArchiveName -CacheDirectory $syntheticCacheRelative `
        -Sha256 $wrongSha256 -Url $originUrl
    $mismatchResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $sandboxNoFileHashWrapper,
        $sandboxPowerShellFetcher,
        "-AcceptEula"
    )
    if ($mismatchResult.ExitCode -eq 0) {
        $sandboxFailures += "PowerShell accepted an archive whose SHA-256 misses the pinned digest"
    }
    $mismatchDiagnostic = "SHA-256 mismatch: expected $wrongSha256, got $originSha256"
    if (-not (Test-OutputContains -Output $mismatchResult.Output -Needle $mismatchDiagnostic)) {
        $sandboxFailures += "PowerShell mismatch failure omitted the exact digests '$mismatchDiagnostic': $($mismatchResult.Output.Trim())"
    }
    foreach ($residue in @($syntheticCache, $syntheticArchivePath, "$syntheticArchivePath.partial")) {
        if (Test-Path -LiteralPath $residue) {
            $sandboxFailures += "PowerShell kept '$residue' after rejecting a mismatched download"
        }
    }

    # A cached archive that no longer matches the pinned digest is discarded and
    # re-fetched rather than trusted.
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{ `"stale`": true }" }
    )
    Write-TestManifest -Template $source -Path $sandboxManifest `
        -Archive $syntheticArchiveName -CacheDirectory $syntheticCacheRelative `
        -Sha256 $originSha256 -Url $originUrl
    $staleResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $sandboxNoFileHashWrapper,
        $sandboxPowerShellFetcher,
        "-AcceptEula"
    )
    if ($staleResult.ExitCode -ne 0) {
        $sandboxFailures += "PowerShell failed to replace a stale cached archive: $($staleResult.Output.Trim())"
    }
    $publishedSentinel = Join-Path $syntheticCache "resource_pack\blocks.json"
    if (-not (Test-Path -LiteralPath $publishedSentinel -PathType Leaf)) {
        $sandboxFailures += "PowerShell did not publish the re-fetched pinned archive"
    } elseif ((Get-Content -Raw -LiteralPath $publishedSentinel).Contains("stale")) {
        $sandboxFailures += "PowerShell published content from the stale cached archive"
    }
    if ((Test-Path -LiteralPath $syntheticArchivePath -PathType Leaf) -and
        (Get-TestSha256Hex -Path $syntheticArchivePath) -cne $originSha256) {
        $sandboxFailures += "PowerShell retained an archive that misses the pinned digest"
    }

    # -----------------------------------------------------------------
    # VPA-209 bounded-extraction contracts: entry-count, expanded-byte and
    # compression-ratio bounds, duplicate/collision rejection, link-entry
    # rejection, transactional staging, and stale-staging reclamation. Both
    # platform fetchers must reject identical fixtures with identical
    # diagnostics. Tightening-only CLI overrides let tiny fixtures trip the
    # exact production bound logic without building multi-gigabyte archives;
    # the ratio cases below run against the REAL default constants.
    # -----------------------------------------------------------------

    $assetSandboxRoot = Join-Path $sandboxRoot ".local\assets"
    # Matches this script's dot-form leftovers AND the PowerShell fetcher's
    # hyphen-form runtime staging ("...extracting-<pid>-<guid>").
    $stagingResidueFilter = "synthetic-vanilla.extracting*"

    $bashDeepTools = $false
    $bashDeepProbeScript = Join-Path $sandboxScripts "probe-bash-tools.sh"
    Set-Content -LiteralPath $bashDeepProbeScript -Value @'
if command -v unzip >/dev/null 2>&1 && command -v cc >/dev/null 2>&1; then
    exit 0
fi
exit 1
'@ -Encoding ASCII
    $bashDeepProbeResult = Invoke-NativeCapture -FilePath $bash -ArgumentList @($bashDeepProbeScript)
    $bashDeepTools = ($bashDeepProbeResult.ExitCode -eq 0)
    function Assert-ExtractionRejection {
        param(
            [Parameter(Mandatory = $true)]
            [string]$Label,
            [string[]]$PowerShellArgs = @(),
            [string[]]$BashArgs = @(),
            [Parameter(Mandatory = $true)]
            [string]$Needle
        )

        $psArguments = @(
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
            $sandboxPowerShellFetcher,
            "-AcceptEula"
        ) + $PowerShellArgs
        $psResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList $psArguments
        if ($psResult.ExitCode -eq 0) {
            $script:sandboxFailures += "$Label(PowerShell): unexpectedly succeeded"
        } elseif (-not (Test-OutputContains -Output $psResult.Output -Needle $Needle)) {
            $script:sandboxFailures += "$Label(PowerShell): omitted '$Needle': $($psResult.Output.Trim())"
        }
        if (Test-Path -LiteralPath $syntheticCache) {
            $script:sandboxFailures += "$Label(PowerShell): published a cache after rejection"
        }
        $residue = @(Get-ChildItem -Force -LiteralPath $assetSandboxRoot -Directory -Filter $stagingResidueFilter -ErrorAction SilentlyContinue)
        if ($residue.Count -ne 0) {
            $script:sandboxFailures += "$Label(PowerShell): left extraction staging behind: $($residue.FullName -join ', ')"
        }

        if (-not $bashDeepTools) {
            return
        }
        $shArgs = @($sandboxBashFetcher, "--accept-eula") + $BashArgs
        $shResult = Invoke-NativeCapture -FilePath $bash -ArgumentList $shArgs
        if ($shResult.ExitCode -eq 0) {
            $script:sandboxFailures += "$Label(bash): unexpectedly succeeded"
        } elseif (-not (Test-OutputContains -Output $shResult.Output -Needle $Needle)) {
            $script:sandboxFailures += "$Label(bash): omitted '$Needle': $($shResult.Output.Trim())"
        }
        if (Test-Path -LiteralPath $syntheticCache) {
            $script:sandboxFailures += "$Label(bash): published a cache after rejection"
        }
        $residue = @(Get-ChildItem -Force -LiteralPath $assetSandboxRoot -Directory -Filter $stagingResidueFilter -ErrorAction SilentlyContinue)
        if ($residue.Count -ne 0) {
            $script:sandboxFailures += "$Label(bash): left extraction staging behind: $($residue.FullName -join ', ')"
        }
    }

    function Write-BoundedCaseManifest {
        param([string]$Sha)
        Write-TestManifest -Template $source -Path $sandboxManifest `
            -Archive $syntheticArchiveName -CacheDirectory $syntheticCacheRelative -Sha256 $Sha
    }

    function Reset-BoundedFixture {
        if (Test-Path -LiteralPath $syntheticArchivePath) {
            Remove-Item -Force -LiteralPath $syntheticArchivePath
        }
        if (Test-Path -LiteralPath $syntheticCache) {
            Remove-Item -Recurse -Force -LiteralPath $syntheticCache
        }
    }

    # --- entry-count bound (tightened override trips on 3 entries) ---
    Reset-BoundedFixture
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "a.txt"; Content = "123" },
        [pscustomobject]@{ Name = "b.txt"; Content = "456" },
        [pscustomobject]@{ Name = "c.txt"; Content = "789" }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "entry-count" `
        -PowerShellArgs @("-MaxArchiveEntriesOverride", "2") `
        -BashArgs @("--max-archive-entries=2") `
        -Needle "archive entry count 3 exceeds the maximum 2"

    # --- per-file declared expanded bytes (tightened override) ---
    Assert-ExtractionRejection -Label "per-file-bytes" `
        -PowerShellArgs @("-MaxExpandedFileBytesOverride", "1") `
        -BashArgs @("--max-expanded-file-bytes=1") `
        -Needle "declared expanded size 3 exceeds the maximum 1 bytes"

    # --- total declared expanded bytes fails fast at the first offender ---
    Assert-ExtractionRejection -Label "total-bytes" `
        -PowerShellArgs @("-MaxTotalExpandedBytesOverride", "2") `
        -BashArgs @("--max-total-expanded-bytes=2") `
        -Needle "total declared expanded size 3 exceeds the maximum 2 bytes"

    # --- tightening-only guard: raising a bound is refused ---
    if ($bashDeepTools) {
        $raiseResult = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
            $sandboxBashFetcher,
            "--accept-eula",
            "--max-archive-entries=99999999"
        )
        if ($raiseResult.ExitCode -eq 0) {
            $sandboxFailures += "tighten-guard(bash): accepted raising a bound"
        } elseif (-not (Test-OutputContains -Output $raiseResult.Output -Needle "overrides may only tighten bounds")) {
            $sandboxFailures += "tighten-guard(bash): omitted tighten-only diagnostic: $($raiseResult.Output.Trim())"
        }

        $overflowRaiseResult = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
            $sandboxBashFetcher,
            "--accept-eula",
            "--dry-run",
            "--max-archive-entries=999999999999999999999999999999999999"
        )
        if ($overflowRaiseResult.ExitCode -eq 0) {
            $sandboxFailures += "tighten-guard-overflow(bash): accepted an overflowing bound"
        } elseif (-not (Test-OutputContains -Output $overflowRaiseResult.Output -Needle "overrides may only tighten bounds")) {
            $sandboxFailures += "tighten-guard-overflow(bash): omitted tighten-only diagnostic: $($overflowRaiseResult.Output.Trim())"
        }
    }
    $raisePsResult = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $sandboxPowerShellFetcher,
        "-AcceptEula",
        "-MaxArchiveEntriesOverride",
        "99999999"
    )
    if ($raisePsResult.ExitCode -eq 0) {
        $sandboxFailures += "tighten-guard(PowerShell): accepted raising a bound"
    } elseif (-not (Test-OutputContains -Output $raisePsResult.Output -Needle "overrides may only tighten bounds")) {
        $sandboxFailures += "tighten-guard(PowerShell): omitted tighten-only diagnostic: $($raisePsResult.Output.Trim())"
    }

    # --- per-entry compression-ratio bomb against DEFAULT constants ---
    # Deterministic content: ~51 KB of varied JSON-ish text (~6 KB deflated)
    # plus 8 MB of one repeated character (~5 KB deflated). Expanded is far
    # below every byte cap while the ratio exceeds the default 500 guard.
    $ratioPrefix = New-Object System.Text.StringBuilder
    for ($i = 0; $i -lt 2048; $i++) { [void]$ratioPrefix.Append("{""key$i"": ""abcdefgh""},`r`n") }
    Reset-BoundedFixture
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "bomb.bin"; Content = ($ratioPrefix.ToString() + ("A" * 8000000)) }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "per-entry-ratio-default" `
        -Needle "exceeds the per-entry maximum 500"

    # --- aggregate-only bomb against DEFAULT constants: two entries whose
    # individual ratios stay under 500 but whose aggregate exceeds 100 ---
    Reset-BoundedFixture
    $midContent = $ratioPrefix.ToString() + ("A" * 1250000)
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "m1.bin"; Content = $midContent },
        [pscustomobject]@{ Name = "m2.bin"; Content = $midContent }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "aggregate-ratio-default" `
        -Needle "exceeds the aggregate maximum 100"

    # --- unsafe entry names beyond traversal (which is covered above) ---
    foreach ($unsafeCase in @(
        @{ Name = "/abs.txt"; Needle = "absolute and UNC paths are not allowed" },
        @{ Name = "resource_pack/CON"; Needle = "reserved filename component 'CON'" },
        @{ Name = "resource_pack/a:b.txt"; Needle = "drive and alternate-stream paths are not allowed" },
        @{ Name = "resource_pack/trail. "; Needle = "invalid filename component 'trail. '" },
        @{ Name = "resource_pack//deep.txt"; Needle = "empty path components are not allowed" }
    )) {
        Reset-BoundedFixture
        New-TestZipArchive -Path $syntheticArchivePath -Entries @(
            [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
            [pscustomobject]@{ Name = $unsafeCase.Name; Content = "x" }
        )
        Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
        Assert-ExtractionRejection -Label "unsafe[$($unsafeCase.Name)]" `
            -Needle $unsafeCase.Needle
    }

    # --- duplicate file entries are rejected rather than last-wins ---
    Reset-BoundedFixture
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}`nsecond" }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "duplicate-entry" `
        -Needle "duplicate ZIP entry path 'resource_pack/blocks.json'"

    . (Join-Path $PSScriptRoot 'vanilla-assets\DirectoryEntryCases.ps1')

    # --- file/directory path collisions are rejected on both platforms ---
    Reset-BoundedFixture
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
        [pscustomobject]@{ Name = "resource_pack/blocks.json/inner.txt"; Content = "{}" }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "path-collision" `
        -Needle "ZIP entry path collision at 'resource_pack/blocks.json'"

    # --- link entries: both fetchers reject declared Unix link modes from
    # central-directory metadata BEFORE anything is written, so neither
    # extractor ever gets the chance to materialize a link or write through
    # one. The leaf-link fixture keeps its original single-entry shape; the
    # directory-symlink fixture adds child members whose only extraction
    # route would be THROUGH the rejected link, proving those children are
    # never written outside staging on either platform.
    Reset-BoundedFixture
    New-TestZipArchive -Raw -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
        @{
            Name = "evil"
            Content = "../../escaped-target.txt"
            MadeByOs = 3
            ExternalAttributes = ([int64]0xA1FF * 65536)
        }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "leaf-symlink" `
        -Needle "link entries are not allowed"

    Reset-BoundedFixture
    New-Item -ItemType Directory -Force -Path (Join-Path $sandboxRoot "escaped-dir") | Out-Null
    New-TestZipArchive -Raw -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
        @{
            Name = "evil"
            Content = "../../../escaped-dir"
            MadeByOs = 3
            ExternalAttributes = ([int64]0xA1FF * 65536)
        },
        [pscustomobject]@{ Name = "evil/escaped-child.txt"; Content = "must never pass through a directory symlink" }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
    Assert-ExtractionRejection -Label "directory-symlink-child" `
        -Needle "link entries are not allowed"
    $escapedChildren = @(Get-ChildItem -Force -Recurse -LiteralPath $sandboxRoot -Filter "escaped-child.txt" -ErrorAction SilentlyContinue)
    if ($escapedChildren.Count -ne 0) {
        $sandboxFailures += "directory-symlink-child: children were written through the link outside staging: $($escapedChildren.FullName -join ', ')"
    }

    # --- publish-race witness: a concurrent writer creating the cache target
    # between extraction and publication must fail the run closed instead of
    # letting POSIX `mv dir target` move staging INTO the live directory and
    # corrupting the published layout. The bash deep leg injects that writer
    # deterministically through the helper's PATH-shimmed unzip.---
    if ($bashDeepTools) {
        Reset-BoundedFixture
        New-TestZipArchive -Path $syntheticArchivePath -Entries @(
            [pscustomobject]@{ Name = "behavior_pack/"; Content = $null },
            [pscustomobject]@{ Name = "behavior_pack/items/"; Content = $null },
            [pscustomobject]@{ Name = "behavior_pack/items/rabbit.json"; Content = "{}" },
            [pscustomobject]@{ Name = "resource_pack/"; Content = $null },
            [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" }
        )
        Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
        $sandboxFailures += Test-PublishRaceInjection `
            -BashPath $bash `
            -ShimRoot (Join-Path $sandboxRoot "race-shims") `
            -FetcherPath $sandboxBashFetcher `
            -CacheDirectory $syntheticCache `
            -AssetRoot $assetSandboxRoot `
            -StagingResidueFilter $stagingResidueFilter
    }
    . (Join-Path $PSScriptRoot 'vanilla-assets\PublicationCases.ps1')

    # --- stale staging reclamation: interrupted-run leftovers older than the
    # retention age are reclaimed at startup, fresh leftovers survive within
    # the count bound, and the oldest beyond the bound lose. Both platforms.---
    Reset-BoundedFixture
    New-TestZipArchive -Path $syntheticArchivePath -Entries @(
        [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" }
    )
    Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)

    $staleOld = Join-Path $assetSandboxRoot "synthetic-vanilla.extracting.staleold"
    New-Item -ItemType Directory -Force -Path $staleOld | Out-Null
    Set-Content -LiteralPath (Join-Path $staleOld "sentinel") -Value "junk"
    [System.IO.Directory]::SetLastWriteTimeUtc($staleOld, ([DateTime]::UtcNow - [TimeSpan]::FromDays(3)))
    $freshKeep = Join-Path $assetSandboxRoot "synthetic-vanilla.extracting.freshkeep"
    New-Item -ItemType Directory -Force -Path $freshKeep | Out-Null
    for ($i = 1; $i -le 6; $i++) {
        $extra = Join-Path $assetSandboxRoot ("synthetic-vanilla.extracting.extra{0:d2}" -f $i)
        New-Item -ItemType Directory -Force -Path $extra | Out-Null
        [System.IO.Directory]::SetLastWriteTimeUtc($extra, ([DateTime]::UtcNow - [TimeSpan]::FromHours(2 * (7 - $i))))
    }

    $reclaimPs = Invoke-NativeCapture -FilePath $childPowerShell -ArgumentList @(
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        $sandboxPowerShellFetcher,
        "-AcceptEula"
    )
    if ($reclaimPs.ExitCode -ne 0) {
        $sandboxFailures += "reclaim(PowerShell): valid fetch failed: $($reclaimPs.Output.Trim())"
    } elseif (-not (Test-OutputContains -Output $reclaimPs.Output -Needle "Reclaimed ")) {
        $sandboxFailures += "reclaim(PowerShell): no reclaim marker emitted"
    }
    if (Test-Path -LiteralPath $staleOld) {
        $sandboxFailures += "reclaim(PowerShell): stale staging survived past the retention age"
    }
    if (-not (Test-Path -LiteralPath $freshKeep)) {
        $sandboxFailures += "reclaim(PowerShell): fresh staging was reclaimed despite remaining under the count bound"
    }
    $remainingAfterPs = @(Get-ChildItem -Force -LiteralPath $assetSandboxRoot -Directory -Filter $stagingResidueFilter)
    if ($remainingAfterPs.Count -gt 4) {
        $sandboxFailures += "reclaim(PowerShell): $($remainingAfterPs.Count) staging dirs exceed the keep bound of 4"
    }

    if ($bashDeepTools) {
        if (Test-Path -LiteralPath $syntheticCache) {
            Remove-Item -Recurse -Force -LiteralPath $syntheticCache
        }
        $staleOldBash = Join-Path $assetSandboxRoot "synthetic-vanilla.extracting.bashstale"
        New-Item -ItemType Directory -Force -Path $staleOldBash | Out-Null
        [System.IO.Directory]::SetLastWriteTimeUtc($staleOldBash, ([DateTime]::UtcNow - [TimeSpan]::FromDays(3)))
        $freshKeepBash = Join-Path $assetSandboxRoot "synthetic-vanilla.extracting.bashfresh"
        New-Item -ItemType Directory -Force -Path $freshKeepBash | Out-Null

        $reclaimSh = Invoke-NativeCapture -FilePath $bash -ArgumentList @($sandboxBashFetcher, "--accept-eula")
        if ($reclaimSh.ExitCode -ne 0) {
            $sandboxFailures += "reclaim(bash): valid fetch failed: $($reclaimSh.Output.Trim())"
        } elseif (-not (Test-OutputContains -Output $reclaimSh.Output -Needle "Reclaimed ")) {
            $sandboxFailures += "reclaim(bash): no reclaim marker emitted"
        }
        if (Test-Path -LiteralPath $staleOldBash) {
            $sandboxFailures += "reclaim(bash): stale staging survived past the retention age"
        }
        if (-not (Test-Path -LiteralPath $freshKeepBash)) {
            $sandboxFailures += "reclaim(bash): fresh staging was reclaimed despite remaining under the count bound"
        }
    } else {
        Write-Output "NOTE: bash deep-extraction contracts skipped because unzip or cc is unavailable in the discovered bash."
    }

    if ($sandboxFailures.Count -ne 0) {
        throw "fetcher safety contract failures:`n$($sandboxFailures -join "`n")"
    }
} finally {
    if ($null -ne $originServerJob) {
        Stop-Job -Job $originServerJob -ErrorAction SilentlyContinue
        Remove-Job -Job $originServerJob -Force -ErrorAction SilentlyContinue
    }
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
