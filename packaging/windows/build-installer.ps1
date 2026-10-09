# Builds the Windows payload, MSI and self-contained setup EXE. Requires WiX v5,
# WixToolset.BootstrapperApplications.wixext v5, and ImageMagick.
# Usage: build-installer.ps1 [-Out <dir>]. Env: CINNABAR_UPDATE_URL (optional).
param([string]$Out = ".local/dist/windows-release", [string]$Binaries = "",
      [string]$Root = (Join-Path $PSScriptRoot "../.."))
$ErrorActionPreference = "Stop"
$root = (Resolve-Path -LiteralPath $Root).Path
$release = if ($Binaries) { (Resolve-Path -LiteralPath $Binaries).Path } else { Join-Path $root "target/release" }
foreach ($name in "bedrock-client.exe", "bedrock-core.exe", "bedrock-local-server.exe", "assetc.exe") {
    if (-not (Test-Path (Join-Path $release $name))) { throw "missing $release\$name; run make package-binaries" }
}
$manifest = Get-Content -LiteralPath (Join-Path $root "Cargo.toml") -Raw
$workspace = [regex]::Match($manifest, '(?ms)^\[workspace\.package\]\s*\r?\n(.*?)(?=^\[|\z)')
$versionMatch = [regex]::Match($workspace.Groups[1].Value, '(?m)^\s*version\s*=\s*"([^"]+)"')
if (-not $versionMatch.Success) { throw "missing workspace.package.version in source Cargo.toml" }
$version = $versionMatch.Groups[1].Value

$outPath = [IO.Path]::GetFullPath($Out)
New-Item -ItemType Directory -Force $outPath | Out-Null
$payload = Join-Path $outPath "payload"
# Only remove this builder's payload directory after resolving it within the output root.
if ([IO.Path]::GetDirectoryName([IO.Path]::GetFullPath($payload)) -ne $outPath) { throw "unsafe payload path" }
Remove-Item -LiteralPath $payload -Recurse -Force -ErrorAction SilentlyContinue
$resources = Join-Path $payload "resources"
$kit = Join-Path $resources "prep-kit"
New-Item -ItemType Directory -Force (Join-Path $resources "assets"), (Join-Path $resources "licenses"), (Join-Path $resources "fonts"), (Join-Path $kit "bin"), (Join-Path $kit "assets"), (Join-Path $kit "assets/fonts"), (Join-Path $kit "data") | Out-Null
Copy-Item (Join-Path $release "bedrock-client.exe"), (Join-Path $release "bedrock-core.exe"), (Join-Path $release "bedrock-local-server.exe") $payload
Copy-Item (Join-Path $release "assetc.exe") (Join-Path $kit "bin")
Copy-Item (Join-Path $root "crates/assets/data/block-physics-v2193.bin") (Join-Path $resources "assets")
Copy-Item (Join-Path $root "THIRD_PARTY_NOTICES.md") (Join-Path $resources "assets")
Copy-Item (Join-Path $root "LICENSE") (Join-Path $resources "licenses/Cinnabar-LICENSE.md")
Copy-Item (Join-Path $root "assets/licenses/*") (Join-Path $resources "licenses")
$fontSource = Get-Content -Raw (Join-Path $root "assets/cinnangles-sans-source.json") | ConvertFrom-Json
$font = Join-Path $root "assets/fonts/$($fontSource.font_file)"
if (-not (Test-Path $font)) { throw "missing $font" }
Copy-Item $font (Join-Path $resources "fonts")
Copy-Item (Join-Path $root "assets/*.json") (Join-Path $kit "assets")
Copy-Item $font (Join-Path $kit "assets/fonts")
foreach ($stem in "block-registry", "block-light-registry", "biome-registry") { Copy-Item (Join-Path $root "crates/assets/data/$stem-v2193.*") (Join-Path $kit "data") }
if ($env:CINNABAR_UPDATE_URL) { [System.IO.File]::WriteAllText((Join-Path $resources "update-url"), $env:CINNABAR_UPDATE_URL) }

$icon = Join-Path $outPath "cinnabar.ico"
magick -background none -density 384 (Join-Path $PSScriptRoot "../icons/cinnabar.svg") -define icon:auto-resize=256,128,64,48,32,16 $icon
if ($LASTEXITCODE -ne 0) { throw "icon rendering failed" }
$logo = Join-Path $outPath "cinnabar.png"
magick -background none (Join-Path $PSScriptRoot "../icons/cinnabar.svg") -resize 64x64 $logo
if ($LASTEXITCODE -ne 0) { throw "logo rendering failed" }
& (Join-Path $PSScriptRoot "set-executable-icon.ps1") -Executable (Join-Path $payload "bedrock-client.exe") -Icon $icon

& (Join-Path $PSScriptRoot "sign.ps1") -Path (Join-Path $payload "bedrock-client.exe"), (Join-Path $payload "bedrock-core.exe"), (Join-Path $payload "bedrock-local-server.exe"), (Join-Path $kit "bin/assetc.exe")
$msi = Join-Path $outPath "Cinnabar-$version-x64.msi"
wix build -arch x64 -d Version=$version -d Payload=$payload -d Icon=$icon -o $msi (Join-Path $PSScriptRoot "cinnabar.wxs")
if ($LASTEXITCODE -ne 0) { throw "wix build failed" }
& (Join-Path $PSScriptRoot "sign.ps1") -Path $msi
$setup = Join-Path $outPath "Cinnabar-$version-x64-setup.exe"
wix build -arch x64 -ext WixToolset.BootstrapperApplications.wixext -d Version=$version -d Msi=$msi -d Icon=$icon -d Logo=$logo -o $setup (Join-Path $PSScriptRoot "setup.wxs")
if ($LASTEXITCODE -ne 0) { throw "setup bundle build failed" }
if ($env:WINDOWS_CERT_PFX_BASE64) {
    # Burn caches its detached engine; sign that executable as well as the whole bundle.
    $engine = Join-Path $outPath "setup-engine.exe"
    wix burn detach $setup -engine $engine
    if ($LASTEXITCODE -ne 0) { throw "setup engine detach failed" }
    & (Join-Path $PSScriptRoot "sign.ps1") -Path $engine
    $signed = Join-Path $outPath "setup-signed.exe"
    wix burn reattach $setup -engine $engine -o $signed
    if ($LASTEXITCODE -ne 0) { throw "setup engine reattach failed" }
    Move-Item -LiteralPath $signed -Destination $setup -Force
}
& (Join-Path $PSScriptRoot "sign.ps1") -Path $setup
Write-Output $msi
Write-Output $setup
