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

if ($null -ne $bash) {
    $sandboxName = "vanilla-assets-test-$([guid]::NewGuid().ToString('N'))"
    $sandboxParent = [System.IO.Path]::GetFullPath([System.IO.Path]::GetTempPath())
    $sandboxRoot = [System.IO.Path]::GetFullPath((Join-Path $sandboxParent $sandboxName))
    $sandboxPrefix = $sandboxParent.TrimEnd([System.IO.Path]::DirectorySeparatorChar) +
        [System.IO.Path]::DirectorySeparatorChar
    if (-not $sandboxRoot.StartsWith($sandboxPrefix, [System.StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing unsafe Bash test sandbox: $sandboxRoot"
    }

    try {
        $sandboxScripts = Join-Path $sandboxRoot "scripts"
        $sandboxAssets = Join-Path $sandboxRoot "assets"
        New-Item -ItemType Directory -Path $sandboxScripts, $sandboxAssets | Out-Null
        Copy-Item -LiteralPath (Join-Path $repoRoot "scripts\fetch-vanilla-assets.sh") `
            -Destination (Join-Path $sandboxScripts "fetch-vanilla-assets.sh")

        $maliciousSource = [ordered]@{}
        foreach ($property in $source.PSObject.Properties) {
            $maliciousSource[$property.Name] = $property.Value
        }
        $maliciousSource["cache_dir"] = ".local/assets/../../tracked-dir"
        $maliciousSource | ConvertTo-Json |
            Set-Content -LiteralPath (Join-Path $sandboxAssets "vanilla-source.json") -Encoding UTF8

        $sandboxFetcher = Join-Path $sandboxScripts "fetch-vanilla-assets.sh"
        $savedErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = "Continue"
            $traversalOutput = & $bash $sandboxFetcher --accept-eula --dry-run 2>&1 | Out-String
            $traversalExit = $LASTEXITCODE
        } finally {
            $ErrorActionPreference = $savedErrorActionPreference
        }
        if ($traversalExit -eq 0) {
            throw "Bash fetcher accepted a traversing cache_dir`n$traversalOutput"
        }
    } finally {
        if (Test-Path -LiteralPath $sandboxRoot) {
            Remove-Item -Recurse -Force -LiteralPath $sandboxRoot
        }
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
