param(
    [ValidateSet('Render', 'World')][string]$Check = 'Render',
    [Parameter(Mandatory)][string]$Emulator,
    [Parameter(Mandatory)][string]$Config,
    [Parameter(Mandatory)][string]$VitaFs,
    [string]$WorldCarrier,
    [ValidateRange(5, 60)][int]$TimeoutSeconds = 45
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$vitaRepo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$vitaStem = if ($Check -eq 'World') { 'world-check' } else { 'render-check' }
$vitaVpk = Join-Path $vitaRepo "target-vita/$vitaStem/cinnabar-$vitaStem.vpk"
$Emulator = [IO.Path]::GetFullPath($Emulator)
$Config = [IO.Path]::GetFullPath($Config)
$VitaFs = [IO.Path]::GetFullPath($VitaFs).TrimEnd('\', '/')
foreach ($vitaFile in @($Emulator, $Config, $vitaVpk)) {
    if (-not (Test-Path -LiteralPath $vitaFile -PathType Leaf)) { throw "Missing input: $vitaFile" }
}
# An isolated binary directory matters: Vita3K writes its log beside its executable.
$vitaOther = Get-Process -Name ([IO.Path]::GetFileNameWithoutExtension($Emulator)) -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $Emulator }
if ($vitaOther) { throw 'That emulator copy is already running. Use a separate test copy.' }
$vitaConfigText = Get-Content -LiteralPath $Config -Raw
$vitaPrefMatch = [regex]::Match($vitaConfigText, '(?m)^pref-path:\s*(.+)$')
if (-not $vitaPrefMatch.Success) { throw 'The test config must explicitly set pref-path.' }
$vitaConfiguredFs = $vitaPrefMatch.Groups[1].Value.Trim().Trim('"').Trim("'")
if ([IO.Path]::GetFullPath($vitaConfiguredFs).TrimEnd('\', '/') -ne $VitaFs) {
    throw 'VitaFs does not match the isolated config pref-path.'
}
$vitaRunId = (Get-Date -Format 'yyyyMMdd-HHmmss') + '-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$vitaResultDir = Join-Path $vitaRepo ('target-vita/emulator/' + $vitaRunId)
$vitaAppData = Join-Path $VitaFs 'ux0/data/cinnabar'
New-Item -ItemType Directory -Path $vitaResultDir, $vitaAppData -Force | Out-Null
$vitaCarrierHash = $null
if ($WorldCarrier) {
    if ($Check -ne 'World') { throw '-WorldCarrier requires -Check World.' }
    $vitaAssets = Join-Path $vitaAppData 'assets'
    New-Item -ItemType Directory -Force -Path $vitaAssets | Out-Null
    $vitaCarrierHash = (Get-FileHash -LiteralPath $WorldCarrier -Algorithm SHA256).Hash
    $vitaCarrierCopy = Join-Path $vitaAssets (Split-Path $WorldCarrier -Leaf)
    Copy-Item -LiteralPath $WorldCarrier -Destination $vitaCarrierCopy -Force
    if ((Get-FileHash -LiteralPath $vitaCarrierCopy -Algorithm SHA256).Hash -ne $vitaCarrierHash) {
        throw 'Staged world carrier hash differs.'
    }
}
$vitaAppLog = Join-Path $vitaAppData "$vitaStem.log"
$vitaFrame = Join-Path $vitaAppData "$vitaStem.tga"
$vitaBeforeFrame = Join-Path $vitaAppData "$vitaStem-before.tga"
$vitaMarker = Join-Path $vitaAppData "$vitaStem.auto"
# Archive these developer-example outputs so a stale PASS/capture cannot pass a new run.
foreach ($vitaOld in @($vitaAppLog, $vitaFrame, $vitaBeforeFrame)) {
    if (Test-Path -LiteralPath $vitaOld) {
        Move-Item -LiteralPath $vitaOld -Destination (Join-Path $vitaResultDir ('previous-' + [IO.Path]::GetFileName($vitaOld)))
    }
}
$vitaMarkerExisted = Test-Path -LiteralPath $vitaMarker
$vitaArgs = @('--frontend', 'sdl', '--load-config', '--keep-config', '--config-location', $Config, $vitaVpk)
# Windows filenames cannot contain a quote; quote each validated file argument for Start-Process.
$vitaArguments = $vitaArgs | ForEach-Object { '"' + $_ + '"' }
$vitaProcess = $null
try {
    if (-not $vitaMarkerExisted) { Set-Content -LiteralPath $vitaMarker -Value 'automatic developer render check' }
    $vitaProcess = Start-Process -FilePath $Emulator -ArgumentList $vitaArguments -WindowStyle Hidden -PassThru `
        -WorkingDirectory $vitaResultDir -RedirectStandardOutput (Join-Path $vitaResultDir 'stdout.log') `
        -RedirectStandardError (Join-Path $vitaResultDir 'stderr.log')
    $vitaFinished = $vitaProcess.WaitForExit($TimeoutSeconds * 1000)
    if (-not $vitaFinished) { Stop-Process -Id $vitaProcess.Id; $vitaProcess.WaitForExit() }
    $vitaGuestPass = $false
    $vitaCarrierLoaded = $false
    if (Test-Path -LiteralPath $vitaAppLog) {
        Copy-Item -LiteralPath $vitaAppLog -Destination $vitaResultDir
        $vitaGuestPass = [bool](Select-String -LiteralPath $vitaAppLog -Pattern '^PASS:')
        $vitaCarrierLoaded = [bool](Select-String -LiteralPath $vitaAppLog -Pattern '^world carrier bytes=')
    }
    $vitaHasFrame = Test-Path -LiteralPath $vitaFrame
    if ($vitaHasFrame) { Copy-Item -LiteralPath $vitaFrame -Destination $vitaResultDir }
    if (Test-Path -LiteralPath $vitaBeforeFrame) { Copy-Item -LiteralPath $vitaBeforeFrame -Destination $vitaResultDir }
    $vitaEmuLog = Join-Path (Split-Path $Emulator -Parent) 'vita3k.log'
    if (Test-Path -LiteralPath $vitaEmuLog) { Copy-Item -LiteralPath $vitaEmuLog -Destination $vitaResultDir }
    $vitaResult = [ordered]@{
        artifact = "developer $vitaStem; not a Minecraft game"
        vpkSha256 = (Get-FileHash -LiteralPath $vitaVpk -Algorithm SHA256).Hash
        emulatorSha256 = (Get-FileHash -LiteralPath $Emulator -Algorithm SHA256).Hash
        configSha256 = (Get-FileHash -LiteralPath $Config -Algorithm SHA256).Hash
        worldCarrierSha256 = $vitaCarrierHash
        worldCarrierLoaded = $vitaCarrierLoaded
        installedCarriers = @(Get-ChildItem -LiteralPath (Join-Path $vitaAppData 'assets') -File -ErrorAction SilentlyContinue | ForEach-Object {
            @{ name = $_.Name; bytes = $_.Length; sha256 = (Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash }
        })
        memoryMapping = [regex]::Match($vitaConfigText, '(?m)^memory-mapping:\s*(.+)$').Groups[1].Value.Trim()
        timedOut = -not $vitaFinished
        guestCompletedWithVariedFrame = $vitaGuestPass
        frameSaved = $vitaHasFrame
        frameSha256 = $(if ($vitaHasFrame) { (Get-FileHash -LiteralPath $vitaFrame -Algorithm SHA256).Hash } else { $null })
        emulatorExitCode = $vitaProcess.ExitCode
        resultDirectory = $vitaResultDir
    }
    $vitaResult | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $vitaResultDir 'result.json')
    $vitaResult | ConvertTo-Json
    if (-not $vitaGuestPass -or -not $vitaHasFrame -or -not $vitaFinished -or $vitaProcess.ExitCode -ne 0 -or ($WorldCarrier -and -not $vitaCarrierLoaded)) {
        throw 'The test did not pass every gate. Inspect result.json, the guest log, and vita3k.log separately.'
    }
} finally {
    if ($vitaProcess -and -not $vitaProcess.HasExited) { Stop-Process -Id $vitaProcess.Id; $vitaProcess.WaitForExit() }
    if (-not $vitaMarkerExisted -and (Test-Path -LiteralPath $vitaMarker)) { Remove-Item -LiteralPath $vitaMarker }
}
