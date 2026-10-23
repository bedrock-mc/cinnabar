param(
    [ValidateSet('Check', 'Test', 'RenderCheck', 'WorldCheck')][string]$Mode = 'Check',
    [string]$Sdk = $env:VITASDK,
    [string]$FselfTool,
    [string]$VitaGlSource = $env:CINNABAR_VITAGL_SOURCE,
    [ValidateRange(1, 16)][int]$Jobs = 4
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$vitaRepo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$vitaBuild = Join-Path (Split-Path $vitaRepo -Parent) ((Split-Path $vitaRepo -Leaf) + '-build')
$vitaTarget = Join-Path $vitaRepo 'target-vita'
$vitaToolchain = 'nightly-2026-10-22'
$vitaTriple = 'armv7-sony-vita-newlibeabihf'
$vitaPackage = $Mode -in @('RenderCheck', 'WorldCheck')
$vitaExample = if ($Mode -eq 'WorldCheck') { 'world-check' } else { 'render-check' }

function Invoke-Checked([string]$Program, [string[]]$Arguments) {
    & $Program @Arguments
    if ($LASTEXITCODE -ne 0) { throw "$Program exited with $LASTEXITCODE" }
}

$env:CARGO_BUILD_JOBS = "$Jobs"
$env:CARGO_INCREMENTAL = '0'
$env:CARGO_BUILD_BUILD_DIR = $vitaBuild
$env:CARGO_TARGET_DIR = $vitaTarget
Push-Location $vitaRepo
try {
    if ($Mode -eq 'Test') {
        Invoke-Checked cargo @("+$vitaToolchain", 'test', '-p', 'cinnabar-vita', '--lib', '--example', 'world-check')
        return
    }
    if (-not $Sdk -or -not (Test-Path -LiteralPath (Join-Path $Sdk 'bin/arm-vita-eabi-gcc.exe'))) {
        throw 'Set -Sdk to the native hard-float VitaSDK. The Android softfp SDK is incompatible.'
    }
    $env:VITASDK = [IO.Path]::GetFullPath($Sdk)
    if ($VitaGlSource) { $env:CINNABAR_VITAGL_SOURCE = [IO.Path]::GetFullPath($VitaGlSource) }
    $env:PATH = "$env:VITASDK\bin;$env:PATH"
    $env:CARGO_TARGET_ARMV7_SONY_VITA_NEWLIBEABIHF_LINKER = Join-Path $env:VITASDK 'bin/arm-vita-eabi-gcc.exe'
    if ($vitaPackage -and -not $VitaGlSource) {
        throw 'Packaging requires -VitaGlSource built with ENABLE_LEGACY_PIPELINE=1 and NO_SPLASHSCREEN=1; see packaging/vita/README.md.'
    }
    # simd-adler32 0.3.9 enables ARM intrinsics for NEON targets without the gate
    # they require. Apply the missing gate only to that package, retaining NEON.
    $vitaArgs = @("+$vitaToolchain", $(if ($Mode -eq 'Check') { 'check' } else { 'build' }),
        '-Z', 'build-std=std,panic_abort', '-Z', 'profile-rustflags',
        '--config', 'profile.dev.package.simd-adler32.rustflags=["-Zcrate-attr=feature(stdarch_arm_neon_intrinsics)"]',
        '--config', 'profile.dev.panic="abort"',
        '--target', $vitaTriple, '-p', 'cinnabar-vita')
    if ($vitaPackage) { $vitaArgs += @('--example', $vitaExample) }
    Invoke-Checked cargo $vitaArgs
    if ($vitaPackage) {
        if (-not $FselfTool) { $FselfTool = Join-Path $env:VITASDK 'bin/vita-make-fself.exe' }
        if (-not (Test-Path -LiteralPath $FselfTool)) {
            throw 'Missing vita-make-fself. Set -FselfTool to the host packaging utility; target libraries must still use the native SDK.'
        }
        # Deliberately NOT called cinnabar.vpk: it cannot join or play a world.
        $vitaOut = Join-Path $vitaTarget $vitaExample
        New-Item -ItemType Directory -Force -Path $vitaOut | Out-Null
        $vitaElf = Join-Path $vitaTarget "$vitaTriple/debug/examples/$vitaExample.elf"
        if (-not (Test-Path -LiteralPath $vitaElf)) { throw "Missing linked Vita ELF: $vitaElf" }
        Invoke-Checked vita-elf-create @($vitaElf, (Join-Path $vitaOut "$vitaExample.velf"))
        Invoke-Checked $FselfTool @('-s', (Join-Path $vitaOut "$vitaExample.velf"), (Join-Path $vitaOut 'eboot.bin'))
        $vitaTitleId = if ($Mode -eq 'WorldCheck') { 'CINNWORL1' } else { 'CINNTEST1' }
        $vitaTitle = if ($Mode -eq 'WorldCheck') { 'Cinnabar World Check' } else { 'Cinnabar Render Check' }
        Invoke-Checked vita-mksfoex @('-s', "TITLE_ID=$vitaTitleId", $vitaTitle, (Join-Path $vitaOut 'param.sfo'))
        $vitaPackArgs = @('-s', (Join-Path $vitaOut 'param.sfo'), '-b', (Join-Path $vitaOut 'eboot.bin'),
            '-a', ((Join-Path $vitaRepo 'LICENSE') + '=LICENSE'),
            '-a', ((Join-Path $PSScriptRoot 'README.md') + '=DEVELOPMENT.md'))
        if ($VitaGlSource) {
            $vitaPackArgs += @('-a', ((Join-Path $VitaGlSource 'COPYING') + '=licenses/vitaGL/COPYING'),
                '-a', ((Join-Path $VitaGlSource 'COPYING.LESSER') + '=licenses/vitaGL/COPYING.LESSER'))
        }
        $vitaPackArgs += (Join-Path $vitaOut "cinnabar-$vitaExample.vpk")
        Invoke-Checked vita-pack-vpk $vitaPackArgs
    }
} finally { Pop-Location }
