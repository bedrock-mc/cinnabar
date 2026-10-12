# Stages the preparation kit: assetc plus each file packaging/common/prep-kit-files.txt lists, at its
# checkout path. The Windows twin of stage_prep_kit in packaging/common/stage-payload.sh.
param([Parameter(Mandatory)][string]$Root, [Parameter(Mandatory)][string]$Kit,
      [Parameter(Mandatory)][string]$Assetc)
$ErrorActionPreference = "Stop"
$list = Join-Path $Root "packaging/common/prep-kit-files.txt"
if (-not (Test-Path -LiteralPath $list)) {
    throw "missing $list; regenerate it with cargo run -p asset-compiler --bin assetc -- kit-files"
}
if (Test-Path -LiteralPath $Kit) { Remove-Item -LiteralPath $Kit -Recurse -Force }
New-Item -ItemType Directory -Force (Join-Path $Kit "bin") | Out-Null
Copy-Item -LiteralPath $Assetc (Join-Path $Kit "bin")
foreach ($file in Get-Content -LiteralPath $list) {
    if (-not $file) { continue }
    $relative = $file.Replace('/', [IO.Path]::DirectorySeparatorChar)
    $target = Join-Path $Kit $relative
    New-Item -ItemType Directory -Force (Split-Path -Parent $target) | Out-Null
    Copy-Item -LiteralPath (Join-Path $Root $relative) $target
}
