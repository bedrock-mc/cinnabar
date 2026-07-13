$ErrorActionPreference = 'Stop'

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param($Expected, $Actual, [string]$Message)
    if ($Expected -cne $Actual) {
        throw "$Message`nexpected: $Expected`nactual:   $Actual"
    }
}

function Assert-Throws {
    param([scriptblock]$Action, [string]$Message)
    $threw = $false
    try {
        & $Action
    }
    catch {
        $threw = $true
    }
    Assert-True $threw $Message
}

function Assert-ThrowsLike {
    param(
        [scriptblock]$Action,
        [string]$Pattern,
        [string]$Message
    )
    $observed = $null
    try {
        & $Action
    }
    catch {
        $observed = $_.Exception.Message
    }
    Assert-True ($null -ne $observed) $Message
    Assert-True ($observed -like $Pattern) "$Message`nexpected error like: $Pattern`nactual error:        $observed"
}

function New-TestCrossCropAssets {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $visualCount = [BitConverter]::ToUInt32($registryBytes, 16)
    $bytes = [byte[]]::new(200 + 40 * $visualCount)
    [Text.Encoding]::ASCII.GetBytes('MCBEAS04').CopyTo($bytes, 0)
    [BitConverter]::GetBytes([uint32]4).CopyTo($bytes, 8)
    [BitConverter]::GetBytes([uint32]16).CopyTo($bytes, 12)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, 16)
    [BitConverter]::GetBytes([uint32]$visualCount).CopyTo($bytes, 20)
    [BitConverter]::GetBytes([uint64]200).CopyTo($bytes, 96)
    for ($index = 0; $index -lt $visualCount; $index++) {
        $offset = 200 + 40 * $index
        $bytes[$offset + 25] = 2
        [BitConverter]::GetBytes([uint32]0).CopyTo($bytes, $offset + 28)
    }
    [IO.File]::WriteAllBytes($Path, $bytes)
}

function New-TestSlabStairAssets {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$Path
    )
    $entries = @(Get-TestRegistryEntries -RegistryPath $RegistryPath)
    $visualCount = 1 + [int](($entries | Measure-Object sequential_id -Maximum).Maximum)
    $templateCount = 12
    $materialCount = 1
    $quadCount = 17
    $pageCount = 1
    $texturePayloadBytes = 1364
    $tintMapBytes = 8 * 256 * 256 * 3
    $visualOffset = 200
    $hashOffset = $visualOffset + 40 * $visualCount
    $materialOffset = $hashOffset
    $templateOffset = $materialOffset + 12 * $materialCount
    $quadOffset = $templateOffset + 12 * $templateCount
    $animationOffset = $quadOffset + 48 * $quadCount
    $frameOffset = $animationOffset
    $pageOffset = $frameOffset
    $textureOffset = $pageOffset + 64 * $pageCount
    $tintOffset = $textureOffset + $texturePayloadBytes
    $biomeOffset = $tintOffset + $tintMapBytes
    $nameOffset = $biomeOffset
    $payloadLength = $nameOffset
    $bytes = [byte[]]::new($payloadLength + 32)
    [Text.Encoding]::ASCII.GetBytes('MCBEAS04').CopyTo($bytes, 0)
    [BitConverter]::GetBytes([uint32]4).CopyTo($bytes, 8)
    [BitConverter]::GetBytes([uint32]16).CopyTo($bytes, 12)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, 16)
    [BitConverter]::GetBytes([uint32]$visualCount).CopyTo($bytes, 20)
    [BitConverter]::GetBytes([uint32]$materialCount).CopyTo($bytes, 28)
    [BitConverter]::GetBytes([uint32]$templateCount).CopyTo($bytes, 32)
    [BitConverter]::GetBytes([uint32]$quadCount).CopyTo($bytes, 36)
    [BitConverter]::GetBytes([uint32]$pageCount).CopyTo($bytes, 48)
    [BitConverter]::GetBytes([uint32]8).CopyTo($bytes, 52)
    [BitConverter]::GetBytes([uint32]256).CopyTo($bytes, 56)
    $offsets = @($visualOffset, $hashOffset, $materialOffset, $templateOffset, $quadOffset, $animationOffset, $frameOffset, $pageOffset, $textureOffset, $tintOffset, $biomeOffset, $nameOffset, $payloadLength)
    for ($index = 0; $index -lt $offsets.Count; $index++) {
        [BitConverter]::GetBytes([uint64]$offsets[$index]).CopyTo($bytes, 96 + 8 * $index)
    }
    for ($index = 0; $index -lt $visualCount; $index++) {
        $offset = $visualOffset + 40 * $index
        [BitConverter]::GetBytes([uint32]::MaxValue).CopyTo($bytes, $offset + 28)
        [BitConverter]::GetBytes([uint32]::MaxValue).CopyTo($bytes, $offset + 32)
    }
    foreach ($entry in $entries) {
        if ($entry.family -notin @(7, 8)) { continue }
        $offset = $visualOffset + 40 * [int]$entry.sequential_id
        $bytes[$offset + 25] = 3
        $half = if ($entry.family -eq 8) { [int](($entry.canonical_state | ConvertFrom-Json).upside_down_bit.value) } else { 0 }
        $template = if ($entry.family -eq 7) { 0 } elseif ($half -eq 0) { 1 } else { 6 }
        [BitConverter]::GetBytes([uint32]$template).CopyTo($bytes, $offset + 28)
    }
    [BitConverter]::GetBytes([uint32]::MaxValue).CopyTo($bytes, $materialOffset + 8)
    for ($index = 0; $index -lt $templateCount; $index++) {
        $offset = $templateOffset + 12 * $index
        if ($index -eq 11) {
            [BitConverter]::GetBytes([uint32]11).CopyTo($bytes, $offset)
            [BitConverter]::GetBytes([uint32]6).CopyTo($bytes, $offset + 4)
            [BitConverter]::GetBytes([uint32]1).CopyTo($bytes, $offset + 8)
            foreach ($quadIndex in 11..16) {
                $quadFlags = if ($quadIndex -ge 15) { 8 } else { 0 }
                [BitConverter]::GetBytes([uint32]$quadFlags).CopyTo($bytes, $quadOffset + 48 * $quadIndex + 44)
            }
        }
        else {
            [BitConverter]::GetBytes([uint32]$index).CopyTo($bytes, $offset)
            [BitConverter]::GetBytes([uint32]1).CopyTo($bytes, $offset + 4)
            if ($index -gt 0) { [BitConverter]::GetBytes([uint32]2).CopyTo($bytes, $offset + 8) }
            [BitConverter]::GetBytes([uint32]1).CopyTo($bytes, $quadOffset + 48 * $index + 44)
        }
    }
    [BitConverter]::GetBytes([uint32]0).CopyTo($bytes, $pageOffset)
    [BitConverter]::GetBytes([uint32]1).CopyTo($bytes, $pageOffset + 4)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, $pageOffset + 8)
    [BitConverter]::GetBytes([uint64]$textureOffset).CopyTo($bytes, $pageOffset + 16)
    [BitConverter]::GetBytes([uint64]$texturePayloadBytes).CopyTo($bytes, $pageOffset + 24)
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $textureDigest = $sha256.ComputeHash($bytes, $textureOffset, $texturePayloadBytes)
        $textureDigest.CopyTo($bytes, $pageOffset + 32)
        $payloadDigest = $sha256.ComputeHash($bytes, 0, $payloadLength)
        $payloadDigest.CopyTo($bytes, $payloadLength)
    }
    finally { $sha256.Dispose() }
    [IO.File]::WriteAllBytes($Path, $bytes)
}

function Set-TestMcbeas04Seal {
    param([Parameter(Mandatory = $true)][byte[]]$Bytes)
    $payloadLength = $Bytes.Length - 32
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try { $digest = $sha256.ComputeHash($Bytes, 0, $payloadLength) }
    finally { $sha256.Dispose() }
    $digest.CopyTo($Bytes, $payloadLength)
}

function Get-TestRegistryEntries {
    param([Parameter(Mandatory = $true)][string]$RegistryPath)

    $bytes = [IO.File]::ReadAllBytes($RegistryPath)
    $reader = [IO.BinaryReader]::new([IO.MemoryStream]::new($bytes, $false))
    $utf8 = [Text.UTF8Encoding]::new($false, $true)
    try {
        Assert-Equal 'BREG1003' $utf8.GetString($reader.ReadBytes(8)) 'test registry helper received the wrong schema'
        Assert-Equal 1001 $reader.ReadUInt32() 'test registry helper received the wrong protocol'
        $null = $reader.ReadUInt32()
        $recordCount = [int]$reader.ReadUInt32()
        foreach ($ignored in 1..4) { $null = $reader.ReadUInt32() }
        $entries = [Collections.Generic.List[object]]::new()
        for ($recordIndex = 0; $recordIndex -lt $recordCount; $recordIndex++) {
            $sequentialId = $reader.ReadUInt32()
            $null = $reader.ReadUInt32()
            $null = $reader.ReadByte()
            $family = $reader.ReadByte()
            foreach ($ignored in 1..5) { $null = $reader.ReadByte() }
            $boxCount = [int]$reader.ReadByte()
            $null = $reader.ReadUInt16()
            $nameLength = [int]$reader.ReadUInt16()
            $stateLength = [int]$reader.ReadUInt32()
            $null = $reader.ReadBytes(32 + 24 * $boxCount)
            $name = $utf8.GetString($reader.ReadBytes($nameLength))
            $canonicalState = $utf8.GetString($reader.ReadBytes($stateLength))
            $entries.Add([pscustomobject][ordered]@{
                sequential_id = $sequentialId
                family = $family
                name = $name
                canonical_state = $canonicalState
            })
        }
        return @($entries)
    }
    finally {
        $reader.Dispose()
    }
}

function New-TestAquaticAssets {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $visualCount = [BitConverter]::ToUInt32($registryBytes, 16)
    $bytes = [byte[]]::new(200 + 40 * $visualCount)
    [Text.Encoding]::ASCII.GetBytes('MCBEAS04').CopyTo($bytes, 0)
    [BitConverter]::GetBytes([uint32]4).CopyTo($bytes, 8)
    [BitConverter]::GetBytes([uint32]16).CopyTo($bytes, 12)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, 16)
    [BitConverter]::GetBytes([uint32]$visualCount).CopyTo($bytes, 20)
    [BitConverter]::GetBytes([uint64]200).CopyTo($bytes, 96)
    $aquaticEntries = @(Get-TestRegistryEntries -RegistryPath $RegistryPath | Where-Object {
        $_.family -eq 27 -and $_.name -in @('minecraft:seagrass', 'minecraft:kelp')
    })
    Assert-Equal 29 $aquaticEntries.Count 'test fixture did not find the exact seagrass+kelp state set'
    foreach ($entry in $aquaticEntries) {
        $offset = 200 + 40 * [int]$entry.sequential_id
        $bytes[$offset + 25] = if ($entry.name -ceq 'minecraft:seagrass') { 2 } else { 3 }
        [BitConverter]::GetBytes([uint32]0).CopyTo($bytes, $offset + 28)
    }
    [IO.File]::WriteAllBytes($Path, $bytes)
}

function New-TestBdsFixtureResultLines {
    param([Parameter(Mandatory = $true)][string[]]$Commands)

    $lines = [Collections.Generic.List[string]]::new()
    foreach ($command in $Commands) {
        if ($command -match '^setblock ') {
            $lines.Add('[2026-07-11 12:00:00:000 INFO] Block placed')
            continue
        }
        if ($command -notmatch '^fill (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) ') {
            throw "test helper cannot model fixture command: $command"
        }
        $volume = ([Math]::Abs([int]$Matches[4] - [int]$Matches[1]) + 1) *
            ([Math]::Abs([int]$Matches[5] - [int]$Matches[2]) + 1) *
            ([Math]::Abs([int]$Matches[6] - [int]$Matches[3]) + 1)
        $lines.Add("[2026-07-11 12:00:00:000 INFO] $volume blocks filled")
    }
    return @($lines)
}

function New-TestBdsMarkerEvidence {
    param(
        [Parameter(Mandatory = $true)][string]$Line,
        [AllowEmptyCollection()][string[]]$SkippedLines = @()
    )

    return [pscustomobject][ordered]@{
        Line = $Line
        SkippedLines = @($SkippedLines)
    }
}

function ConvertTo-TestCommandArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Format-TestResolvedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments
    )

    $parts = @((ConvertTo-TestCommandArgument $Executable))
    $parts += @($Arguments | ForEach-Object { ConvertTo-TestCommandArgument $_ })
    return $parts -join ' '
}

function Invoke-Acceptance {
    param([string[]]$Arguments)
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $output = & powershell -NoProfile -ExecutionPolicy Bypass -File $script:AcceptanceScript @Arguments 2>&1
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }
    [pscustomobject]@{
        ExitCode = $LASTEXITCODE
        Output = @($output | ForEach-Object { $_.ToString() })
    }
}

function Complete-TestLoggedProcess {
    param(
        $Handle,
        [ValidateSet('app', 'core', 'bds')][string]$Kind = 'core'
    )

    if ($null -eq $Handle) {
        return
    }

    $cleanupFailures = [Collections.Generic.List[string]]::new()
    try {
        if (-not $Handle.Process.HasExited) {
            Stop-BoundedProcess -Handle $Handle -Kind $Kind
        }
        if (-not $Handle.Process.WaitForExit(10000)) {
            throw 'test helper remained alive after bounded cleanup'
        }
    }
    catch {
        $cleanupFailures.Add("bounded process cleanup failed: $($_.Exception.Message)")
        try {
            if (-not $Handle.Process.HasExited) {
                $Handle.Process.Kill()
            }
            if (-not $Handle.Process.WaitForExit(10000)) {
                throw 'test helper remained alive after forced termination'
            }
        }
        catch {
            $cleanupFailures.Add("forced process cleanup failed: $($_.Exception.Message)")
        }
    }

    try {
        Complete-ProcessLogs $Handle
    }
    catch {
        $cleanupFailures.Add("log cleanup failed: $($_.Exception.Message)")
        foreach ($stream in @($Handle.StdoutStream, $Handle.StderrStream)) {
            try {
                $stream.Dispose()
            }
            catch {
                $cleanupFailures.Add("fallback log stream disposal failed: $($_.Exception.Message)")
            }
        }
    }
    finally {
        try {
            $Handle.Process.Dispose()
        }
        catch {
            $cleanupFailures.Add("process disposal failed: $($_.Exception.Message)")
        }
    }

    if ($cleanupFailures.Count -ne 0) {
        throw "test logged-process cleanup failed: $($cleanupFailures -join '; ')"
    }
}

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$AcceptanceScript = Join-Path $ProjectRoot 'scripts\acceptance.ps1'
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("rust-mcbe acceptance tests {0}" -f [guid]::NewGuid().ToString('N'))
$BdsDir = Join-Path $TempRoot 'bds source'
$MetricsOut = Join-Path $TempRoot 'metrics output\metrics.json'
$Assets = Join-Path $TempRoot 'vanilla assets with spaces.mcpack'
$CrossCropAssets = Join-Path $TempRoot 'compiled cross crop assets.mcbea'
$AquaticAssets = Join-Path $TempRoot 'compiled aquatic assets.mcbea'
$SlabStairAssets = Join-Path $TempRoot 'compiled slab stair assets.mcbea'
$BlockRegistry = Join-Path $ProjectRoot 'crates\assets\data\block-registry-v1001.bin'
$PrebuiltClient = Join-Path $TempRoot 'opaque base client\bedrock-client.exe'
$DryRunDirectory = Join-Path $ProjectRoot '.local\acceptance\dry-run'
$testFailure = $null
$tempRootCleanupFailure = $null

try {
    New-Item -ItemType Directory -Path $BdsDir -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $BdsDir 'bedrock_server.exe') -Value 'fixture' -NoNewline
    Set-Content -LiteralPath $Assets -Value 'assets fixture' -NoNewline
    New-TestCrossCropAssets -RegistryPath $BlockRegistry -Path $CrossCropAssets
    New-TestAquaticAssets -RegistryPath $BlockRegistry -Path $AquaticAssets
    New-TestSlabStairAssets -RegistryPath $BlockRegistry -Path $SlabStairAssets
    New-Item -ItemType Directory -Path (Split-Path -Parent $PrebuiltClient) -Force | Out-Null
    Set-Content -LiteralPath $PrebuiltClient -Value 'pinned opaque client fixture' -NoNewline
    $prebuiltHashBefore = (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) "pre-existing dry-run artifact prevents an immutability assertion: $DryRunDirectory"
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $BdsDir 'worlds'))) 'generic dry-run fixture unexpectedly contains a pre-created BDS world'

    $success = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($success.ExitCode -eq 0) "dry-run failed: $($success.Output -join [Environment]::NewLine)"
    $commands = @($success.Output | Where-Object { $_ -match '^(BDS|CORE|APP)_COMMAND=' })
    Assert-True ($commands.Count -eq 3) "expected exactly three commands, got $($commands.Count)"
    Assert-True ($commands[0] -match '^BDS_COMMAND=') 'BDS command was not first'
    Assert-True ($commands[1] -match '^CORE_COMMAND=') 'core command was not second'
    Assert-True ($commands[2] -match '^APP_COMMAND=') 'app command was not third'
    Assert-True ($success.Output.Count -eq 3) 'default dry-run output changed'
    $expectedRuntimeDirectory = Join-Path (Join-Path $ProjectRoot '.local\bds-runtime') (Split-Path -Leaf $BdsDir)
    $expectedSocketDirectory = Join-Path $DryRunDirectory 'socket'
    $expectedCanonicalMetrics = Join-Path $DryRunDirectory 'app-metrics.json'
    $expectedCommands = @(
        ('BDS_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $expectedRuntimeDirectory 'bedrock_server.exe') `
            -Arguments @()))
        ('CORE_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $ProjectRoot 'target\release\bedrock-core.exe') `
            -Arguments @('-socket-dir', $expectedSocketDirectory, '-upstream', '127.0.0.1:19132')))
        ('APP_COMMAND=' + (Format-TestResolvedCommand `
            -Executable (Join-Path $ProjectRoot 'target\release\bedrock-client.exe') `
            -Arguments @(
                '--socket-dir', $expectedSocketDirectory,
                '--acceptance-seconds', '900',
                '--metrics-out', $expectedCanonicalMetrics,
                '--auto-fly',
                '--no-vsync'
            )))
    )
    Assert-Equal `
        ($expectedCommands -join [Environment]::NewLine) `
        ($commands -join [Environment]::NewLine) `
        'default dry-run commands changed'
    foreach ($flag in @('--socket-dir', '--acceptance-seconds 900', '--metrics-out', '--auto-fly', '--no-vsync')) {
        Assert-True ($commands[2].Contains($flag)) "app command is missing $flag"
    }
    Assert-True (-not $commands[2].Contains('--assets')) 'default app command unexpectedly gained --assets'
    Assert-True (-not ($success.Output -match '^VISUAL_FIXTURE_POSE=')) 'default dry-run recorded a fixture pose'
    Assert-True ($commands[0].Contains('"')) 'path containing spaces was not quoted'
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) 'dry-run created its run directory'
    Assert-True (-not (Test-Path -LiteralPath $MetricsOut)) 'dry-run wrote metrics'

    $frontDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-VisualFixturePose', 'Front'
    )
    Assert-True ($frontDryRun.ExitCode -eq 0) "front fixture dry-run failed: $($frontDryRun.Output -join [Environment]::NewLine)"
    $frontAppCommand = @($frontDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($frontAppCommand.Count -eq 1) 'front fixture dry-run did not emit one app command'
    Assert-True ($frontAppCommand[0].Contains("--assets `"$((Resolve-Path -LiteralPath $Assets).Path)`"")) 'front fixture app command did not include the resolved assets path'
    Assert-True (-not $frontAppCommand[0].Contains('--auto-fly')) 'front fixture app command retained --auto-fly'
    Assert-True ($frontAppCommand[0].Contains('--no-vsync')) 'front fixture app command lost --no-vsync'
    Assert-True ($frontDryRun.Output -contains 'VISUAL_FIXTURE_POSE=Front') 'front fixture dry-run did not record its pose'

    $backDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-VisualFixturePose', 'Back'
    )
    Assert-True ($backDryRun.ExitCode -eq 0) "back fixture dry-run failed: $($backDryRun.Output -join [Environment]::NewLine)"
    Assert-True ($backDryRun.Output -contains 'VISUAL_FIXTURE_POSE=Back') 'back fixture dry-run did not record its pose'

    $teleportDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $CrossCropAssets,
        '-FullViewTeleportGate'
    )
    Assert-True ($teleportDryRun.ExitCode -eq 0) "full-view dry-run failed: $($teleportDryRun.Output -join [Environment]::NewLine)"
    $teleportAppCommand = @($teleportDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $teleportAppCommand.Count 'full-view dry-run did not emit one app command'
    Assert-True ($teleportAppCommand[0].Contains('--full-view-teleport-gate')) 'full-view app command omitted its gate flag'
    Assert-True ($teleportAppCommand[0].Contains('--frame-cap 60')) 'full-view app command omitted the deterministic 60fps cap'
    Assert-True (-not $teleportAppCommand[0].Contains('--auto-fly')) 'full-view app command retained auto-fly'
    Assert-True (-not $teleportAppCommand[0].Contains('--no-vsync')) 'full-view app command bypassed its capped presentation mode'
    Assert-True ($teleportDryRun.Output -contains 'FULL_VIEW_TELEPORT_GATE=1') 'full-view dry-run did not record its mode'

    $leafFrontDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-VisualFixturePose', 'LeafGalleryFront',
        '-SteadyResourceTrigger', 'VisualFixtureReady',
        '-UseVsync'
    )
    Assert-True ($leafFrontDryRun.ExitCode -eq 0) "leaf-front dry-run failed: $($leafFrontDryRun.Output -join [Environment]::NewLine)"
    $leafFrontAppCommand = @($leafFrontDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $leafFrontAppCommand.Count 'leaf-front dry-run did not emit one app command'
    Assert-True (-not $leafFrontAppCommand[0].Contains('--auto-fly')) 'leaf-front mode retained auto-fly'
    Assert-True (-not $leafFrontAppCommand[0].Contains('--no-vsync')) 'leaf-front mode bypassed -UseVsync'
    Assert-True (-not $leafFrontAppCommand[0].Contains('--full-view-teleport-gate')) 'leaf-front mode armed the far tracker'
    Assert-True ($leafFrontDryRun.Output -contains 'VISUAL_FIXTURE_POSE=LeafGalleryFront') 'leaf-front dry-run lost its pose'
    Assert-True ($leafFrontDryRun.Output -contains 'STEADY_RESOURCE_TRIGGER=VisualFixtureReady') 'leaf-front dry-run lost its trigger'
    Assert-True ($leafFrontDryRun.Output -contains 'USE_VSYNC=1') 'leaf-front dry-run lost its vsync mode'

    $crossCropDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $CrossCropAssets,
        '-VisualFixturePose', 'CrossCropGalleryFront',
        '-SteadyResourceTrigger', 'VisualFixtureReady',
        '-UseVsync'
    )
    Assert-True ($crossCropDryRun.ExitCode -eq 0) "cross/crop gallery dry-run failed: $($crossCropDryRun.Output -join [Environment]::NewLine)"
    $assetIdentity = (Get-FileHash -Algorithm SHA256 -LiteralPath $CrossCropAssets).Hash.ToLowerInvariant()
    Assert-True ($crossCropDryRun.Output -contains 'VISUAL_FIXTURE_POSE=CrossCropGalleryFront') 'cross/crop dry-run lost its exact gallery argument'
    Assert-True ($crossCropDryRun.Output -contains "CROSS_CROP_GALLERY_ASSETS_SHA256=$assetIdentity") 'cross/crop dry-run did not record exact artifact identity'
    Assert-Equal 1 @($crossCropDryRun.Output | Where-Object { $_ -match '^CROSS_CROP_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count 'cross/crop dry-run did not record deterministic gallery arguments identity'

    $aquaticDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $AquaticAssets,
        '-VisualFixturePose', 'AquaticGalleryFront',
        '-SteadyResourceTrigger', 'VisualFixtureReady',
        '-UseVsync'
    )
    Assert-True ($aquaticDryRun.ExitCode -eq 0) "aquatic gallery dry-run failed: $($aquaticDryRun.Output -join [Environment]::NewLine)"
    $aquaticAssetIdentity = (Get-FileHash -Algorithm SHA256 -LiteralPath $AquaticAssets).Hash.ToLowerInvariant()
    Assert-True ($aquaticDryRun.Output -contains 'VISUAL_FIXTURE_POSE=AquaticGalleryFront') 'aquatic dry-run lost its exact gallery argument'
    Assert-True ($aquaticDryRun.Output -contains "AQUATIC_GALLERY_ASSETS_SHA256=$aquaticAssetIdentity") 'aquatic dry-run did not record exact artifact identity'
    Assert-Equal 1 @($aquaticDryRun.Output | Where-Object { $_ -match '^AQUATIC_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count 'aquatic dry-run did not record deterministic gallery arguments identity'
    $aquaticAppCommand = @($aquaticDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($aquaticAppCommand[0] -notmatch '--require-transparent-presentation') 'non-water aquatic gallery unexpectedly required transparent presentation settle'

    $waterDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $AquaticAssets,
        '-VisualFixturePose', 'WaterGalleryFront',
        '-UseVsync',
        '-SteadyResourceTrigger', 'VisualFixtureReady'
    )
    Assert-True ($waterDryRun.ExitCode -eq 0) "water gallery dry-run failed: $($waterDryRun.Output -join [Environment]::NewLine)"
    $waterAppCommand = @($waterDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-True ($waterAppCommand[0] -match '--require-transparent-presentation') 'water gallery did not opt into bounded transparent presentation settle'
    Assert-True ($waterAppCommand[0] -match '--transparent-witness-request') 'water gallery did not pass its ignored-local transparent witness request path to the app'

    foreach ($flowerBedPose in @('FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite')) {
        $flowerBedDryRun = Invoke-Acceptance -Arguments @(
            '-DryRun',
            '-DurationSeconds', '60',
            '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut,
            '-Assets', $CrossCropAssets,
            '-VisualFixturePose', $flowerBedPose,
            '-UseVsync',
            '-SteadyResourceTrigger', 'VisualFixtureReady'
        )
        Assert-True ($flowerBedDryRun.ExitCode -eq 0) "$flowerBedPose dry-run failed: $($flowerBedDryRun.Output -join [Environment]::NewLine)"
        Assert-True ($flowerBedDryRun.Output -contains "VISUAL_FIXTURE_POSE=$flowerBedPose") "$flowerBedPose dry-run lost its exact pose"
        Assert-Equal 1 @($flowerBedDryRun.Output | Where-Object { $_ -match '^FLOWERBED_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count "$flowerBedPose dry-run did not emit deterministic arguments identity"
    }

    foreach ($slabStairPose in @('SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite')) {
        $slabStairDryRun = Invoke-Acceptance -Arguments @(
            '-DryRun', '-DurationSeconds', '60', '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut, '-Assets', $SlabStairAssets,
            '-VisualFixturePose', $slabStairPose, '-UseVsync',
            '-SteadyResourceTrigger', 'VisualFixtureReady'
        )
        Assert-True ($slabStairDryRun.ExitCode -eq 0) "$slabStairPose dry-run failed: $($slabStairDryRun.Output -join [Environment]::NewLine)"
        Assert-True ($slabStairDryRun.Output -contains "VISUAL_FIXTURE_POSE=$slabStairPose") "$slabStairPose lost exact pose"
        Assert-True ($slabStairDryRun.Output -contains 'STEADY_RESOURCE_TRIGGER=VisualFixtureReady') "$slabStairPose lost trigger"
        Assert-True ($slabStairDryRun.Output -contains 'USE_VSYNC=1') "$slabStairPose lost vsync"
        Assert-Equal 1 @($slabStairDryRun.Output | Where-Object { $_ -match '^SLAB_STAIR_GALLERY_ARGUMENTS_SHA256=[0-9a-f]{64}$' }).Count "$slabStairPose lost arguments hash"
        Assert-Equal 1 @($slabStairDryRun.Output | Where-Object { $_ -match '^SLAB_STAIR_GALLERY_ASSETS_SHA256=[0-9a-f]{64}$' }).Count "$slabStairPose lost assets hash"
        $slabStairAppCommand = @($slabStairDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
        Assert-True ($slabStairAppCommand[0] -match '--model-witness-request') "$slabStairPose did not pass its ignored-local model witness request path to the app"
    }

    $baselineDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '60',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-LeafForestBaseline',
        '-SteadyResourceTrigger', 'WorldReady',
        '-ClientExecutable', $PrebuiltClient,
        '-SkipClientBuild',
        '-UseVsync'
    )
    Assert-True ($baselineDryRun.ExitCode -eq 0) "leaf-forest baseline dry-run failed: $($baselineDryRun.Output -join [Environment]::NewLine)"
    $baselineAppCommand = @($baselineDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $baselineAppCommand.Count 'baseline dry-run did not emit one app command'
    Assert-True ($baselineAppCommand[0].StartsWith('APP_COMMAND=' + (ConvertTo-TestCommandArgument ((Resolve-Path -LiteralPath $PrebuiltClient).Path)))) 'baseline did not select the exact prebuilt executable'
    Assert-True (-not $baselineAppCommand[0].Contains('--auto-fly')) 'baseline retained auto-fly'
    Assert-True (-not $baselineAppCommand[0].Contains('--no-vsync')) 'baseline bypassed -UseVsync'
    Assert-True (-not $baselineAppCommand[0].Contains('--full-view-teleport-gate')) 'baseline armed the far tracker'
    foreach ($marker in @(
        'LEAF_FOREST_BASELINE=1',
        'STEADY_RESOURCE_TRIGGER=WorldReady',
        'SKIP_CLIENT_BUILD=1',
        'USE_VSYNC=1'
    )) {
        Assert-True ($baselineDryRun.Output -contains $marker) "baseline dry-run omitted $marker"
    }
    Assert-Equal $prebuiltHashBefore (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash 'dry-run overwrote the explicit prebuilt client'

    $leafForestFullViewDryRun = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $Assets,
        '-LeafForestFullView',
        '-FullViewTeleportGate',
        '-SteadyResourceTrigger', 'FullViewPresented'
    )
    Assert-True ($leafForestFullViewDryRun.ExitCode -eq 0) "leaf-forest full-view dry-run failed: $($leafForestFullViewDryRun.Output -join [Environment]::NewLine)"
    $leafForestFullViewAppCommand = @($leafForestFullViewDryRun.Output | Where-Object { $_ -match '^APP_COMMAND=' })
    Assert-Equal 1 $leafForestFullViewAppCommand.Count 'leaf-forest full-view emitted the wrong app-command count'
    Assert-True ($leafForestFullViewAppCommand[0].Contains('--full-view-teleport-gate --frame-cap 60')) 'leaf-forest full-view lost the binding capped mode'
    Assert-True (-not $leafForestFullViewAppCommand[0].Contains('--no-vsync')) 'leaf-forest full-view added no-vsync'
    Assert-True ($leafForestFullViewDryRun.Output -contains 'LEAF_FOREST_FULL_VIEW=1') 'leaf-forest full-view lost its mode marker'
    Assert-True ($leafForestFullViewDryRun.Output -contains 'STEADY_RESOURCE_TRIGGER=FullViewPresented') 'leaf-forest full-view lost its trigger marker'

    $invalidLeafModes = @(
        @('-LeafForestBaseline', '-LeafForestFullView', '-FullViewTeleportGate', '-SteadyResourceTrigger', 'WorldReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild', '-UseVsync'),
        @('-LeafForestBaseline', '-SteadyResourceTrigger', 'WorldReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild'),
        @('-LeafForestBaseline', '-SteadyResourceTrigger', 'VisualFixtureReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild', '-UseVsync'),
        @('-LeafForestFullView', '-SteadyResourceTrigger', 'FullViewPresented'),
        @('-LeafForestFullView', '-FullViewTeleportGate', '-SteadyResourceTrigger', 'WorldReady'),
        @('-VisualFixturePose', 'LeafGalleryBack', '-SteadyResourceTrigger', 'VisualFixtureReady'),
        @('-VisualFixturePose', 'LeafGalleryBack', '-SteadyResourceTrigger', 'WorldReady', '-UseVsync'),
        @('-VisualFixturePose', 'LeafGalleryFront', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync'),
        @('-VisualFixturePose', 'leafgalleryfront', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $Assets),
        @('-VisualFixturePose', 'slabstairgallerytop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $SlabStairAssets),
        @('-VisualFixturePose', 'SlabStairGalleryTop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync'),
        @('-VisualFixturePose', 'SlabStairGalleryTop', '-SteadyResourceTrigger', 'VisualFixtureReady', '-UseVsync', '-Assets', $Assets),
        @('-SteadyResourceTrigger', 'worldready'),
        @('-LeafForestBaseline', '-SteadyResourceTrigger', 'WorldReady', '-ClientExecutable', $PrebuiltClient, '-SkipClientBuild', '-UseVsync'),
        @('-LeafForestFullView', '-FullViewTeleportGate', '-SteadyResourceTrigger', 'FullViewPresented'),
        @('-SkipClientBuild'),
        @('-ClientExecutable', $PrebuiltClient),
        @('-SkipClientBuild', '-ClientExecutable', (Join-Path $TempRoot 'missing-client.exe'))
    )
    foreach ($invalidLeafMode in $invalidLeafModes) {
        $invalidArguments = @(
            '-DryRun',
            '-DurationSeconds', '900',
            '-BdsDir', $BdsDir,
            '-MetricsOut', $MetricsOut
        ) + $invalidLeafMode
        $invalidResult = Invoke-Acceptance -Arguments $invalidArguments
        Assert-True ($invalidResult.ExitCode -ne 0) "invalid leaf/prebuilt mode was accepted: $($invalidLeafMode -join ' ')"
    }

    $conflictingModes = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-FullViewTeleportGate',
        '-VisualFixturePose', 'Front'
    )
    Assert-True ($conflictingModes.ExitCode -ne 0) 'full-view and visual-fixture modes were accepted together'

    $missingAssets = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', (Join-Path $TempRoot 'missing.mcpack')
    )
    Assert-True ($missingAssets.ExitCode -ne 0) 'missing assets file was accepted'

    $directoryAssets = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut,
        '-Assets', $BdsDir
    )
    Assert-True ($directoryAssets.ExitCode -ne 0) 'assets directory was accepted as a file'

    $short = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '59',
        '-BdsDir', $BdsDir,
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($short.ExitCode -ne 0) 'duration below 60 seconds was accepted'

    $missing = Invoke-Acceptance -Arguments @(
        '-DryRun',
        '-DurationSeconds', '900',
        '-BdsDir', (Join-Path $TempRoot 'missing'),
        '-MetricsOut', $MetricsOut
    )
    Assert-True ($missing.ExitCode -ne 0) 'missing BDS directory was accepted'

    $source = Get-Content -Raw -LiteralPath $AcceptanceScript
    $identityFunctionStart = $source.IndexOf('function Get-BdsSourceWorldIdentity {', [StringComparison]::Ordinal)
    $identityFunctionEnd = $source.IndexOf('function Assert-BdsSourceWorldIdentityUnchanged {', $identityFunctionStart, [StringComparison]::Ordinal)
    $identityFunctionSource = $source.Substring($identityFunctionStart, $identityFunctionEnd - $identityFunctionStart)
    Assert-True (-not $identityFunctionSource.Contains('Test-Path -LiteralPath $worldsPath')) 'optional source identity still treats Test-Path false as proof that worlds is absent'
    Assert-True (-not $identityFunctionSource.Contains('Test-Path -LiteralPath $worldPath')) 'optional source identity still treats Test-Path false as proof that the configured world is absent'
    Assert-True ($source.Contains('CopyToAsync')) 'child logs are not streamed directly to files'
    $runtimeSnapshotIndex = $source.IndexOf('$runtimeWorldIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $RuntimeDirectory', [StringComparison]::Ordinal)
    $bdsLaunchIndex = $source.IndexOf('$bdsHandle = Start-LoggedProcess -Executable $BdsExecutable', [StringComparison]::Ordinal)
    Assert-True ($runtimeSnapshotIndex -ge 0 -and $runtimeSnapshotIndex -lt $bdsLaunchIndex) 'runtime world identity is captured after BDS can lock level.dat'
    Assert-True ($source.Contains('Get-BdsSourceWorldIdentity -SourceDirectory $RuntimeDirectory -AllowMissingWorld')) 'fresh BDS runtime world detection is not optional before bootstrap'
    Assert-True ($source.Contains("`$metadata['runtime_world_bootstrapped'] = `$true")) 'fresh BDS runtime bootstrap is not recorded in metadata'
    Assert-True ($source.Contains("'bds-bootstrap.stdout.log'")) 'fresh BDS runtime has no isolated bootstrap process evidence'
    Assert-True ($source.Contains('[IO.FileOptions]::WriteThrough')) 'child log files are not write-through'
    Assert-True (-not $source.Contains('ReadToEndAsync')) 'child logs are retained in memory'
    Assert-True ($source.Contains('-WorkingDirectory $ProjectRoot')) 'builds are not rooted at the project directory'
    Assert-True ($source.Contains("'9948b1729395d2e819fce28e079d4a7bfc67716c'")) 'gophertunnel metadata commit is not the repository pin'
    Assert-True ($source.Contains("'6cd8087fc3f0b500e41708a8afc94a0fa3291525'")) 'Valentine metadata commit is not the compiled fork revision'
    Assert-True ($source.Contains('Assert-ProtocolDependencyProvenance')) 'acceptance metadata does not detect Cargo/provenance drift'
    Assert-True (-not $source.Contains("'^RUST_MCBE_TELEPORT_SETTLED ms=")) 'live teleport path still assumes ms precedes target'
    Assert-True (-not $source.Contains("'^RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED ms=")) 'live forced-remesh path still assumes ms precedes target'
    Assert-True ($source.Contains('TeleportMarker = $teleportMarkerEvidence')) 'live metrics validation does not receive parsed teleport evidence'
    Assert-True ($source.Contains('ForcedRemeshMarker = $forcedRemeshMarkerEvidence')) 'live metrics validation does not receive parsed forced-remesh evidence'
    Assert-True ($source.Contains('ExpectedTargetCohort = $expectedTargetCohort')) 'live metrics validation does not receive the planned target cohort'
    Assert-True ($source.Contains('SteadyResourceArtifactPath = $steadyResourceArtifactPath')) 'live metrics validation does not require the steady-resource artifact'
    Assert-True (([regex]::Matches($source, '-TeleportMarker \$teleportMarkerEvidence')).Count -ge 1) 'steady-resource sampler does not receive teleport trigger evidence'
    Assert-True (([regex]::Matches($source, '-ForcedRemeshMarker \$forcedRemeshMarkerEvidence')).Count -ge 1) 'steady-resource sampler does not receive forced-remesh trigger evidence'
    Assert-True ($source.Contains('if (-not $SkipClientBuild)')) 'prebuilt client mode does not skip the app build'
    Assert-True ($source.Contains('RUST_MCBE_TARGET_MUTATION_ARMED ')) 'live harness does not wait for the target-mutation arming marker'
    Assert-True ($source.Contains('ConvertFrom-TargetMutationArmedMarker')) 'live harness does not parse target-mutation evidence'
    Assert-True ($source.Contains('RUST_MCBE_MOVE_PLAYER_INGRESS ')) 'live harness does not wait for binding MovePlayer ingress evidence'
    Assert-True ($source.Contains('ConvertFrom-MovePlayerIngressMarker')) 'live harness does not parse binding MovePlayer ingress evidence'
    Assert-True ($source.Contains('-PassThruEvidence')) 'binding marker waits do not retain stdout positions'
    Assert-True ($source.Contains('Write-AcceptanceEvent')) 'live harness does not persist ordered fixture/teleport events'
    Assert-True `
        ([regex]::IsMatch(
            $source,
            'if \(\$isLeafEvidence\) \{\s*\$sourceWorldIdentity = Get-BdsSourceWorldIdentity',
            [Text.RegularExpressions.RegexOptions]::CultureInvariant
        )) `
        'generic live smoke runs still require a pre-created source world identity'
    Assert-True ($source.Contains('Move-Item -LiteralPath $temporaryPath -Destination $Path')) 'fixture manifest publication is not an atomic sibling rename'
    Assert-True ($source.Contains('$cpuPercent = 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount)')) 'steady CPU normalization formula changed'
    Assert-True (([regex]::Matches($source, '\.Refresh\(\)')).Count -ge 4) 'resource sampling does not refresh both process handles before/during sampling'
    $baselineSourceMutationIndex = $source.IndexOf('$baselineSourceMutationCommand = Publish-BaselineSourceMutation', [StringComparison]::Ordinal)
    $resourceSamplingIndex = $source.IndexOf('$resourceDocument = Measure-SteadyResources', [StringComparison]::Ordinal)
    $baselineForestPlanIndex = $source.IndexOf(
        '$baselineForestPlan = New-LeafForestPlan -MutationCoordinate $coordinate -Mode Baseline',
        [Math]::Max(0, $baselineSourceMutationIndex),
        [StringComparison]::Ordinal
    )
    $baselinePreloadIndex = $source.IndexOf(
        '$null = Start-BdsFixtureLoadArea',
        [Math]::Max(0, $baselineForestPlanIndex),
        [StringComparison]::Ordinal
    )
    $baselineForestPublishIndex = $source.IndexOf('$fixturePlan = $baselineForestPlan', [StringComparison]::Ordinal)
    Assert-True ($baselineSourceMutationIndex -ge 0 -and $resourceSamplingIndex -gt $baselineSourceMutationIndex) 'baseline did not issue its source mutation immediately before the WorldReady observation window'
    Assert-True ($baselineForestPlanIndex -gt $baselineSourceMutationIndex) 'baseline did not derive its exact far preload plan after source mutation'
    Assert-True ($baselinePreloadIndex -gt $baselineForestPlanIndex -and $baselinePreloadIndex -lt $resourceSamplingIndex) 'baseline did not start its ticking-area preload before the 30s WorldReady observation window'
    Assert-True `
        ([regex]::IsMatch(
            $source.Substring($baselinePreloadIndex, $resourceSamplingIndex - $baselinePreloadIndex),
            'Start-BdsFixtureLoadArea[\s\S]*?-SettleMilliseconds 0',
            [Text.RegularExpressions.RegexOptions]::CultureInvariant
        )) `
        'baseline added an extra preload settle instead of using the existing 30s WorldReady sample'
    Assert-True ($baselineForestPublishIndex -gt $resourceSamplingIndex) 'baseline far forest could publish before the source mutation observation window'
    $metricsValidationIndex = $source.IndexOf('$metrics = Assert-AcceptanceMetrics', [StringComparison]::Ordinal)
    Assert-True ($resourceSamplingIndex -ge 0 -and $metricsValidationIndex -gt $resourceSamplingIndex) 'full-view metrics SLA validation can run before steady-resource sampling/artifact publication'
    $cleanupFailureThrowIndex = $source.LastIndexOf('throw "acceptance cleanup failed:', [StringComparison]::Ordinal)
    $passedStatusIndex = $source.LastIndexOf('$metadata[''status''] = ''passed''', [StringComparison]::Ordinal)
    $successArtifactOutputIndex = $source.LastIndexOf('Write-Output "ACCEPTANCE_ARTIFACTS=', [StringComparison]::Ordinal)
    Assert-True ($cleanupFailureThrowIndex -ge 0) 'main finalizer omitted its cleanup-failure barrier'
    Assert-True ($passedStatusIndex -gt $cleanupFailureThrowIndex) 'metadata could claim passed before required cleanup/source verification succeeded'
    Assert-True ($successArtifactOutputIndex -gt $cleanupFailureThrowIndex) 'acceptance success markers could be emitted before required cleanup/source verification succeeded'

    $env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY = '1'
    try {
        . $AcceptanceScript `
            -DryRun `
            -DurationSeconds 900 `
            -BdsDir $BdsDir `
            -MetricsOut $MetricsOut `
            -Assets $Assets `
            -VisualFixturePose Front
    }
    finally {
        Remove-Item Env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -ErrorAction SilentlyContinue
    }

    $crossCropPlan = New-CrossCropGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose CrossCropGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $CrossCropAssets
    $aquaticPlan = New-AquaticGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose AquaticGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $AquaticAssets
    $waterPlan = New-WaterGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose WaterGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $AquaticAssets
    $flowerBedPlans = @('FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite') | ForEach-Object {
        New-FlowerBedGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose $_ -RegistryPath $BlockRegistry
    }
    # slab_stair_gallery_covers_all_variants
    $slabStairPlans = @('SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite') | ForEach-Object {
        New-SlabStairGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose $_ -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    }
    $unsealedSlabStairAssets = Join-Path $TempRoot 'unsealed covered visual mutation.mcbea'
    $unsealedBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
    $firstStairId = [int](@(Get-TestRegistryEntries -RegistryPath $BlockRegistry | Where-Object family -eq 8)[0].sequential_id)
    $coveredVisualOffset = [int]([BitConverter]::ToUInt64($unsealedBytes, 96) + 40 * $firstStairId)
    $unsealedBytes[$coveredVisualOffset] = $unsealedBytes[$coveredVisualOffset] -bxor 1
    [IO.File]::WriteAllBytes($unsealedSlabStairAssets, $unsealedBytes)
    Assert-ThrowsLike {
        Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $unsealedSlabStairAssets
    } 'MCBEAS04 slab/stair integrity SHA-256 mismatch*' 'slab/stair coverage accepted an unsealed covered-visual mutation'
    $oversizedSlabStairAssets = Join-Path $TempRoot 'oversized slab stair assets.mcbea'
    $oversizedStream = [IO.File]::Create($oversizedSlabStairAssets)
    try { $oversizedStream.SetLength(16 * 1024 * 1024 + 1) }
    finally { $oversizedStream.Dispose() }
    Assert-ThrowsLike {
        Get-StrictMcbeas04ModelTables -Path $oversizedSlabStairAssets
    } 'MCBEAS04 blob exceeds the app 16 MiB ceiling:*' 'slab/stair validation allocated an oversized blob before rejecting it'
    $kelpBackedSlabAssets = Join-Path $TempRoot 'resealed kelp backed slab.mcbea'
    $kelpBackedBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
    $firstSlabId = [int](@(Get-TestRegistryEntries -RegistryPath $BlockRegistry | Where-Object family -eq 7)[0].sequential_id)
    $firstSlabVisual = [int]([BitConverter]::ToUInt64($kelpBackedBytes, 96) + 40 * $firstSlabId)
    [BitConverter]::GetBytes([uint32]11).CopyTo($kelpBackedBytes, $firstSlabVisual + 28)
    Set-TestMcbeas04Seal -Bytes $kelpBackedBytes
    [IO.File]::WriteAllBytes($kelpBackedSlabAssets, $kelpBackedBytes)
    Assert-ThrowsLike {
        Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $kelpBackedSlabAssets
    } 'slab/stair compiled coverage contains diagnostic or malformed visuals:*' 'slab/stair coverage accepted a canonical kelp template for a slab visual'
    $templateOffset = [int][BitConverter]::ToUInt64([IO.File]::ReadAllBytes($SlabStairAssets), 120)
    $quadOffset = [int][BitConverter]::ToUInt64([IO.File]::ReadAllBytes($SlabStairAssets), 128)
    foreach ($malformedTemplate in @(
        [pscustomobject]@{ name = 'resealed 33-quad template'; pattern = 'MCBEAS04 model template 0 span or flags are noncanonical*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]33).CopyTo($bytes, $templateOffset + 4) } },
        [pscustomobject]@{ name = 'resealed out-of-range template span'; pattern = 'MCBEAS04 model template 10 span or flags are noncanonical*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]11).CopyTo($bytes, $templateOffset + 120) } },
        [pscustomobject]@{ name = 'resealed middle-of-stair-group visual'; pattern = 'MCBEAS04 stair visual * does not reference an exact group base or has reserved variant bits*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]2).CopyTo($bytes, $coveredVisualOffset + 28) } },
        [pscustomobject]@{ name = 'resealed malformed kelp sidedness'; pattern = 'MCBEAS04 kelp template 11 has noncanonical sidedness*'; mutate = { param($bytes) [BitConverter]::GetBytes([uint32]0).CopyTo($bytes, $quadOffset + 48 * 16 + 44) } }
    )) {
        $malformedPath = Join-Path $TempRoot ($malformedTemplate.name + '.mcbea')
        $malformedBytes = [IO.File]::ReadAllBytes($SlabStairAssets)
        & $malformedTemplate.mutate $malformedBytes
        Set-TestMcbeas04Seal -Bytes $malformedBytes
        [IO.File]::WriteAllBytes($malformedPath, $malformedBytes)
        Assert-ThrowsLike {
            Get-SlabStairCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $malformedPath
        } $malformedTemplate.pattern "slab/stair coverage accepted $($malformedTemplate.name)"
    }

    $expectedFlowerBedStates = [Collections.Generic.List[string]]::new()
    $expectedFlowerBedReferences = [Collections.Generic.List[string]]::new()
    $expectedFlowerBedStateCommands = [Collections.Generic.List[string]]::new()
    foreach ($name in @('minecraft:pink_petals', 'minecraft:wildflowers')) {
        foreach ($direction in @('south', 'west', 'north', 'east')) {
            foreach ($growth in 0..7) {
                $canonical = '{"growth":{"type":"int","value":' + $growth + '},"minecraft:cardinal_direction":{"type":"string","value":"' + $direction + '"}}'
                $stateIdentity = "$name|$canonical"
                $index = $expectedFlowerBedStates.Count
                $xOffset = -14 + 4 * ($index % 8)
                $zOffset = -10 + 3 * [Math]::Floor($index / 8)
                $expectedFlowerBedStates.Add($stateIdentity)
                $expectedFlowerBedReferences.Add("$stateIdentity|state=$xOffset,2,$zOffset|cube=$($xOffset + 1),2,$zOffset|minecraft:polished_andesite")
                $expectedFlowerBedStateCommands.Add("setblock $($xOffset + 100) 66 $($zOffset + 200) $name [`"growth`"=$growth,`"minecraft:cardinal_direction`"=`"$direction`"]")
            }
        }
    }
    Assert-Equal 64 $expectedFlowerBedStates.Count 'pinned flowerbed test matrix changed'
    $expectedFirstFlowerBedState = 'minecraft:pink_petals|{"growth":{"type":"int","value":0},"minecraft:cardinal_direction":{"type":"string","value":"south"}}'
    $expectedLastFlowerBedState = 'minecraft:wildflowers|{"growth":{"type":"int","value":7},"minecraft:cardinal_direction":{"type":"string","value":"east"}}'

    foreach ($flowerBedPlan in $flowerBedPlans) {
        Assert-Equal 'FlowerBedGallery' $flowerBedPlan.Manifest.fixture_kind 'flowerbed plan lost fixture kind'
        Assert-Equal 64 ([int]$flowerBedPlan.Manifest.gallery_state_count) 'flowerbed plan did not enumerate exactly 64 states'
        Assert-Equal 64 @($flowerBedPlan.Manifest.gallery_states | Sort-Object -Unique).Count 'flowerbed manifest states were not unique'
        Assert-Equal 64 @($flowerBedPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:(wildflowers|pink_petals) ' }).Count 'flowerbed plan did not issue exactly one placement per canonical state'
        Assert-Equal 64 @($flowerBedPlan.Manifest.reference_cubes).Count 'flowerbed plan did not pair every state with a reference cube'
        Assert-Equal 5 @($flowerBedPlan.Manifest.camera_poses.PSObject.Properties).Count 'flowerbed plan lost a fixed diagnostic camera'
        Assert-Equal 'a2fe82092cb22835a0553091ecfcdd67cedcddc9e791feb2d0ddeff9fe091f15' ([string]$flowerBedPlan.Manifest.coverage_evidence.state_set_sha256) 'flowerbed exact ordered BREG state-set identity drifted'
        Assert-Equal 'e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763' ([string]$flowerBedPlan.Manifest.fixture_layout_hash) 'flowerbed canonical layout identity drifted'
        Assert-Equal $expectedFirstFlowerBedState ([string]$flowerBedPlan.Manifest.gallery_states[0]) 'flowerbed first canonical identity drifted'
        Assert-Equal $expectedLastFlowerBedState ([string]$flowerBedPlan.Manifest.gallery_states[-1]) 'flowerbed last canonical identity drifted'
        Assert-Equal ($expectedFlowerBedStates -join "`n") (@($flowerBedPlan.Manifest.gallery_states) -join "`n") 'flowerbed exact ordered 64-state manifest drifted'
        $actualReferences = @($flowerBedPlan.Manifest.reference_cubes | ForEach-Object {
            "$($_.state)|state=$($_.state_offset -join ',')|cube=$($_.cube_offset -join ',')|$($_.cube)"
        })
        Assert-Equal ($expectedFlowerBedReferences -join "`n") ($actualReferences -join "`n") 'flowerbed state-to-grid/reference-cube identity drifted'
        $actualStateCommands = @($flowerBedPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:(wildflowers|pink_petals) ' })
        Assert-Equal ($expectedFlowerBedStateCommands -join "`n") ($actualStateCommands -join "`n") 'flowerbed typed state-to-world-coordinate command identity drifted'
        Assert-True ($flowerBedPlan.LoadAreaCommand -match '^tickingarea add ') 'flowerbed plan did not preload its bounded gallery'
        Assert-True ($flowerBedPlan.CleanupCommand -match '^tickingarea remove ') 'flowerbed plan did not provide ticking-area cleanup'
        Assert-Equal (Get-CanonicalObjectHash -Value $flowerBedPlan.Manifest.relative_layout) $flowerBedPlan.Manifest.fixture_layout_hash 'flowerbed layout hash was not derived from the complete relative layout'

        $growths = @($flowerBedPlan.CoverageEntries | ForEach-Object growth | Sort-Object -Unique)
        $directions = @($flowerBedPlan.CoverageEntries | ForEach-Object direction | Sort-Object -Unique)
        $names = @($flowerBedPlan.CoverageEntries | ForEach-Object name | Sort-Object -Unique)
        Assert-Equal '0,1,2,3,4,5,6,7' ($growths -join ',') 'flowerbed gallery growth coverage changed'
        Assert-Equal 'east,north,south,west' ($directions -join ',') 'flowerbed gallery cardinal coverage changed'
        Assert-Equal 'minecraft:pink_petals,minecraft:wildflowers' ($names -join ',') 'flowerbed gallery name coverage changed'
    }
    $oppositeFlowerBedPlan = @($flowerBedPlans | Where-Object { $_.Manifest.pose -ceq 'FlowerBedGalleryObliqueOpposite' })[0]
    Assert-Equal '38,28,38' (@(
        ([int]$oppositeFlowerBedPlan.Manifest.camera.position.x - [int]$oppositeFlowerBedPlan.Manifest.mutation.x)
        ([int]$oppositeFlowerBedPlan.Manifest.camera.position.y - [int]$oppositeFlowerBedPlan.Manifest.mutation.y)
        ([int]$oppositeFlowerBedPlan.Manifest.camera.position.z - [int]$oppositeFlowerBedPlan.Manifest.mutation.z)
    ) -join ',') 'flowerbed opposite oblique camera lost its symmetric offset'
    Assert-Equal '138,92,238' (@($oppositeFlowerBedPlan.Manifest.camera.position.x, $oppositeFlowerBedPlan.Manifest.camera.position.y, $oppositeFlowerBedPlan.Manifest.camera.position.z) -join ',') 'flowerbed opposite oblique camera lost its exact position'
    Assert-Equal 'tp @a[name=RustMCBE] 138 92 238 facing 100 66 200' ([string]$oppositeFlowerBedPlan.TeleportCommand) 'flowerbed opposite oblique camera command drifted'
    Assert-Equal 'e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763' ([string]$oppositeFlowerBedPlan.Manifest.fixture_layout_hash) 'flowerbed opposite oblique camera changed canonical layout identity'
    Assert-Equal 'a2fe82092cb22835a0553091ecfcdd67cedcddc9e791feb2d0ddeff9fe091f15' ([string]$oppositeFlowerBedPlan.Manifest.coverage_evidence.state_set_sha256) 'flowerbed opposite oblique camera changed exact state-set identity'
    Assert-Equal 1 @($flowerBedPlans | ForEach-Object { $_.Manifest.fixture_layout_hash } | Sort-Object -Unique).Count 'flowerbed camera pose changed canonical layout identity'
    $movedFlowerBedPlan = New-FlowerBedGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose FlowerBedGalleryTop -RegistryPath $BlockRegistry
    Assert-Equal $flowerBedPlans[0].Manifest.fixture_layout_hash $movedFlowerBedPlan.Manifest.fixture_layout_hash 'flowerbed absolute coordinate changed canonical layout identity'
    Assert-Equal ($flowerBedPlans[0].Manifest.gallery_states -join "`n") ($movedFlowerBedPlan.Manifest.gallery_states -join "`n") 'flowerbed BREG state manifest was not deterministic'
    $tamperedFlowerBedLayout = $flowerBedPlans[0].Manifest.relative_layout | ConvertTo-Json -Depth 12 | ConvertFrom-Json
    $tamperedFlowerBedLayout.spacing[0] = 5
    Assert-True ((Get-CanonicalObjectHash -Value $tamperedFlowerBedLayout) -cne 'e6eb62b75661d8de7508bbb40095e105301051d22462ef39f82f4226528ef763') 'flowerbed pinned layout hash did not detect spacing drift'
    $reorderedFlowerBedStates = @($expectedFlowerBedStates)
    [array]::Reverse($reorderedFlowerBedStates)
    Assert-True (($reorderedFlowerBedStates -join "`n") -cne ($flowerBedPlans[0].Manifest.gallery_states -join "`n")) 'flowerbed exact ordered manifest assertion cannot detect reordering'

    foreach ($slabStairPlan in $slabStairPlans) {
        Assert-Equal 'SlabStairGallery' $slabStairPlan.Manifest.fixture_kind 'slab/stair plan lost fixture kind'
        Assert-Equal 43 ([int]$slabStairPlan.Manifest.central_witness_count) 'slab/stair plan lost the 43 central witnesses'
        Assert-Equal 43 @($slabStairPlan.Manifest.witnesses).Count 'slab/stair witness inventory changed'
        Assert-Equal 3 @($slabStairPlan.Manifest.witnesses | Where-Object kind -ceq 'slab').Count 'slab variants changed'
        Assert-Equal 40 @($slabStairPlan.Manifest.witnesses | Where-Object kind -ceq 'stair').Count 'stair state matrix changed'
        Assert-Equal 5 @($slabStairPlan.Manifest.camera_poses.PSObject.Properties).Count 'slab/stair plan lost a fixed diagnostic camera'
        Assert-Equal 77 @($slabStairPlan.FixtureCommands).Count 'slab/stair fixture command bound changed'
        Assert-Equal 784 ([int]$slabStairPlan.Manifest.coverage_evidence.state_count) 'slab/stair coverage lost exact BREG state count'
        Assert-Equal 272 ([int]$slabStairPlan.Manifest.coverage_evidence.slab_state_count) 'slab coverage count drifted'
        Assert-Equal 512 ([int]$slabStairPlan.Manifest.coverage_evidence.stair_state_count) 'stair coverage count drifted'
        Assert-Equal 64 ([int]$slabStairPlan.Manifest.coverage_evidence.stair_name_count) 'stair identifier count drifted'
        Assert-Equal 0 ([int]$slabStairPlan.Manifest.coverage_evidence.diagnostic_slab_stair) 'slab/stair compiled coverage retained diagnostics'
        Assert-Equal '860f1e5629d7d6f390d554cedcef16546237f9f9df9f24a2abaa5a22c785fbc8' ([string]$slabStairPlan.Manifest.state_set_sha256) 'slab/stair exact state-set identity drifted'
        Assert-Equal '8c035c430d72ce4e62df32a99d126608e2b476bb155f941c89671500f91f4448' ([string]$slabStairPlan.Manifest.fixture_layout_hash) 'slab/stair canonical layout identity drifted'
        Assert-True ($slabStairPlan.LoadAreaCommand -match '^tickingarea add ') 'slab/stair gallery omitted bounded preload'
        Assert-True ($slabStairPlan.CleanupCommand -match '^tickingarea remove ') 'slab/stair gallery omitted ticking-area cleanup'
        Assert-Equal (Get-CanonicalObjectHash -Value $slabStairPlan.Manifest.relative_layout) $slabStairPlan.Manifest.fixture_layout_hash 'slab/stair layout hash was not derived from its complete relative layout'
        foreach ($half in @($false, $true)) {
            foreach ($orientation in @('south', 'west', 'north', 'east')) {
                foreach ($shape in @('straight', 'right_inner', 'left_inner', 'right_outer', 'left_outer')) {
                    Assert-Equal 1 @($slabStairPlan.Manifest.witnesses | Where-Object { $_.kind -ceq 'stair' -and $_.upside_down -eq $half -and $_.orientation -ceq $orientation -and $_.shape -ceq $shape }).Count "slab/stair matrix missing half=$half orientation=$orientation shape=$shape"
                }
            }
        }
        Assert-Equal 32 @($slabStairPlan.Manifest.witnesses | Where-Object { $_.kind -ceq 'stair' -and $null -ne $_.neighbor_offset }).Count 'corner witnesses lost isolated neighbours'
    }
    Assert-Equal 1 @($slabStairPlans | ForEach-Object { $_.Manifest.fixture_layout_hash } | Sort-Object -Unique).Count 'slab/stair camera pose changed canonical layout identity'
    Assert-Equal 1 @($slabStairPlans | ForEach-Object { $_.Manifest.state_set_sha256 } | Sort-Object -Unique).Count 'slab/stair camera pose changed exact state identity'
    $movedSlabStairPlan = New-SlabStairGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose SlabStairGalleryTop -RegistryPath $BlockRegistry -AssetsPath $SlabStairAssets
    Assert-Equal $slabStairPlans[0].Manifest.fixture_layout_hash $movedSlabStairPlan.Manifest.fixture_layout_hash 'slab/stair absolute coordinate changed canonical layout identity'
    Assert-Equal $slabStairPlans[0].Manifest.state_set_sha256 $movedSlabStairPlan.Manifest.state_set_sha256 'slab/stair absolute coordinate changed state identity'

    $freshSource = Join-Path $TempRoot 'fresh gallery source'
    $freshRuntime = Join-Path $TempRoot 'fresh gallery runtime'
    New-Item -ItemType Directory -Path $freshSource, (Join-Path $freshRuntime 'worlds\Bedrock level') -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $freshSource 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $freshRuntime 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $freshRuntime 'worlds\Bedrock level\level.dat') -Value 'fresh runtime world' -NoNewline
    $missingSourceIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $freshSource -AllowMissingWorld
    Assert-True ($null -eq $missingSourceIdentity) 'fresh BDS source unexpectedly reported a generated source world'
    $freshRuntimeIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $freshRuntime
    $freshPlan = New-CrossCropGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose CrossCropGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $CrossCropAssets
    Set-BdsSourceWorldIdentityOnPlan `
        -Plan $freshPlan `
        -Identity $missingSourceIdentity `
        -RuntimeIdentity $freshRuntimeIdentity
    Assert-True ($null -eq $freshPlan.Manifest.PSObject.Properties['source_world_identity']) 'fresh gallery mislabeled a runtime-created world as source evidence'
    Assert-Equal $freshRuntimeIdentity.sha256 $freshPlan.Manifest.runtime_world_identity.sha256 'fresh gallery did not bind the runtime-created world identity'
    $sourcePreferredPlan = New-CrossCropGalleryPlan `
        -MutationCoordinate @(100, 64, 200) `
        -Pose CrossCropGalleryFront `
        -RegistryPath $BlockRegistry `
        -AssetsPath $CrossCropAssets
    Set-BdsSourceWorldIdentityOnPlan `
        -Plan $sourcePreferredPlan `
        -Identity $freshRuntimeIdentity `
        -RuntimeIdentity $freshRuntimeIdentity
    Assert-Equal $freshRuntimeIdentity.sha256 $sourcePreferredPlan.Manifest.source_world_identity.sha256 'existing source identity did not take precedence'
    Assert-True ($null -eq $sourcePreferredPlan.Manifest.PSObject.Properties['runtime_world_identity']) 'source-backed gallery also recorded runtime identity on its plan'

    $brokenSource = Join-Path $TempRoot 'broken reparse gallery source'
    $brokenTarget = Join-Path $TempRoot 'broken reparse gallery target'
    New-Item -ItemType Directory -Path $brokenSource, $brokenTarget -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $brokenSource 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    $null = New-Item -ItemType Junction -Path (Join-Path $brokenSource 'worlds') -Target $brokenTarget
    Remove-Item -LiteralPath $brokenTarget -Force
    Assert-ThrowsLike {
        Get-BdsSourceWorldIdentity -SourceDirectory $brokenSource -AllowMissingWorld
    } '*worlds*' 'fresh-world allowance accepted a broken worlds reparse point'

    $malformedSource = Join-Path $TempRoot 'malformed gallery source'
    New-Item -ItemType Directory -Path $malformedSource -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $malformedSource 'server.properties') -Value 'level-name=Bedrock level' -Encoding ASCII
    Set-Content -LiteralPath (Join-Path $malformedSource 'worlds') -Value 'not a directory' -NoNewline
    Assert-ThrowsLike {
        Get-BdsSourceWorldIdentity -SourceDirectory $malformedSource -AllowMissingWorld
    } '*worlds*' 'fresh-world allowance accepted a malformed worlds entry'
    Assert-Equal 'CrossCropGallery' $crossCropPlan.Manifest.fixture_kind 'cross/crop plan lost fixture kind'
    Assert-Equal 411 ([int]$crossCropPlan.Manifest.gallery_state_count) 'cross/crop plan did not enumerate the exact tracked Cross/Crop state set after flowerbeds moved to family 31'
    Assert-Equal 0 ([int]$crossCropPlan.Manifest.family_diagnostics.cross) 'cross family diagnostic contract changed'
    Assert-Equal 0 ([int]$crossCropPlan.Manifest.family_diagnostics.crop) 'crop family diagnostic contract changed'
    Assert-Equal $assetIdentity ([string]$crossCropPlan.Manifest.artifact_identity.assets_sha256) 'cross/crop plan lost asset identity'
    Assert-Equal 413 $crossCropPlan.GalleryCommands.Count 'cross/crop gallery command coverage is not one command per tracked state plus bounded setup'
    Assert-True (-not (($crossCropPlan.GalleryCommands -join "`n") -match 'seagrass|kelp')) 'Task 9 gallery included Task 10 aquatic plants'
    $firstPlan = $crossCropPlan.Manifest | ConvertTo-Json -Compress -Depth 12
    $secondPlan = (New-CrossCropGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose CrossCropGalleryFront -RegistryPath $BlockRegistry -AssetsPath $CrossCropAssets).Manifest | ConvertTo-Json -Compress -Depth 12
    Assert-Equal $firstPlan $secondPlan 'cross/crop gallery arguments were not deterministic'
    $backPlan = New-CrossCropGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose CrossCropGalleryBack -RegistryPath $BlockRegistry -AssetsPath $CrossCropAssets
    $movedPlan = New-CrossCropGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose CrossCropGalleryFront -RegistryPath $BlockRegistry -AssetsPath $CrossCropAssets
    Assert-Equal $crossCropPlan.Manifest.fixture_layout_hash $backPlan.Manifest.fixture_layout_hash 'front/back capture pose changed the fixture layout identity'
    Assert-Equal $crossCropPlan.Manifest.fixture_layout_hash $movedPlan.Manifest.fixture_layout_hash 'absolute mutation coordinate changed the fixture layout identity'
    Assert-Equal (Get-CanonicalObjectHash -Value $crossCropPlan.Manifest.relative_layout) $crossCropPlan.Manifest.fixture_layout_hash 'fixture layout hash is not the complete canonical relative layout descriptor'
    foreach ($plan in @($crossCropPlan, $backPlan)) {
        $cameraDistance = [Math]::Abs([double]$plan.Manifest.camera.position.z - [double]$plan.Manifest.gallery_center.z)
        $nearDepth = $cameraDistance - 18.0
        $requiredHorizontalFov = 2.0 * [Math]::Atan(23.0 / $nearDepth) * 180.0 / [Math]::PI
        Assert-True ($requiredHorizontalFov -le 60.0) "$($plan.Pose) cannot frame every exhaustive gallery column within the 60-degree horizontal-FOV contract"
    }

    $tamperedAssets = Join-Path $TempRoot 'tampered cross crop assets.mcbea'
    [IO.File]::WriteAllBytes($tamperedAssets, [IO.File]::ReadAllBytes($CrossCropAssets))
    $firstCrossCropId = [int]$crossCropPlan.CoverageEntries[0].sequential_id
    $tamperedBytes = [IO.File]::ReadAllBytes($tamperedAssets)
    $tamperedBytes[200 + 40 * $firstCrossCropId + 25] = 0
    [IO.File]::WriteAllBytes($tamperedAssets, $tamperedBytes)
    Assert-ThrowsLike {
        Get-CrossCropCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $tamperedAssets
    } '*diagnostic*' 'cross/crop coverage evidence accepted a diagnostic visual'

    $metadataIdentityIndex = $source.IndexOf("`$metadata['cross_crop_gallery']", [StringComparison]::Ordinal)
    $visualPublicationIndex = $source.IndexOf('`$fixturePublication = Publish-VisualFixture'.TrimStart('`'), [StringComparison]::Ordinal)
    Assert-True ($metadataIdentityIndex -ge 0 -and $visualPublicationIndex -gt $metadataIdentityIndex) 'cross/crop arguments and artifact identity were not recorded before visual fixture publication/capture'

    Assert-Equal 'AquaticGallery' $aquaticPlan.Manifest.fixture_kind 'aquatic plan lost fixture kind'
    Assert-Equal 29 ([int]$aquaticPlan.Manifest.gallery_state_count) 'aquatic plan did not enumerate exactly seagrass+kelp'
    Assert-Equal 3 ([int]$aquaticPlan.Manifest.coverage_evidence.seagrass_state_count) 'aquatic plan lost seagrass coverage'
    Assert-Equal 26 ([int]$aquaticPlan.Manifest.coverage_evidence.kelp_state_count) 'aquatic plan lost kelp coverage'
    Assert-Equal 0 ([int]$aquaticPlan.Manifest.family_diagnostics.seagrass_kelp) 'aquatic target diagnostic contract changed'
    Assert-Equal 113 $aquaticPlan.GalleryCommands.Count 'aquatic gallery command coverage changed'
    Assert-Equal 26 @($aquaticPlan.Manifest.body_witnesses).Count 'aquatic gallery did not provide one above-neighbor body witness per kelp age'
    $growthCappedTip = @($aquaticPlan.CoverageEntries | Where-Object { $_.name -ceq 'minecraft:kelp' -and $_.canonical_state -match '"kelp_age".*"value":25' })
    Assert-Equal 1 $growthCappedTip.Count 'aquatic fixture did not resolve one canonical age-25 kelp tip'
    Assert-Equal 26 @($aquaticPlan.Manifest.body_witnesses | Where-Object { $_.upper.sequential_id -eq $growthCappedTip[0].sequential_id }).Count 'body witnesses use a growable upper kelp tip'
    Assert-Equal 26 @($aquaticPlan.Manifest.isolated_kelp_heads).Count 'aquatic gallery did not provide one isolated head per kelp age'
    Assert-Equal 26 @($aquaticPlan.Manifest.head_growth_caps).Count 'isolated kelp heads can grow nondeterministically during capture'
    Assert-Equal 26 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:bedrock$' }).Count 'isolated kelp heads were not capped with a rendered non-kelp block'
    Assert-Equal 29 @($aquaticPlan.CoverageEntries).Count 'aquatic plan coverage entries included witness duplicates'
    Assert-Equal 1 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:water$' }).Count 'aquatic gallery did not build one source-water volume'
    Assert-Equal 1 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:bedrock$' }).Count 'aquatic gallery did not build one supported-texture tank shell'
    Assert-Equal 1 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:dirt$' }).Count 'aquatic gallery did not provide submerged plant support'
    Assert-Equal 3 @($aquaticPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:air$' }).Count 'aquatic gallery did not clear setup volume and open both camera faces'
    $aquaticStateNames = @($aquaticPlan.CoverageEntries | ForEach-Object name | Sort-Object -Unique)
    Assert-Equal 'minecraft:kelp,minecraft:seagrass' ($aquaticStateNames -join ',') 'aquatic coverage admitted another Aquatic-family block'
    $aquaticFirst = $aquaticPlan.Manifest | ConvertTo-Json -Compress -Depth 12
    $aquaticSecond = (New-AquaticGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose AquaticGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets).Manifest | ConvertTo-Json -Compress -Depth 12
    Assert-Equal $aquaticFirst $aquaticSecond 'aquatic gallery arguments were not deterministic'
    $aquaticBack = New-AquaticGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose AquaticGalleryBack -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    $aquaticMoved = New-AquaticGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose AquaticGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    Assert-Equal $aquaticPlan.Manifest.fixture_layout_hash $aquaticBack.Manifest.fixture_layout_hash 'aquatic front/back pose changed fixture layout identity'
    Assert-Equal $aquaticPlan.Manifest.fixture_layout_hash $aquaticMoved.Manifest.fixture_layout_hash 'aquatic absolute coordinate changed fixture layout identity'
    Assert-Equal 20 ([Math]::Abs([int]$aquaticPlan.Manifest.camera.position.z - [int]$aquaticPlan.Manifest.gallery_center.z)) 'aquatic front camera is not outside the open tank face near the plants'
    Assert-Equal 20 ([Math]::Abs([int]$aquaticBack.Manifest.camera.position.z - [int]$aquaticBack.Manifest.gallery_center.z)) 'aquatic back camera is not outside the open tank face near the plants'

    Assert-Equal 'WaterGallery' $waterPlan.Manifest.fixture_kind 'water plan lost fixture kind'
    Assert-Equal 24 $waterPlan.GalleryCommands.Count 'water gallery command line changed'
    Assert-Equal 1 @($waterPlan.GalleryCommands | Where-Object { $_ -match '^fill .* minecraft:water$' }).Count 'water gallery did not contain exactly one still-pool fill'
    Assert-True (@($waterPlan.GalleryCommands[3..6] | Where-Object { $_ -match ' minecraft:glass$' }).Count -eq 4) 'water gallery did not build its still-pool enclosure before placing water'
    Assert-True ($waterPlan.GalleryCommands[7] -match ' minecraft:water$') 'water gallery placed still water before its enclosure was complete'
    Assert-Equal 6 @($waterPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:water \["liquid_depth"=[0-5]\]$' }).Count 'water gallery lost its six-state downhill flow edge'
    Assert-Equal 'glass' $waterPlan.Manifest.relative_layout.flow_enclosure.block 'water gallery flow states were not enclosed against fluid ticks'
    Assert-Equal 1 @($waterPlan.GalleryCommands | Where-Object { $_ -match '^setblock .* minecraft:seagrass' }).Count 'water gallery lost its waterlogged plant witness'
    Assert-Equal 2 @($waterPlan.Manifest.relative_layout.biome_tint_witnesses).Count 'water gallery lost a biome-tint witness'
    Assert-Equal 'runtime-biome-index-water-tint-lookup' $waterPlan.Manifest.relative_layout.biome_tint_evidence.kind 'water gallery did not bind tint witnesses to runtime biome lookup'
    Assert-True (-not [bool]$waterPlan.Manifest.relative_layout.biome_tint_evidence.distinct_biome_colours_claimed) 'water gallery claimed nearby witnesses prove distinct biome colours'
    Assert-Equal 1 ([uint64]$waterPlan.Manifest.relative_layout.biome_tint_evidence.minimum_rendered_distinct_tint_count) 'water gallery did not state its honest single-biome live tint requirement'
    Assert-Equal 'bedrock-client::tests::compiled_and_live_biome_tables_preserve_raw_id_water_colour_parity' ([string]$waterPlan.Manifest.relative_layout.biome_tint_evidence.multi_biome_lookup_parity_test) 'water gallery did not retain the separate multi-biome lookup parity proof'
    Assert-True ([Math]::Abs([double]$waterPlan.Manifest.performance.maximum_p99_frame_ms - (1000.0 / 60.0)) -lt 0.0000001) 'water gallery did not manifest the exact 60fps p99 threshold'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.still_pool) 'water gallery did not manifest its still pool'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.downhill_flow_edge) 'water gallery did not manifest its downhill flow edge'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.waterlogged_plant) 'water gallery did not manifest its waterlogged plant'
    Assert-True ($null -ne $waterPlan.Manifest.relative_layout.blend_edge) 'water gallery did not manifest its blend edge'
    Assert-True ($waterPlan.TeleportCommand -cne $waterPlan.CameraResortCommand) 'water gallery camera movement did not change the view'
    Assert-Equal $waterPlan.TeleportCommand $waterPlan.Manifest.camera_poses.initial.command 'water gallery manifest lost its initial fixed camera pose'
    Assert-Equal $waterPlan.CameraResortCommand $waterPlan.Manifest.camera_poses.resort.command 'water gallery manifest lost its moving-camera re-sort pose'
    $waterPlanAgain = New-WaterGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose WaterGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    $waterBack = New-WaterGalleryPlan -MutationCoordinate @(100, 64, 200) -Pose WaterGalleryBack -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    $waterMoved = New-WaterGalleryPlan -MutationCoordinate @(500, 70, -300) -Pose WaterGalleryFront -RegistryPath $BlockRegistry -AssetsPath $AquaticAssets
    Assert-Equal ($waterPlan.Manifest | ConvertTo-Json -Compress -Depth 12) ($waterPlanAgain.Manifest | ConvertTo-Json -Compress -Depth 12) 'water gallery was not deterministic'
    Assert-Equal $waterPlan.Manifest.fixture_layout_hash $waterBack.Manifest.fixture_layout_hash 'water front/back pose changed fixture layout identity'
    Assert-Equal $waterPlan.Manifest.fixture_layout_hash $waterMoved.Manifest.fixture_layout_hash 'water absolute coordinate changed fixture layout identity'
    Assert-Equal $waterPlan.TeleportCommand $waterBack.CameraResortCommand 'water front initial pose did not equal back resort pose'
    Assert-Equal $waterPlan.CameraResortCommand $waterBack.TeleportCommand 'water front resort pose did not equal back initial pose'
    $waterWitnessRequest = New-WaterGalleryTransparentWitnessRequest -Plan $waterPlan -Revision 1
    Assert-Equal 'rust-mcbe-transparent-witness-v1' $waterWitnessRequest.schema 'water witness request lost its strict schema'
    Assert-Equal 1 ([uint64]$waterWitnessRequest.revision) 'water witness request lost its revision'
    Assert-Equal 0 ([int]$waterWitnessRequest.dimension) 'water witness request lost its dimension'
    Assert-Equal '5,4,12;6,4,12;7,4,12' (@($waterWitnessRequest.sub_chunks | ForEach-Object { "$($_.x),$($_.y),$($_.z)" }) -join ';') 'water witness request did not derive the exact unique liquid-bearing subchunks'

    $modelWitnessRequest = New-SlabStairGalleryModelWitnessRequest -Plan $slabStairPlans[0] -Revision 1
    Assert-Equal 'rust-mcbe-model-witness-v1' $modelWitnessRequest.schema 'model witness request lost its strict schema'
    Assert-Equal 1 ([uint64]$modelWitnessRequest.revision) 'model witness request lost its revision'
    Assert-Equal 0 ([int]$modelWitnessRequest.dimension) 'model witness request lost its dimension'
    Assert-True (@($modelWitnessRequest.sub_chunks).Count -gt 0) 'model witness request did not derive unique central-witness keys'
    Assert-Equal @($modelWitnessRequest.sub_chunks).Count @($modelWitnessRequest.sub_chunks | Sort-Object x, y, z -Unique).Count 'model witness request retained duplicate keys'
    Assert-True ([string]$modelWitnessRequest.request_sha256 -cmatch '^[0-9a-f]{64}$') 'model witness request lost its deterministic hash'

    $galleryAnchor = ConvertFrom-GalleryAnchorReadyMarker -Line 'RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=false clean=true'
    Assert-Equal '14,71,-6' (@($galleryAnchor.coordinate) -join ',') 'gallery anchor parser lost its exact mutation coordinate'
    Assert-Equal $false ([bool]$galleryAnchor.visible) 'gallery anchor parser lost the observed cave-visibility state'
    Assert-ThrowsLike {
        ConvertFrom-GalleryAnchorReadyMarker -Line 'RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=true clean=false'
    } '*invalid gallery anchor ready marker*' 'gallery anchor parser accepted an unclean target'
    Assert-ThrowsLike {
        ConvertFrom-GalleryAnchorReadyMarker -Line 'RUST_MCBE_GALLERY_ANCHOR_READY coordinate=14,71,-6 rendered=true visible=true clean=true extra=true'
    } '*invalid gallery anchor ready marker*' 'gallery anchor parser accepted an unknown field'
    $appLaunchIndex = $source.IndexOf('$appHandle = Start-LoggedProcess -Executable $AppExecutable', [StringComparison]::Ordinal)
    $galleryAnchorBranchIndex = $source.IndexOf('if ($isSlabStairGallery) {', $appLaunchIndex, [StringComparison]::Ordinal)
    $galleryAnchorWaitIndex = $source.IndexOf("-Marker 'RUST_MCBE_GALLERY_ANCHOR_READY '", $galleryAnchorBranchIndex, [StringComparison]::Ordinal)
    $normalStartupBranchIndex = $source.IndexOf("`n    else {", $galleryAnchorWaitIndex, [StringComparison]::Ordinal)
    $worldReadyWaitIndex = $source.IndexOf("-Marker 'RUST_MCBE_WORLD_READY '", $normalStartupBranchIndex, [StringComparison]::Ordinal)
    Assert-True ($appLaunchIndex -ge 0 -and $galleryAnchorBranchIndex -gt $appLaunchIndex -and $galleryAnchorWaitIndex -gt $galleryAnchorBranchIndex) 'slab/stair startup does not wait for its gallery-only early anchor'
    Assert-True ($normalStartupBranchIndex -gt $galleryAnchorWaitIndex -and $worldReadyWaitIndex -gt $normalStartupBranchIndex) 'normal/perf startup no longer retains its strict WorldReady wait'

    $modelMarker = ConvertFrom-ModelWitnessCompleteMarker -Line "RUST_MCBE_MODEL_WITNESS_COMPLETE revision=1 request_sha256=$($modelWitnessRequest.request_sha256) sequence=40 view_generation=3 key_count=$(@($modelWitnessRequest.sub_chunks).Count) model_ref_count=43 manifest_count=$(@($modelWitnessRequest.sub_chunks).Count) manifest_sha256=$('a' * 64) missing=0 stale=0 wrong_stream=0 zero_ref=0 draw_mismatch=0 consecutive=1"
    Assert-Equal 1 ([int]$modelMarker.consecutive) 'model marker parser lost consecutive count'
    $modelMarker2 = $modelMarker.PSObject.Copy()
    $modelMarker2.sequence = [uint64]41
    $modelMarker2.consecutive = 2
    $null = Assert-StableModelWitnessEvidence -Request $modelWitnessRequest -First $modelMarker -Second $modelMarker2
    Assert-ThrowsLike {
        $bad = $modelMarker2.PSObject.Copy(); $bad.sequence = [uint64]42
        Assert-StableModelWitnessEvidence -Request $modelWitnessRequest -First $modelMarker -Second $bad
    } '*model witness*adjacent*' 'non-adjacent model witness evidence was accepted'

    $tamperedAquaticAssets = Join-Path $TempRoot 'tampered aquatic assets.mcbea'
    [IO.File]::WriteAllBytes($tamperedAquaticAssets, [IO.File]::ReadAllBytes($AquaticAssets))
    $firstAquaticId = [int]$aquaticPlan.CoverageEntries[0].sequential_id
    $tamperedAquaticBytes = [IO.File]::ReadAllBytes($tamperedAquaticAssets)
    $tamperedAquaticBytes[200 + 40 * $firstAquaticId + 25] = 0
    [IO.File]::WriteAllBytes($tamperedAquaticAssets, $tamperedAquaticBytes)
    Assert-ThrowsLike {
        Get-AquaticCoverageEvidence -RegistryPath $BlockRegistry -AssetsPath $tamperedAquaticAssets
    } '*diagnostic*' 'aquatic coverage evidence accepted a diagnostic target visual'
    $aquaticMetadataIdentityIndex = $source.IndexOf("`$metadata['aquatic_gallery']", [StringComparison]::Ordinal)
    Assert-True ($aquaticMetadataIdentityIndex -ge 0 -and $visualPublicationIndex -gt $aquaticMetadataIdentityIndex) 'aquatic arguments and artifact identity were not recorded before visual fixture publication/capture'

    $safeGeneratedRoot = Join-Path $TempRoot 'generated destinations'
    Assert-PrebuiltClientPathSafe `
        -ClientExecutable $PrebuiltClient `
        -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
        -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
        -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
        -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Split-Path -Parent $PrebuiltClient) `
            -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
            -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
            -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    } '*overlaps stable BDS runtime*' 'prebuilt client inside the generated BDS runtime was accepted'
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
            -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
            -CoreExecutable $PrebuiltClient `
            -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    } '*aliases generated core executable*' 'prebuilt client aliasing the core output was accepted'
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
            -RunDirectory (Split-Path -Parent $PrebuiltClient) `
            -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
            -MetricsOut (Join-Path $safeGeneratedRoot 'metrics.json')
    } '*overlaps acceptance run output*' 'prebuilt client inside the acceptance output directory was accepted'
    Assert-ThrowsLike {
        Assert-PrebuiltClientPathSafe `
            -ClientExecutable $PrebuiltClient `
            -RuntimeDirectory (Join-Path $safeGeneratedRoot 'runtime') `
            -RunDirectory (Join-Path $safeGeneratedRoot 'run') `
            -CoreExecutable (Join-Path $safeGeneratedRoot 'bedrock-core.exe') `
            -MetricsOut $PrebuiltClient
    } '*aliases requested metrics output*' 'prebuilt client aliasing MetricsOut was accepted'
    $prebuiltGuardHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $PrebuiltClient).Hash.ToLowerInvariant()
    Assert-FileHashUnchanged -Path $PrebuiltClient -ExpectedSha256 $prebuiltGuardHash -Label 'test prebuilt client'

    $teleportMarkerLine = 'RUST_MCBE_TELEPORT_SETTLED target=0:65:65:16 committed=0:65:65:16 ms=1500.0000 view_generation=7 transparent_sort_generation=11 render_ready_ms=1200.0000 publisher_ms=100.0000 first_level_ms=200.0000 last_level_ms=600.0000 level_events=1089 first_sub_ms=250.0000 last_sub_ms=900.0000 sub_events=1089 first_frame_sequence=41 stable_frame_sequence=42 first_present_ms=1300.0000 first_gpu_ms=1350.0000 stable_present_ms=1400.0000 stable_gpu_ms=1500.0000 expected_manifest_count=4 expected_manifest_hash=1111222233334444 first_presented_manifest_count=4 first_presented_manifest_hash=1111222233334444 stable_presented_manifest_count=4 stable_presented_manifest_hash=1111222233334444 expected=1089 loaded_target=1089 missing_target=0 foreign_loaded=0 foreign_requested=0 foreign_resident=0 source_leftover=0 resident_count=3 resident_hash=aaaabbbbccccdddd known_air_count=1 known_air_hash=eeeeffff00001111 missing_target_instances=0 unexpected_target_instances=0 source_instances=0 foreign_instances=0 stale_generation_instances=0 orphan_allocations=0 frame_count=90'
    $forcedMarkerLine = 'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED target=0:65:65:16 committed=0:65:65:16 ms=1500.0000 view_generation=8 transparent_sort_generation=12 render_ready_ms=0.0000 first_frame_sequence=43 stable_frame_sequence=44 first_present_ms=1200.0000 first_gpu_ms=1300.0000 stable_present_ms=1400.0000 stable_gpu_ms=1500.0000 expected_manifest_count=4 expected_manifest_hash=5555666677778888 first_presented_manifest_count=4 first_presented_manifest_hash=5555666677778888 stable_presented_manifest_count=4 stable_presented_manifest_hash=5555666677778888 expected=1089 loaded_target=1089 missing_target=0 foreign_loaded=0 foreign_requested=0 foreign_resident=0 source_leftover=0 resident_count=3 resident_hash=aaaabbbbccccdddd known_air_count=1 known_air_hash=eeeeffff00001111 missing_target_instances=0 unexpected_target_instances=0 source_instances=0 foreign_instances=0 stale_generation_instances=0 orphan_allocations=0 frame_count=90'
    $teleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine `
        -Kind Teleport
    $forcedMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine `
        -Kind ForcedRemesh
    Assert-Equal '0:65:65:16' $teleportMarker.target 'target-prefixed teleport marker lost its cohort'
    Assert-Equal 1500.0 $teleportMarker.ms 'target-prefixed teleport marker did not parse milliseconds'
    Assert-Equal '0:65:65:16' $forcedMarker.target 'target-prefixed forced-remesh marker lost its cohort'
    Assert-Equal 1500.0 $forcedMarker.ms 'target-prefixed forced-remesh marker did not parse milliseconds'
    Assert-Equal 11 $teleportMarker.transparent_sort_generation 'teleport marker lost presented transparent sort generation'
    Assert-Equal 12 $forcedMarker.transparent_sort_generation 'forced-remesh marker lost presented transparent sort generation'
    Assert-ThrowsLike {
        ConvertFrom-FullViewSettleMarker `
            -Line $teleportMarkerLine.Replace(' transparent_sort_generation=11', '') `
            -Kind Teleport
    } 'Teleport settle marker is missing transparent_sort_generation*' 'full-view marker accepted missing transparent presentation evidence'

    $worldReadyLine = 'RUST_MCBE_WORLD_READY source_tag=v1 blob_sha256=abc'
    $worldReadyTrigger = New-SteadyResourceTriggerEvidence `
        -Kind WorldReady `
        -WorldReadyMarker $worldReadyLine
    Assert-Equal 'WorldReady' $worldReadyTrigger.kind 'world-ready trigger changed kind'
    Assert-True ([string]$worldReadyTrigger.marker_sha256 -match '^[0-9a-f]{64}$') 'world-ready trigger omitted marker hash'
    $visualTrigger = New-SteadyResourceTriggerEvidence `
        -Kind VisualFixtureReady `
        -FixturePublication ([pscustomobject]@{
            ManifestSha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
            LayoutHash = 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb'
            Pose = 'LeafGalleryFront'
        })
    Assert-Equal 'VisualFixtureReady' $visualTrigger.kind 'visual trigger changed kind'
    Assert-Equal 'LeafGalleryFront' $visualTrigger.pose 'visual trigger lost pose'
    Assert-Equal ('a' * 64) $visualTrigger.manifest_sha256 'visual trigger lost manifest hash'
    Assert-Equal ('b' * 64) $visualTrigger.fixture_layout_hash 'visual trigger lost layout hash'
    $fullViewTrigger = New-SteadyResourceTriggerEvidence `
        -Kind FullViewPresented `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    Assert-Equal 'FullViewPresented' $fullViewTrigger.kind 'full-view trigger changed kind'
    Assert-ThrowsLike {
        New-SteadyResourceTriggerEvidence -Kind FullViewPresented -TeleportMarker $teleportMarker
    } '*ForcedRemeshMarker*' 'full-view trigger accepted incomplete binding evidence'

    $mutationCoordinate = @(101, 64, -37)
    $frontPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Front
    $frontPlanAgain = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Front
    $backPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose Back
    $teleportPlan = New-FullViewTeleportPlan -MutationCoordinate $mutationCoordinate
    $leafFrontPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose LeafGalleryFront
    $leafFrontPlanAgain = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose LeafGalleryFront
    $leafBackPlan = New-VisualFixturePlan -MutationCoordinate $mutationCoordinate -Pose LeafGalleryBack
    $baselineForestPlan = New-LeafForestPlan -MutationCoordinate $mutationCoordinate -Mode Baseline
    $fullViewForestPlan = New-FullViewTeleportPlan -MutationCoordinate $mutationCoordinate -LeafForest

    $stablePrefix = [Text.Encoding]::UTF8.GetBytes("first line`n")
    $sparseLogBytes = [byte[]]::new($stablePrefix.Length + 8)
    [Array]::Copy($stablePrefix, $sparseLogBytes, $stablePrefix.Length)
    $sparseLogBytes[$stablePrefix.Length + 4] = [byte][char]'x'
    Assert-Equal `
        $stablePrefix.Length `
        (Get-ContiguousProcessLogByteCount -Buffer $sparseLogBytes -Count $sparseLogBytes.Length) `
        'process-log reader advanced into an uncommitted NUL gap'
    Assert-Equal `
        $stablePrefix.Length `
        (Get-ContiguousProcessLogByteCount -Buffer $stablePrefix -Count $stablePrefix.Length) `
        'process-log reader truncated a fully committed byte range'

    $zeroErrorEvidence = Assert-BdsFixtureCommandResults `
        -Commands @('fill 0 0 0 0 0 0 minecraft:air') `
        -Lines @('[2026-07-11 12:00:00:000 ERROR] 0 blocks filled')
    Assert-Equal 0 $zeroErrorEvidence.results[0].changed_count 'BDS zero-change fill error did not retain an exact zero result'

    foreach ($leafPlan in @($leafFrontPlan, $leafBackPlan, $baselineForestPlan, $fullViewForestPlan)) {
        Assert-Equal 'rust-mcbe-visual-fixture-v2' $leafPlan.Manifest.schema 'leaf plan used the wrong manifest schema'
        Assert-True ([string]$leafPlan.Manifest.fixture_layout_hash -match '^[0-9a-f]{64}$') 'leaf plan omitted a canonical SHA-256 layout hash'
        Assert-True ($leafPlan.Commands.Count -le 64) "leaf plan exceeded the 64-command bound: $($leafPlan.Commands.Count)"
        Assert-True ($leafPlan.Commands[-2] -ceq 'list') 'leaf plan fence was not immediately before teleport'
        Assert-True ($leafPlan.Commands[-1] -ceq $leafPlan.TeleportCommand) 'leaf plan teleport was not the final planned command'
        foreach ($volume in @($leafPlan.Manifest.fill_volumes)) {
            Assert-True ([int]$volume -le 32768) "leaf plan fill exceeded the BDS bound: $volume"
        }
        $leafCommands = @($leafPlan.FixtureCommands | Where-Object { $_ -match 'minecraft:.*leaves' })
        Assert-True ($leafCommands.Count -gt 0) 'leaf plan emitted no leaf command'
        foreach ($leafCommand in $leafCommands) {
            Assert-True ($leafCommand.Contains('"persistent_bit"=true')) "leaf command omitted persistent_bit=true: $leafCommand"
            Assert-True ($leafCommand.Contains('"update_bit"=false')) "leaf command omitted update_bit=false: $leafCommand"
        }
    }

    Assert-Equal ($leafFrontPlan.Commands -join "`n") ($leafFrontPlanAgain.Commands -join "`n") 'leaf-front commands were not deterministic'
    Assert-Equal $leafFrontPlan.Manifest.fixture_layout_hash $leafFrontPlanAgain.Manifest.fixture_layout_hash 'leaf-front layout hash was not deterministic'
    Assert-Equal $leafFrontPlan.Manifest.fixture_layout_hash $leafBackPlan.Manifest.fixture_layout_hash 'leaf gallery poses did not share one canonical layout'
    Assert-True ($leafFrontPlan.TeleportCommand -cne $leafBackPlan.TeleportCommand) 'leaf gallery front/back cameras were identical'
    $selfColored = @('minecraft:cherry_leaves', 'minecraft:azalea_leaves', 'minecraft:azalea_leaves_flowered')
    $tintDeferred = @('minecraft:oak_leaves', 'minecraft:birch_leaves', 'minecraft:spruce_leaves')
    Assert-Equal ($selfColored -join ',') (@($leafFrontPlan.Manifest.self_colored) -join ',') 'self-colored leaf set changed'
    Assert-Equal ($tintDeferred -join ',') (@($leafFrontPlan.Manifest.tint_deferred) -join ',') 'tint-deferred leaf set changed'
    Assert-Equal 6 @($leafFrontPlan.Manifest.blocks).Count 'leaf gallery did not contain six labeled 2x2x2 cubes'
    foreach ($block in @($leafFrontPlan.Manifest.blocks)) {
        Assert-Equal '2,2,2' (@($block.size) -join ',') "leaf cube $($block.label) was not 2x2x2"
        Assert-True ([bool]$block.persistent_bit) "leaf cube $($block.label) was not persistent"
        Assert-True (-not [bool]$block.update_bit) "leaf cube $($block.label) enabled update_bit"
    }
    Assert-True (@($leafFrontPlan.Manifest.leaf_adjacency).Count -gt 0) 'leaf gallery omitted leaf-to-leaf adjacency evidence'
    Assert-True (@($leafFrontPlan.Manifest.opaque_backing).Count -gt 0) 'leaf gallery omitted opaque backing touching leaves'
    Assert-Equal 'near,far' (@($leafFrontPlan.Manifest.panels | ForEach-Object { $_.distance }) -join ',') 'leaf gallery omitted deterministic near/far panels'

    Assert-Equal $baselineForestPlan.Manifest.fixture_layout_hash $fullViewForestPlan.Manifest.fixture_layout_hash 'baseline and full-view forests changed canonical layout'
    Assert-True (@($baselineForestPlan.Manifest.canopies).Count -ge 4) 'forest did not contain multiple bounded canopies'
    foreach ($forestPlan in @($baselineForestPlan, $fullViewForestPlan)) {
        Assert-Equal ($selfColored -join ',') (@($forestPlan.Manifest.self_colored) -join ',') 'forest self-colored leaf set changed'
        Assert-Equal ($tintDeferred -join ',') (@($forestPlan.Manifest.tint_deferred) -join ',') 'forest tint-deferred leaf set changed'
        $forestSelfColored = @($forestPlan.Manifest.canopies | Where-Object { $_.category -ceq 'self_colored' } | ForEach-Object { $_.block } | Sort-Object -Unique)
        $forestTintDeferred = @($forestPlan.Manifest.canopies | Where-Object { $_.category -ceq 'tint_deferred' } | ForEach-Object { $_.block } | Sort-Object -Unique)
        Assert-Equal (($selfColored | Sort-Object) -join ',') ($forestSelfColored -join ',') 'forest canopy categories lost a self-colored identifier'
        Assert-Equal (($tintDeferred | Sort-Object) -join ',') ($forestTintDeferred -join ',') 'forest canopy categories lost a tint-deferred identifier'
        Assert-Equal $mutationCoordinate[1] $forestPlan.Manifest.clear.min.y 'forest clear did not own the target ground layer'
        Assert-Equal 31213 $forestPlan.Manifest.clear.volume 'forest clear volume changed from the bounded 49x13x49 scene'
        Assert-True (@($forestPlan.Manifest.fill_volumes) -contains 31213) 'forest fill-volume evidence omitted the exact clear volume'
        Assert-Equal 0 $forestPlan.Manifest.layout.clear_min_offset[1] 'forest canonical layout did not own the ground layer'
        Assert-Equal 'rust_mcbe_leaf_forest' $forestPlan.LoadAreaName 'forest ticking-area name changed'
        Assert-Equal `
            'tickingarea add 1117 64 991 1165 76 1039 rust_mcbe_leaf_forest true' `
            $forestPlan.LoadAreaCommand `
            'forest did not request an exact deterministic preload rectangle around its clear bounds'
        Assert-Equal 'marked for preload.' $forestPlan.LoadAreaMarker 'forest waited for the wrong preload acknowledgement'
        Assert-Equal 8000 $forestPlan.LoadAreaSettleMilliseconds 'forest preload settle bound changed'
        Assert-Equal 'tickingarea remove rust_mcbe_leaf_forest' $forestPlan.CleanupCommand 'forest cleanup command changed'
        Assert-Equal 'Removed ticking area(s)' $forestPlan.CleanupMarker 'forest cleanup waited for the wrong acknowledgement'
        Assert-Equal $forestPlan.LoadAreaCommand $forestPlan.Commands[0] 'forest preload was not the first planned command'
        Assert-Equal 22 $forestPlan.Manifest.command_count 'forest command count omitted preload/fence/teleport'
        Assert-Equal 'rust_mcbe_leaf_forest' $forestPlan.Manifest.load_area.name 'forest manifest omitted the deterministic ticking-area name'
        Assert-True ([bool]$forestPlan.Manifest.load_area.preload) 'forest manifest did not require ticking-area preload'
        Assert-Equal 8000 $forestPlan.Manifest.load_area.settle_milliseconds 'forest manifest omitted preload settle evidence'
    }
    Assert-True ($null -eq $leafFrontPlan.PSObject.Properties['LoadAreaCommand']) 'near leaf gallery unexpectedly acquired a ticking area'
    $expectedFarCamera = @(($mutationCoordinate[0] + 1040), ($mutationCoordinate[1] + 12), ($mutationCoordinate[2] + 1040))
    $expectedTargetMutation = @(($mutationCoordinate[0] + 1040), $mutationCoordinate[1], ($mutationCoordinate[2] + 1052))
    Assert-Equal ($expectedFarCamera -join ',') (@($baselineForestPlan.Target.x, $baselineForestPlan.Target.y, $baselineForestPlan.Target.z) -join ',') 'baseline forest did not use the identical far camera/cohort'
    Assert-Equal ($expectedFarCamera -join ',') (@($fullViewForestPlan.Target.x, $fullViewForestPlan.Target.y, $fullViewForestPlan.Target.z) -join ',') 'far camera changed from the fixed 65-chunk binding target'
    Assert-Equal ($expectedTargetMutation -join ',') (@($baselineForestPlan.TargetMutation.x, $baselineForestPlan.TargetMutation.y, $baselineForestPlan.TargetMutation.z) -join ',') 'baseline forest did not use the identical far mutation coordinate'
    Assert-Equal ($expectedTargetMutation -join ',') (@($fullViewForestPlan.TargetMutation.x, $fullViewForestPlan.TargetMutation.y, $fullViewForestPlan.TargetMutation.z) -join ',') 'far target mutation changed from the no-CLI contract'
    Assert-Equal ($baselineForestPlan.Commands -join "`n") ($fullViewForestPlan.Commands -join "`n") 'baseline and full-view forests did not publish identical scene commands'
    Assert-Equal 65 $baselineForestPlan.Manifest.offset_chunks 'baseline forest did not publish the same far offset'
    $redundantTargetSupportCommand = "setblock $($expectedTargetMutation[0]) $($expectedTargetMutation[1] - 1) $($expectedTargetMutation[2]) minecraft:stone"
    Assert-True (-not ($fullViewForestPlan.FixtureCommands -contains $redundantTargetSupportCommand)) 'forest redundantly replaced target support already covered by its stone platform'
    $initialTargetCommand = "setblock $($expectedTargetMutation[0]) $($expectedTargetMutation[1]) $($expectedTargetMutation[2]) minecraft:diamond_block"
    Assert-True ($fullViewForestPlan.FixtureCommands -contains $initialTargetCommand) 'forest did not initialize target mutation to the opposite block'
    Assert-True (-not ($fullViewForestPlan.FixtureCommands -contains $initialTargetCommand.Replace('diamond_block', 'gold_block'))) 'forest initialized target to the first post-ARM block, making it a no-op'
    Assert-Equal 'minecraft:gold_block,minecraft:diamond_block' (@($fullViewForestPlan.Manifest.mutation_blocks) -join ',') 'target mutation alternation changed'
    Assert-Equal ($mutationCoordinate -join ',') (@($fullViewForestPlan.Manifest.source_mutation.x, $fullViewForestPlan.Manifest.source_mutation.y, $fullViewForestPlan.Manifest.source_mutation.z) -join ',') 'forest manifest lost source mutation identity'
    Assert-Equal ($expectedTargetMutation -join ',') (@($fullViewForestPlan.Manifest.target_mutation.x, $fullViewForestPlan.Manifest.target_mutation.y, $fullViewForestPlan.Manifest.target_mutation.z) -join ',') 'forest manifest lost target mutation identity'

    $preloadResult = Assert-BdsTickingAreaPreloadResult `
        -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.' `
        -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
        -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    Assert-Equal '1104,976,1167,1039' (@($preloadResult.min_x, $preloadResult.min_z, $preloadResult.max_x, $preloadResult.max_z) -join ',') 'preload acknowledgement lost snapped X/Z bounds'
    Assert-ThrowsLike {
        Assert-BdsTickingAreaPreloadResult `
            -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1120, 0, 992 to 1151, 0, 1023 marked for preload.' `
            -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
            -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    } 'ticking-area acknowledgement did not match exact chunk-snapped fixture bounds:*' 'forest accepted a snapped ticking area that did not cover its clear bounds'
    Assert-ThrowsLike {
        Assert-BdsTickingAreaPreloadResult `
            -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1088, 0, 960 to 1183, 0, 1055 marked for preload.' `
            -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
            -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    } 'ticking-area acknowledgement did not match exact chunk-snapped fixture bounds:*' 'forest accepted an overbroad stale ticking area that merely covered its clear bounds'
    Assert-ThrowsLike {
        Assert-BdsTickingAreaPreloadResult `
            -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039.' `
            -ExpectedMinimum $fullViewForestPlan.Manifest.clear.min `
            -ExpectedMaximum $fullViewForestPlan.Manifest.clear.max
    } 'invalid ticking-area preload acknowledgement:*' 'forest accepted a non-preloaded ticking area acknowledgement'

    foreach ($resultPlan in @($leafFrontPlan, $leafBackPlan, $baselineForestPlan, $fullViewForestPlan)) {
        $resultLines = New-TestBdsFixtureResultLines -Commands $resultPlan.FixtureCommands
        $resultEvidence = Assert-BdsFixtureCommandResults -Commands $resultPlan.FixtureCommands -Lines $resultLines
        Assert-Equal $resultPlan.FixtureCommands.Count $resultEvidence.result_count 'schema-v2 fixture result evidence lost a command result'
    }
    $zeroFillLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands)
    $zeroFillIndex = [Array]::FindIndex([string[]]$leafFrontPlan.FixtureCommands, [Predicate[string]]{ param($command) $command.StartsWith('fill ', [StringComparison]::Ordinal) })
    $zeroFillLines[$zeroFillIndex] = '[2026-07-11 12:00:00:000 INFO] 0 blocks filled'
    $null = Assert-BdsFixtureCommandResults -Commands $leafFrontPlan.FixtureCommands -Lines $zeroFillLines
    $outsideWorldLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $outsideWorldLines[0] = '[2026-07-11 12:00:00:000 ERROR] Cannot place blocks outside of the world'
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $fullViewForestPlan.FixtureCommands -Lines $outsideWorldLines
    } 'BDS fixture command failed:*outside of the world*' 'schema-v2 fixture accepted the live-observed outside-world failure'
    $missingResultLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands | Select-Object -Skip 1)
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $leafFrontPlan.FixtureCommands -Lines $missingResultLines
    } 'BDS fixture result count mismatch:*' 'schema-v2 fixture accepted a missing command result'
    $extraResultLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands) + @('[2026-07-11 12:00:00:000 INFO] Block placed')
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $leafFrontPlan.FixtureCommands -Lines $extraResultLines
    } 'BDS fixture result count mismatch:*' 'schema-v2 fixture accepted an extra command result'
    $outOfOrderLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $outOfOrderLines[0] = '[2026-07-11 12:00:00:000 INFO] Block placed'
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $fullViewForestPlan.FixtureCommands -Lines $outOfOrderLines
    } 'BDS fixture result did not match command 1:*' 'schema-v2 fixture accepted an out-of-order result type'
    $tooManyFilledLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $tooManyFilledLines[0] = '[2026-07-11 12:00:00:000 INFO] 31214 blocks filled'
    Assert-ThrowsLike {
        Assert-BdsFixtureCommandResults -Commands $fullViewForestPlan.FixtureCommands -Lines $tooManyFilledLines
    } 'BDS fill result exceeded declared command volume:*' 'schema-v2 fixture accepted an impossible fill count'

    $armedMarker = ConvertFrom-TargetMutationArmedMarker -Line 'RUST_MCBE_TARGET_MUTATION_ARMED source=101,64,-37 target=1141,64,1015 view_generation=9'
    Assert-Equal '101,64,-37' (@($armedMarker.source) -join ',') 'target-mutation marker lost source coordinate'
    Assert-Equal '1141,64,1015' (@($armedMarker.target) -join ',') 'target-mutation marker lost target coordinate'
    Assert-Equal 9 $armedMarker.view_generation 'target-mutation marker lost view generation'
    Assert-ThrowsLike {
        ConvertFrom-TargetMutationArmedMarker -Line 'RUST_MCBE_TARGET_MUTATION_ARMED source=101,64,-37 target=1141,64,1015'
    } 'invalid target mutation armed marker:*' 'target-mutation marker accepted a missing generation'
    $movePlayerIngress = ConvertFrom-MovePlayerIngressMarker -Line 'RUST_MCBE_MOVE_PLAYER_INGRESS sequence=27 position=1141.5,76.25,1003.5'
    Assert-Equal 27 $movePlayerIngress.sequence 'MovePlayer ingress marker lost its sequence'
    Assert-Equal 1141.5 ([double]$movePlayerIngress.position[0]) 'MovePlayer ingress marker lost decimal X'
    Assert-Equal 76.25 ([double]$movePlayerIngress.position[1]) 'MovePlayer ingress marker lost decimal Y'
    Assert-Equal 1003.5 ([double]$movePlayerIngress.position[2]) 'MovePlayer ingress marker lost decimal Z'
    Assert-ThrowsLike {
        ConvertFrom-MovePlayerIngressMarker -Line 'RUST_MCBE_MOVE_PLAYER_INGRESS sequence=0 position=1141.5,76.25,1003.5'
    } 'invalid MovePlayer ingress marker:*' 'MovePlayer ingress marker accepted sequence zero'

    Assert-Equal ($frontPlan.Commands -join "`n") ($frontPlanAgain.Commands -join "`n") 'front fixture commands were not deterministic'
    Assert-True ($frontPlan.TeleportCommand -cne $backPlan.TeleportCommand) 'front and back fixture teleports were identical'
    $expectedGalleryCenter = @($mutationCoordinate[0], ($mutationCoordinate[1] + 3), ($mutationCoordinate[2] + 4))
    $expectedFrontCamera = @($mutationCoordinate[0], ($mutationCoordinate[1] + 12), ($mutationCoordinate[2] - 24))
    $expectedBackCamera = @($mutationCoordinate[0], ($mutationCoordinate[1] + 10), ($mutationCoordinate[2] + 32))
    $frontCamera = $frontPlan.Manifest.camera.position
    $backCamera = $backPlan.Manifest.camera.position
    Assert-Equal `
        ($expectedFrontCamera -join ',') `
        (@($frontCamera.x, $frontCamera.y, $frontCamera.z) -join ',') `
        'front camera did not use the live-proven framing offset'
    Assert-Equal `
        ($expectedBackCamera -join ',') `
        (@($backCamera.x, $backCamera.y, $backCamera.z) -join ',') `
        'back camera did not use the live-proven framing offset'
    Assert-Equal 28 ([Math]::Abs([int]$frontCamera.z - [int]$frontPlan.Manifest.gallery_center.z)) 'front camera was not 28 Z blocks from gallery center'
    Assert-Equal 28 ([Math]::Abs([int]$backCamera.z - [int]$backPlan.Manifest.gallery_center.z)) 'back camera was not 28 Z blocks from gallery center'
    Assert-Equal `
        "tp @a[name=RustMCBE] $($expectedFrontCamera -join ' ') facing $($expectedGalleryCenter -join ' ')" `
        $frontPlan.TeleportCommand `
        'front teleport did not preserve the exact widened pose and gallery target'
    Assert-Equal `
        "tp @a[name=RustMCBE] $($expectedBackCamera -join ' ') facing $($expectedGalleryCenter -join ' ')" `
        $backPlan.TeleportCommand `
        'back teleport did not preserve the exact widened pose and gallery target'
    Assert-Equal 'list' $frontPlan.FenceCommand 'front fixture did not use the observable BDS list fence'
    Assert-Equal 'players online:' $frontPlan.FenceMarker 'front fixture waited for the wrong BDS list output'
    Assert-True ($frontPlan.Commands[-2] -ceq 'list') 'front processing fence was not immediately before teleport'
    Assert-True ($frontPlan.Commands[-1] -ceq $frontPlan.TeleportCommand) 'front teleport was not the final fixture command'
    Assert-Equal 'list' $backPlan.FenceCommand 'back fixture did not use the observable BDS list fence'
    Assert-Equal 'players online:' $backPlan.FenceMarker 'back fixture waited for the wrong BDS list output'
    Assert-True ($backPlan.Commands[-2] -ceq 'list') 'back processing fence was not immediately before teleport'
    Assert-True ($backPlan.Commands[-1] -ceq $backPlan.TeleportCommand) 'back teleport was not the final fixture command'
    Assert-True ($frontPlan.TeleportCommand.Contains('@a[name=RustMCBE]')) 'fixture teleport did not target the stable offline player name'
    $teleportDeltaChunks = [Math]::Abs([int]$teleportPlan.Target.x - $mutationCoordinate[0]) / 16
    Assert-True ($teleportDeltaChunks -gt 64) 'full-view teleport did not exceed two radius-16 view diameters'
    Assert-Equal 'list' $teleportPlan.FenceCommand 'full-view teleport did not use the observable BDS fence'
    Assert-True ($teleportPlan.TeleportCommand.Contains('@a[name=RustMCBE]')) 'full-view teleport did not target the stable offline player name'

    $clear = $frontPlan.Manifest.clear
    $clearVolume = ([int]$clear.max.x - [int]$clear.min.x + 1) *
        ([int]$clear.max.y - [int]$clear.min.y + 1) *
        ([int]$clear.max.z - [int]$clear.min.z + 1)
    Assert-True ($clearVolume -le 32768) "fixture clear volume exceeded BDS fill limit: $clearVolume"
    Assert-True (([int]$clear.min.y) -gt $mutationCoordinate[1]) 'fixture clear volume did not preserve the mutation surface block'

    $sandMinX = $mutationCoordinate[0] + 14
    $sandMaxX = $mutationCoordinate[0] + 15
    $sandMinZ = $mutationCoordinate[2] + 5
    $sandMaxZ = $mutationCoordinate[2] + 6
    $expectedSandSupport = "fill $sandMinX $($mutationCoordinate[1]) $sandMinZ $sandMaxX $($mutationCoordinate[1]) $sandMaxZ minecraft:stone"
    $expectedSandCube = "fill $sandMinX $($mutationCoordinate[1] + 1) $sandMinZ $sandMaxX $($mutationCoordinate[1] + 2) $sandMaxZ minecraft:sand"
    $sandSupportIndexes = @(for ($index = 0; $index -lt $frontPlan.Commands.Count; $index++) {
        if ($frontPlan.Commands[$index] -ceq $expectedSandSupport) {
            $index
        }
    })
    $sandCubeIndexes = @(for ($index = 0; $index -lt $frontPlan.Commands.Count; $index++) {
        if ($frontPlan.Commands[$index] -ceq $expectedSandCube) {
            $index
        }
    })
    Assert-Equal 1 $sandSupportIndexes.Count 'fixture did not contain exactly one deterministic hidden sand support'
    Assert-Equal 1 $sandCubeIndexes.Count 'fixture did not contain exactly one sand cube fill'
    Assert-True ($sandSupportIndexes[0] -lt $sandCubeIndexes[0]) 'fixture built the sand cube before its hidden support'

    $requiredBlocks = @(
        'minecraft:stone',
        'minecraft:dirt',
        'minecraft:grass_block',
        'minecraft:oak_planks',
        'minecraft:coal_ore',
        'minecraft:iron_ore',
        'minecraft:diamond_ore',
        'minecraft:sand',
        'minecraft:glass'
    )
    $fixtureCommands = $frontPlan.Commands -join "`n"
    foreach ($requiredBlock in $requiredBlocks) {
        Assert-True ($fixtureCommands.Contains($requiredBlock)) "fixture commands are missing $requiredBlock"
    }
    foreach ($axis in @('x', 'y', 'z')) {
        Assert-True ($fixtureCommands.Contains("minecraft:oak_log [`"pillar_axis`"=`"$axis`"]")) "fixture commands are missing the Bedrock-state $axis oak-log beam"
    }
    Assert-True ($fixtureCommands.Contains('minecraft:oak_stairs')) 'fixture commands are missing oak stairs'
    Assert-True ($fixtureCommands.Contains('minecraft:glass_pane')) 'fixture commands are missing glass panes'

    $expectedLabels = @('stone', 'dirt', 'grass', 'oak_planks', 'coal_ore', 'iron_ore', 'diamond_ore', 'sand', 'glass')
    $manifestLabels = @($frontPlan.Manifest.blocks | ForEach-Object { $_.label })
    Assert-Equal ($expectedLabels -join ',') ($manifestLabels -join ',') 'fixture manifest labels changed'
    Assert-Equal 'Front' $frontPlan.Manifest.pose 'fixture manifest did not record its pose'
    Assert-Equal 'list' $frontPlan.Manifest.processing_fence.command 'fixture manifest recorded the wrong fence command'
    Assert-Equal 'players online:' $frontPlan.Manifest.processing_fence.stdout_marker 'fixture manifest recorded the wrong fence marker'
    Assert-Equal ($mutationCoordinate -join ',') (@($frontPlan.Manifest.mutation.x, $frontPlan.Manifest.mutation.y, $frontPlan.Manifest.mutation.z) -join ',') 'fixture manifest did not derive from the mutation coordinate'
    Assert-True ($null -ne $frontPlan.Manifest.camera) 'fixture manifest omitted expected camera coordinates'
    Assert-True ($null -ne $frontPlan.Manifest.gallery_center) 'fixture manifest omitted expected gallery coordinates'

    foreach ($fixtureCommand in $frontPlan.Commands) {
        Assert-True (($fixtureCommand -ceq 'list') -or ($fixtureCommand -match '^(fill|setblock|tp) ')) "fixture contains an unexpected server command: $fixtureCommand"
        Assert-True (-not $fixtureCommand.Contains($BdsDir)) 'fixture command targeted the source BDS directory'
        Assert-True (-not $fixtureCommand.Contains("`r") -and -not $fixtureCommand.Contains("`n")) 'fixture command contains an injected newline'
    }

    $fixtureInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $fixtureHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $fixtureInput }
    }
    $script:ObservedFixtureFence = $null
    $fixtureRunDirectory = Join-Path $TempRoot 'fixture run'
    New-Item -ItemType Directory -Path $fixtureRunDirectory | Out-Null
    $fixturePublication = Publish-VisualFixture `
        -Handle $fixtureHandle `
        -Plan $frontPlan `
        -RunDirectory $fixtureRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedFixtureFence = $Marker
            return $Marker
        }
    $fixtureLogPath = Join-Path $fixtureRunDirectory 'bds.console.log'
    $fixtureReadyPath = Join-Path $fixtureRunDirectory 'visual-fixture-ready.json'
    Assert-True (Test-Path -LiteralPath $fixtureReadyPath -PathType Leaf) 'fixture ready artifact was not published'
    Assert-Equal $fixtureReadyPath $fixturePublication.Path 'fixture publication returned the wrong path'
    Assert-True ([string]$fixturePublication.ManifestSha256 -match '^[0-9a-f]{64}$') 'fixture publication omitted its file hash'
    Assert-Equal ($frontPlan.Commands -join "`n") ((Get-Content -LiteralPath $fixtureLogPath) -join "`n") 'fixture console log did not record every command in order'
    Assert-Equal ($frontPlan.Commands -join [Environment]::NewLine) $fixtureInput.ToString().TrimEnd("`r", "`n") 'fixture commands were not sent through the owned standard input in order'
    Assert-Equal $frontPlan.FenceMarker $script:ObservedFixtureFence 'fixture publisher did not wait for the processing fence'
    $fixtureReady = Get-Content -Raw -LiteralPath $fixtureReadyPath | ConvertFrom-Json
    Assert-Equal 'Front' $fixtureReady.pose 'fixture ready artifact recorded the wrong pose'
    Assert-Equal 'list' $fixtureReady.processing_fence.command 'fixture ready artifact recorded the wrong fence command'
    Assert-Equal 'players online:' $fixtureReady.processing_fence.stdout_marker 'fixture ready artifact recorded the wrong fence marker'
    Assert-Equal 3000 $fixtureReady.settle_milliseconds 'fixture ready artifact did not record the production settle duration'
    Assert-Equal $frontPlan.TeleportCommand $fixtureReady.teleport_command 'fixture ready artifact recorded the wrong teleport'

    $waterInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $waterHandle = [pscustomobject]@{ Process = [pscustomobject]@{ StandardInput = $waterInput } }
    $script:WaterFenceCount = 0
    $script:WaterSortMarkerCount = 0
    $waterAppStdout = Join-Path $TempRoot 'water app stdout.log'
    [IO.File]::WriteAllText(
        $waterAppStdout,
        "RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=3 ref_count=12`n",
        [Text.UTF8Encoding]::new($false)
    )
    $waterAppHandle = [pscustomobject]@{
        StdoutPath = $waterAppStdout
        StdoutMarkerCursor = [pscustomobject]@{ Offset = [long]0; PartialLine = ''; LineNumber = [uint64]0 }
        Process = [pscustomobject]@{}
    }
    $waterRunDirectory = Join-Path $TempRoot 'water gallery run'
    New-Item -ItemType Directory -Path $waterRunDirectory | Out-Null
    $waterPublication = Publish-VisualFixture `
        -Handle $waterHandle `
        -Plan $waterPlan `
        -RunDirectory $waterRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:WaterFenceCount++
            if ($script:WaterFenceCount -eq 1) {
                $commandResults = @($waterPlan.FixtureCommands | ForEach-Object {
                    if ($_ -match '^fill ') { '[2026-07-11 00:00:00 INFO] 1 blocks filled' }
                    else { '[2026-07-11 00:00:00 INFO] Block placed' }
                })
                return New-TestBdsMarkerEvidence -Line '[INFO] There are 1/10 players online:' -SkippedLines $commandResults
            }
            if ($script:WaterFenceCount -eq 2) {
                return New-TestBdsMarkerEvidence `
                    -Line '[INFO] There are 1/10 players online:' `
                    -SkippedLines @('[INFO] Teleported RustMCBE to 100.000000, 74.000000, 176.000000')
            }
            return New-TestBdsMarkerEvidence `
                -Line '[INFO] There are 1/10 players online:' `
                -SkippedLines @('[INFO] Teleported RustMCBE to 100.000000, 73.000000, 224.000000')
        } `
        -AppHandle $waterAppHandle `
        -WaitForAppMarker {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:WaterSortMarkerCount++
            $lines = if ($Marker -ceq 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE ') {
                if ($script:WaterSortMarkerCount -eq 3) {
                    @('RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=1 sequence=21 generation=194 key_count=3 consecutive=1')
                }
                else {
                    @('RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=1 sequence=22 generation=194 key_count=3 consecutive=2')
                }
            }
            elseif ($script:WaterSortMarkerCount -eq 1) {
                @(
                    'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=7 ref_count=42',
                    'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=8 ref_count=42'
                )
            }
            else {
                @('RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=9 ref_count=42')
            }
            [IO.File]::AppendAllText($Handle.StdoutPath, (($lines -join "`n") + "`n"), [Text.UTF8Encoding]::new($false))
            return Wait-ProcessOutputMarker -Handle $Handle -Marker $Marker -TimeoutSeconds $TimeoutSeconds -PassThruEvidence
        }
    Assert-Equal $waterPlan.Manifest.fixture_layout_hash $waterPublication.LayoutHash 'water publication lost layout hash'
    Assert-Equal ($waterPlan.Commands -join [Environment]::NewLine) $waterInput.ToString().TrimEnd("`r", "`n") 'water gallery did not execute its fixed initial and moving-camera resort poses in order'
    Assert-Equal 3 $script:WaterFenceCount 'water gallery did not fence fixture construction and both camera poses'
    Assert-Equal 4 $script:WaterSortMarkerCount 'water gallery did not fence both committed transparent sorts and two GPU-presented exact-key witnesses'
    Assert-True (Test-Path -LiteralPath (Join-Path $waterRunDirectory 'transparent-witness-request.json') -PathType Leaf) 'water gallery did not atomically publish its exact witness request'
    $publishedWater = Get-Content -Raw -LiteralPath $waterPublication.Path | ConvertFrom-Json
    Assert-Equal $waterPlan.CameraResortCommand $publishedWater.camera_resort_command 'published water manifest lost moving-camera evidence'
    $waterEvents = @(Get-Content -LiteralPath (Join-Path $waterRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'fixture_commands_validated,initial_camera_fence_observed,initial_transparent_sort_committed,camera_resort_issued,camera_resort_fence_observed,resort_transparent_sort_committed,transparent_witness_complete,transparent_witness_complete' (@($waterEvents.event) -join ',') 'water gallery did not retain validated commands and causal exact-key GPU-presentation evidence'
    Assert-Equal '7,9' (@($waterEvents | Where-Object { $_.event -match 'transparent_sort_committed$' } | ForEach-Object { $_.generation }) -join ',') 'buffered pre-pose sort markers satisfied causal water-gallery evidence'
    Assert-ThrowsLike {
        Assert-BdsCameraResortResult -Evidence (New-TestBdsMarkerEvidence `
            -Line '[INFO] There are 1/10 players online:' `
            -SkippedLines @('[ERROR] No targets matched selector'))
    } 'BDS camera resort command failed:*' 'water gallery accepted a rejected camera resort before its fence'

    $sortMarker = ConvertFrom-TransparentSortCommittedMarker -Line 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=17 ref_count=99'
    Assert-Equal 17 $sortMarker.generation 'transparent sort marker parser lost generation'
    Assert-Equal 99 $sortMarker.ref_count 'transparent sort marker parser lost ref count'
    Assert-ThrowsLike {
        ConvertFrom-TransparentSortCommittedMarker -Line 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=0 ref_count=99'
    } 'invalid transparent sort committed marker:*' 'transparent sort marker accepted generation zero'
    $witnessMarker = ConvertFrom-TransparentWitnessCompleteMarker -Line 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=3 sequence=17 generation=194 key_count=3 consecutive=2'
    Assert-Equal 3 ([uint64]$witnessMarker.revision) 'transparent witness marker parser lost revision'
    Assert-Equal 2 ([int]$witnessMarker.consecutive) 'transparent witness marker parser lost consecutive proof count'
    Assert-ThrowsLike {
        ConvertFrom-TransparentWitnessCompleteMarker -Line 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=3 sequence=17 generation=194 key_count=0 consecutive=2'
    } 'invalid transparent witness complete marker:*' 'transparent witness marker accepted zero keys'
    $null = Assert-StableTransparentWitnessEvidence `
        -Request ([pscustomobject]@{ revision = [uint64]3; sub_chunks = @([pscustomobject]@{ x = 1; y = 2; z = 3 }) }) `
        -First ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]21; generation = [uint64]194; key_count = [uint64]1; consecutive = 1 }) `
        -Second ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]22; generation = [uint64]194; key_count = [uint64]1; consecutive = 2 })
    Assert-ThrowsLike {
        Assert-StableTransparentWitnessEvidence `
            -Request ([pscustomobject]@{ revision = [uint64]3; sub_chunks = @([pscustomobject]@{ x = 1; y = 2; z = 3 }) }) `
            -First ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]21; generation = [uint64]193; key_count = [uint64]1; consecutive = 1 }) `
            -Second ([pscustomobject]@{ revision = [uint64]3; sequence = [uint64]22; generation = [uint64]194; key_count = [uint64]1; consecutive = 1 })
    } 'transparent witness did not complete twice consecutively:*' 'a gen193 missing-key frame followed by only one gen194 complete frame satisfied readiness'
    $null = Assert-NewerTransparentSortCommit `
        -Initial ([pscustomobject]@{ generation = [uint64]7 }) `
        -InitialLineNumber 101 `
        -Resort ([pscustomobject]@{ generation = [uint64]8 }) `
        -ResortLineNumber 102
    Assert-ThrowsLike {
        Assert-NewerTransparentSortCommit `
            -Initial ([pscustomobject]@{ generation = [uint64]7 }) `
            -InitialLineNumber 101 `
            -Resort ([pscustomobject]@{ generation = [uint64]7 }) `
            -ResortLineNumber 102
    } 'camera resort did not commit a newer transparent sort:*' 'camera resort accepted the initial sort generation again'

    $forestInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $forestHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $forestInput }
    }
    $forestResultLines = New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands
    $script:ObservedForestFence = $null
    $script:ObservedForestLoadAreaMarker = $null
    $forestRunDirectory = Join-Path $TempRoot 'forest full view run'
    New-Item -ItemType Directory -Path $forestRunDirectory | Out-Null
    $forestPublication = Publish-FullViewTeleport `
        -Handle $forestHandle `
        -Plan $fullViewForestPlan `
        -RunDirectory $forestRunDirectory `
        -PreloadSettleMilliseconds 0 `
        -WaitForLoadArea {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedForestLoadAreaMarker = $Marker
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
        } `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:ObservedForestFence = $Marker
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                -SkippedLines $forestResultLines
        }
    Assert-Equal $fullViewForestPlan.LoadAreaMarker $script:ObservedForestLoadAreaMarker 'forest publisher did not observe the preload acknowledgement'
    Assert-Equal $fullViewForestPlan.FenceMarker $script:ObservedForestFence 'forest publisher did not observe the list fence'
    Assert-Equal $fullViewForestPlan.LoadAreaName $forestHandle.ActiveTickingArea.Name 'forest publisher did not retain active ticking-area cleanup state'
    Assert-True (Test-Path -LiteralPath $forestPublication.Path -PathType Leaf) 'forest publisher did not atomically publish its manifest'
    Assert-Equal $fullViewForestPlan.Manifest.fixture_layout_hash $forestPublication.LayoutHash 'forest publication lost layout hash'
    $forestManifestHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $forestPublication.Path).Hash.ToLowerInvariant()
    Assert-Equal $forestManifestHash $forestPublication.ManifestSha256 'forest publication hash did not match bytes'
    Assert-PublishedTargetMutation -Path $forestPublication.Path -Expected $fullViewForestPlan.TargetMutation
    $tamperedForestManifestPath = Join-Path $forestRunDirectory 'tampered-visual-fixture-ready.json'
    $tamperedForestManifest = Get-Content -Raw -LiteralPath $forestPublication.Path | ConvertFrom-Json
    $tamperedForestManifest.target_mutation.x = [int]$tamperedForestManifest.target_mutation.x + 1
    [IO.File]::WriteAllText(
        $tamperedForestManifestPath,
        ($tamperedForestManifest | ConvertTo-Json -Depth 16),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike {
        Assert-PublishedTargetMutation -Path $tamperedForestManifestPath -Expected $fullViewForestPlan.TargetMutation
    } 'published target mutation did not match plan*' 'far publisher accepted a serialized target mutation mismatch'
    Assert-Equal ($fullViewForestPlan.Commands -join "`n") ((Get-Content -LiteralPath (Join-Path $forestRunDirectory 'bds.console.log')) -join "`n") 'forest commands/fence/teleport were not sent in exact order'
    Assert-Equal ($fullViewForestPlan.Commands -join [Environment]::NewLine) $forestInput.ToString().TrimEnd("`r", "`n") 'forest owned-stdin order changed'
    $forestEvents = @(Get-Content -LiteralPath (Join-Path $forestRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal `
        'load_area_ready,fixture_commands_completed,processing_fence_observed,visual_fixture_ready,teleport_issued' `
        (@($forestEvents | ForEach-Object { $_.event }) -join ',') `
        'forest evidence event order changed'
    $forestReadyEvent = @($forestEvents | Where-Object { $_.event -ceq 'visual_fixture_ready' })[0]
    $forestTeleportEvent = @($forestEvents | Where-Object { $_.event -ceq 'teleport_issued' })[0]
    Assert-True ([int]$forestReadyEvent.sequence -lt [int]$forestTeleportEvent.sequence) 'forest teleport preceded atomic manifest readiness'
    Assert-Equal `
        (@($fullViewForestPlan.TargetMutation.x, $fullViewForestPlan.TargetMutation.y, $fullViewForestPlan.TargetMutation.z) -join ',') `
        ([string]$forestReadyEvent.target_mutation) `
        'forest ready event lost target mutation coordinate'
    Assert-Equal 0 @(Get-ChildItem -LiteralPath $forestRunDirectory -Filter '*.partial-*' -File).Count 'forest publication leaked a partial manifest'

    $baselineInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $baselineHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $baselineInput }
    }
    $baselineRunDirectory = Join-Path $TempRoot 'forest baseline run'
    New-Item -ItemType Directory -Path $baselineRunDirectory | Out-Null
    $baselineSourceCommand = Publish-BaselineSourceMutation `
        -Handle $baselineHandle `
        -Coordinate $mutationCoordinate `
        -RunDirectory $baselineRunDirectory
    $null = Start-BdsFixtureLoadArea `
        -Handle $baselineHandle `
        -Plan $baselineForestPlan `
        -RunDirectory $baselineRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForLoadArea {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
        }
    $script:BaselineReuseWaitInvoked = $false
    $null = Publish-VisualFixture `
        -Handle $baselineHandle `
        -Plan $baselineForestPlan `
        -RunDirectory $baselineRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForLoadArea {
            param($Handle, $Marker, $TimeoutSeconds)
            $script:BaselineReuseWaitInvoked = $true
            throw 'publisher attempted to wait for an already-ready exact ticking area'
        } `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                -SkippedLines (New-TestBdsFixtureResultLines -Commands $baselineForestPlan.FixtureCommands)
        }
    Assert-Equal 'setblock 101 64 -37 minecraft:gold_block' $baselineSourceCommand 'baseline source mutation prelude changed'
    Assert-True (-not $script:BaselineReuseWaitInvoked) 'baseline publisher did not reuse its ready ticking area'
    $expectedBaselineConsole = @($baselineSourceCommand) + @($baselineForestPlan.Commands)
    Assert-Equal ($expectedBaselineConsole -join "`n") ((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) -join "`n") 'baseline source mutation did not precede the far forest fence/teleport'
    Assert-Equal 1 @((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) | Where-Object { $_ -ceq $baselineForestPlan.LoadAreaCommand }).Count 'baseline exact-plan reuse issued the ticking-area add more than once'
    $baselineEvents = @(Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal `
        'source_mutation_command,load_area_ready,load_area_reused,fixture_commands_completed,processing_fence_observed,visual_fixture_ready,teleport_issued' `
        (@($baselineEvents | ForEach-Object { $_.event }) -join ',') `
        'baseline event evidence did not order source mutation before the far forest'
    $mismatchedLoadAreaPlan = New-LeafForestPlan -MutationCoordinate $mutationCoordinate -Mode Baseline
    $mismatchedLoadAreaPlan.LoadAreaCommand = $mismatchedLoadAreaPlan.LoadAreaCommand.Replace(' true', ' false')
    Assert-ThrowsLike {
        Start-BdsFixtureLoadArea `
            -Handle $baselineHandle `
            -Plan $mismatchedLoadAreaPlan `
            -RunDirectory $baselineRunDirectory `
            -SettleMilliseconds 0 `
            -WaitForLoadArea { throw 'mismatched plan attempted a second BDS command' }
    } 'BDS handle already owns a different exact ticking-area plan:*' 'baseline reused an active ticking area for a non-identical plan'
    Assert-Equal 1 @((Get-Content -LiteralPath (Join-Path $baselineRunDirectory 'bds.console.log')) | Where-Object { $_ -ceq $baselineForestPlan.LoadAreaCommand }).Count 'mismatched-plan rejection issued a second ticking-area add'

    $failedForestInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $failedForestHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $failedForestInput }
    }
    $failedForestRunDirectory = Join-Path $TempRoot 'forest rejected results run'
    New-Item -ItemType Directory -Path $failedForestRunDirectory | Out-Null
    $failedForestLines = @(New-TestBdsFixtureResultLines -Commands $fullViewForestPlan.FixtureCommands)
    $failedForestLines[0] = '[2026-07-11 12:00:00:000 ERROR] Cannot place blocks outside of the world'
    Assert-ThrowsLike {
        Publish-FullViewTeleport `
            -Handle $failedForestHandle `
            -Plan $fullViewForestPlan `
            -RunDirectory $failedForestRunDirectory `
            -PreloadSettleMilliseconds 0 `
            -WaitForLoadArea {
                param($Handle, $Marker, $TimeoutSeconds)
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
            } `
            -WaitForFence {
                param($Handle, $Marker, $TimeoutSeconds)
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                    -SkippedLines $failedForestLines
            }
    } 'BDS fixture command failed:*outside of the world*' 'forest publisher did not fail closed on the live-observed outside-world result'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $failedForestRunDirectory 'visual-fixture-ready.json'))) 'failed forest published a fixture manifest'
    Assert-True (Test-Path -LiteralPath (Join-Path $failedForestRunDirectory 'fixture-command-stdout.json') -PathType Leaf) 'failed forest did not preserve its exact live stdout interval'
    Assert-True (-not $failedForestInput.ToString().Contains($fullViewForestPlan.TeleportCommand)) 'failed forest teleported after a rejected fixture command'
    Assert-Equal $fullViewForestPlan.LoadAreaName $failedForestHandle.ActiveTickingArea.Name 'failed forest lost cleanup ownership for its active ticking area'
    $failedForestEvents = @(Get-Content -LiteralPath (Join-Path $failedForestRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'load_area_ready' (@($failedForestEvents | ForEach-Object { $_.event }) -join ',') 'failed forest claimed fixture command completion or publication'

    $leafGalleryInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $leafGalleryHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $leafGalleryInput }
    }
    $leafGalleryRunDirectory = Join-Path $TempRoot 'leaf gallery validated run'
    New-Item -ItemType Directory -Path $leafGalleryRunDirectory | Out-Null
    $leafGalleryPublication = Publish-VisualFixture `
        -Handle $leafGalleryHandle `
        -Plan $leafFrontPlan `
        -RunDirectory $leafGalleryRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForFence {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                -SkippedLines (New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands)
        }
    Assert-True (Test-Path -LiteralPath $leafGalleryPublication.Path -PathType Leaf) 'leaf gallery did not publish after every command result succeeded'
    $leafGalleryStdoutEvidence = Get-Content -Raw -LiteralPath (Join-Path $leafGalleryRunDirectory 'fixture-command-stdout.json') | ConvertFrom-Json
    Assert-Equal $leafFrontPlan.FixtureCommands.Count @($leafGalleryStdoutEvidence.skipped_lines).Count 'leaf gallery stdout artifact lost an exact result line'
    Assert-Equal 'players online:' $leafGalleryStdoutEvidence.marker 'leaf gallery stdout artifact lost its fence marker'
    Assert-Equal ($leafFrontPlan.Commands -join [Environment]::NewLine) $leafGalleryInput.ToString().TrimEnd("`r", "`n") 'leaf gallery result validation changed command/fence/teleport order'

    $failedGalleryInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $failedGalleryHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $failedGalleryInput }
    }
    $failedGalleryRunDirectory = Join-Path $TempRoot 'leaf gallery rejected results run'
    New-Item -ItemType Directory -Path $failedGalleryRunDirectory | Out-Null
    $failedGalleryLines = @(New-TestBdsFixtureResultLines -Commands $leafFrontPlan.FixtureCommands)
    $failedGalleryLines[0] = '[2026-07-11 12:00:00:000 ERROR] No blocks filled'
    Assert-ThrowsLike {
        Publish-VisualFixture `
            -Handle $failedGalleryHandle `
            -Plan $leafFrontPlan `
            -RunDirectory $failedGalleryRunDirectory `
            -SettleMilliseconds 0 `
            -WaitForFence {
                param($Handle, $Marker, $TimeoutSeconds)
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] There are 1/10 players online:' `
                    -SkippedLines $failedGalleryLines
            }
    } 'BDS fixture command failed:*No blocks filled*' 'leaf gallery publisher did not fail closed on an ERROR result'
    Assert-True (-not (Test-Path -LiteralPath (Join-Path $failedGalleryRunDirectory 'visual-fixture-ready.json'))) 'failed leaf gallery published a fixture manifest'
    Assert-True (-not $failedGalleryInput.ToString().Contains($leafFrontPlan.TeleportCommand)) 'failed leaf gallery teleported after a rejected fixture command'

    $cleanupResult = Remove-BdsTickingArea `
        -Handle $forestHandle `
        -RunDirectory $forestRunDirectory `
        -WaitForAck {
            param($Handle, $Marker, $TimeoutSeconds)
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Removed ticking area(s)'
        }
    Assert-Equal $fullViewForestPlan.CleanupCommand $cleanupResult.command 'ticking-area cleanup issued the wrong command'
    $activeTickingAreaProperty = $forestHandle.PSObject.Properties['ActiveTickingArea']
    Assert-True ($null -eq $activeTickingAreaProperty -or $null -eq $activeTickingAreaProperty.Value) 'acknowledged ticking-area cleanup left active ownership state'
    Assert-Equal $fullViewForestPlan.CleanupCommand ((Get-Content -LiteralPath (Join-Path $forestRunDirectory 'bds.console.log'))[-1]) 'cleanup command was not persisted in the BDS console log'
    $cleanupEvents = @(Get-Content -LiteralPath (Join-Path $forestRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'load_area_removed' $cleanupEvents[-1].event 'cleanup acknowledgement was not recorded as the final forest event'

    $failedCleanupInput = [IO.StringWriter]::new([Globalization.CultureInfo]::InvariantCulture)
    $failedCleanupHandle = [pscustomobject]@{
        Process = [pscustomobject]@{ StandardInput = $failedCleanupInput }
    }
    $failedCleanupRunDirectory = Join-Path $TempRoot 'ticking area cleanup failure run'
    New-Item -ItemType Directory -Path $failedCleanupRunDirectory | Out-Null
    $null = Start-BdsFixtureLoadArea `
        -Handle $failedCleanupHandle `
        -Plan $fullViewForestPlan `
        -RunDirectory $failedCleanupRunDirectory `
        -SettleMilliseconds 0 `
        -WaitForLoadArea {
            return New-TestBdsMarkerEvidence `
                -Line '[2026-07-11 12:00:00:000 INFO] Added ticking area from 1104, 0, 976 to 1167, 0, 1039 marked for preload.'
        }
    Assert-ThrowsLike {
        Remove-BdsTickingArea `
            -Handle $failedCleanupHandle `
            -RunDirectory $failedCleanupRunDirectory `
            -WaitForAck {
                return New-TestBdsMarkerEvidence `
                    -Line '[2026-07-11 12:00:00:000 INFO] Removed ticking areas'
            }
    } 'invalid ticking-area cleanup acknowledgement:*' 'cleanup accepted a non-exact acknowledgement'
    Assert-Equal $fullViewForestPlan.LoadAreaName $failedCleanupHandle.ActiveTickingArea.Name 'failed cleanup discarded active ticking-area ownership state'
    $failedCleanupEvents = @(Get-Content -LiteralPath (Join-Path $failedCleanupRunDirectory 'acceptance-events.jsonl') | ForEach-Object { ConvertFrom-Json $_ })
    Assert-Equal 'load_area_ready' (@($failedCleanupEvents | ForEach-Object { $_.event }) -join ',') 'failed cleanup falsely recorded load_area_removed'
    Assert-Equal $fullViewForestPlan.CleanupCommand ($failedCleanupInput.ToString().TrimEnd("`r", "`n").Split([Environment]::NewLine)[-1]) 'failed cleanup did not issue the owned exact removal command'

    $serverPropertiesPath = Join-Path $TempRoot 'server.properties'
    [IO.File]::WriteAllLines(
        $serverPropertiesPath,
        @(
            'server-port=19132'
            'server-portv6=19133'
            'online-mode=true'
            'allow-list=true'
            'enable-lan-visibility=true'
            'gamemode=survival'
            'force-gamemode=false'
            'allow-cheats=false'
            'view-distance=32'
            'player-idle-timeout=30'
            'default-player-permission-level=member'
            'client-side-chunk-generation-enabled=true'
            'server-name=fixture'
            'level-name=Bedrock level'
            'level-seed=unchanged-seed'
        ),
        [Text.UTF8Encoding]::new($false)
    )
    Set-ServerProperties -Path $serverPropertiesPath -Port 20000 -PortV6 20001
    $serverProperties = @([IO.File]::ReadAllLines($serverPropertiesPath))
    foreach ($expectedProperty in @(
        'server-port=20000',
        'server-portv6=20001',
        'online-mode=false',
        'allow-list=false',
        'enable-lan-visibility=false',
        'gamemode=creative',
        'force-gamemode=true',
        'allow-cheats=true',
        'view-distance=16',
        'player-idle-timeout=0',
        'default-player-permission-level=operator',
        'client-side-chunk-generation-enabled=false',
        'server-name=fixture',
        'level-name=Bedrock level',
        'level-seed=unchanged-seed'
    )) {
        Assert-True ($serverProperties -contains $expectedProperty) "missing rewritten property: $expectedProperty"
    }
    $duplicateAcceptancePropertyPath = Join-Path $TempRoot 'duplicate-acceptance-property.properties'
    [IO.File]::WriteAllLines(
        $duplicateAcceptancePropertyPath,
        @($serverProperties) + 'client-side-chunk-generation-enabled=true',
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike {
        Set-ServerProperties -Path $duplicateAcceptancePropertyPath -Port 20002 -PortV6 20003
    } 'server.properties must contain exactly one client-side-chunk-generation-enabled entry' 'duplicate client-side terrain generation setting was silently accepted'

    $worldIdentitySource = Join-Path $TempRoot 'world identity source'
    $worldIdentitySourceReverse = Join-Path $TempRoot 'world identity source reverse'
    foreach ($identityRoot in @($worldIdentitySource, $worldIdentitySourceReverse)) {
        $identityWorld = Join-Path $identityRoot 'worlds\Bedrock level'
        New-Item -ItemType Directory -Path (Join-Path $identityWorld 'db') -Force | Out-Null
        [IO.File]::WriteAllLines(
            (Join-Path $identityRoot 'server.properties'),
            @('server-name=identity fixture', 'level-name=Bedrock level'),
            [Text.UTF8Encoding]::new($false)
        )
    }
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySource 'worlds\Bedrock level\level.dat'), [byte[]]@(1, 2, 3, 4))
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySource 'worlds\Bedrock level\db\CURRENT'), [byte[]]@(5, 6))
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySource 'worlds\Bedrock level\db\MANIFEST-000001'), [byte[]]@(7, 8, 9))
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySourceReverse 'worlds\Bedrock level\db\MANIFEST-000001'), [byte[]]@(7, 8, 9))
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySourceReverse 'worlds\Bedrock level\db\CURRENT'), [byte[]]@(5, 6))
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySourceReverse 'worlds\Bedrock level\level.dat'), [byte[]]@(1, 2, 3, 4))
    $worldIdentity = Get-BdsSourceWorldIdentity -SourceDirectory $worldIdentitySource
    $worldIdentityAgain = Get-BdsSourceWorldIdentity -SourceDirectory $worldIdentitySource
    $worldIdentityReverse = Get-BdsSourceWorldIdentity -SourceDirectory $worldIdentitySourceReverse
    Assert-Equal 'Bedrock level' $worldIdentity.level_name 'source-world identity lost level-name'
    Assert-Equal 3 $worldIdentity.file_count 'source-world identity did not cover level.dat plus the full DB tree'
    Assert-Equal 9 $worldIdentity.total_bytes 'source-world identity byte count changed'
    Assert-True ([string]$worldIdentity.level_dat_sha256 -match '^[0-9a-f]{64}$') 'source-world identity omitted level.dat SHA-256'
    Assert-Equal $worldIdentity.sha256 $worldIdentityAgain.sha256 'source-world identity was not deterministic'
    Assert-Equal $worldIdentity.sha256 $worldIdentityReverse.sha256 'source-world identity depended on filesystem enumeration or root path'
    Assert-BdsSourceWorldIdentityUnchanged -Expected $worldIdentity -SourceDirectory $worldIdentitySource
    [IO.File]::WriteAllBytes((Join-Path $worldIdentitySource 'worlds\Bedrock level\db\CURRENT'), [byte[]]@(5, 7))
    Assert-ThrowsLike {
        Assert-BdsSourceWorldIdentityUnchanged -Expected $worldIdentity -SourceDirectory $worldIdentitySource
    } 'BDS source world identity changed:*' 'source-world mutation was not detected after the acceptance copy/run boundary'
    Assert-True `
        ([regex]::IsMatch($source, "source_world_identity[\s\S]*Assert-BdsSourceWorldIdentityUnchanged", [Text.RegularExpressions.RegexOptions]::CultureInvariant)) `
        'live flow did not record source-world identity before verifying it after the run'

    $runtimeSource = (Resolve-Path -LiteralPath $BdsDir).Path.TrimEnd('\', '/')
    if ($runtimeSource.StartsWith('\\')) {
        $goRuntimeSource = '\\?\UNC\' + $runtimeSource.TrimStart('\')
    }
    else {
        $goRuntimeSource = '\\?\' + $runtimeSource
    }
    $goRuntimeOwner = "rust-mcbe-bds-runtime-v1`nsource=$($goRuntimeSource.ToLowerInvariant())`n"
    $goOwnedRuntime = Join-Path $TempRoot 'go-owned stable runtime'
    New-Item -ItemType Directory -Path $goOwnedRuntime -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $goOwnedRuntime '.rust-mcbe-runtime-owner'),
        $goRuntimeOwner,
        [Text.UTF8Encoding]::new($false)
    )
    $goOwnedExecutable = Set-StableRuntime `
        -SourceDirectory $BdsDir `
        -RuntimeDirectory $goOwnedRuntime `
        -ExecutableName 'bedrock_server.exe'
    Assert-True (Test-Path -LiteralPath $goOwnedExecutable) 'Go-style extended owner path was rejected for the same BDS source'

    $differentRuntime = Join-Path $TempRoot 'different-owned stable runtime'
    New-Item -ItemType Directory -Path $differentRuntime -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $differentRuntime '.rust-mcbe-runtime-owner'),
        "rust-mcbe-bds-runtime-v1`nsource=\\?\c:\definitely-different-bds-source`n",
        [Text.UTF8Encoding]::new($false)
    )
    Assert-Throws {
        Set-StableRuntime `
            -SourceDirectory $BdsDir `
            -RuntimeDirectory $differentRuntime `
            -ExecutableName 'bedrock_server.exe'
    } 'different Go-style BDS owner marker was accepted'

    $runtimeSafetyScript = Join-Path $PSScriptRoot 'acceptance.RuntimeSafety.Tests.ps1'
    $previousErrorAction = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $runtimeSafetyOutput = & powershell `
            -NoProfile `
            -ExecutionPolicy Bypass `
            -File $runtimeSafetyScript `
            -Case All 2>&1
        $runtimeSafetyExitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
    }
    Assert-True ($runtimeSafetyExitCode -eq 0) "runtime safety tests failed: $($runtimeSafetyOutput -join [Environment]::NewLine)"
    Assert-True (@($runtimeSafetyOutput | Where-Object { $_.ToString().Contains('acceptance runtime safety tests (All): PASS') }).Count -eq 1) 'runtime safety test success marker was missing'

    Invoke-CheckedBuild `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "if ((Get-Location).Path -ne '$TempRoot') { exit 9 }") `
        -LogPath (Join-Path $TempRoot 'working-directory.log') `
        -WorkingDirectory $TempRoot

    $stderrBuildLog = Join-Path $TempRoot 'successful-stderr-build.log'
    Invoke-CheckedBuild `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "[Console]::Error.WriteLine('compiler-progress'); exit 0") `
        -LogPath $stderrBuildLog `
        -WorkingDirectory $TempRoot
    Assert-True ((Get-Content -Raw -LiteralPath $stderrBuildLog).Contains('compiler-progress')) 'successful native stderr was not retained in the build log'

    $helper = $null
    $helperCleanupFailure = $null
    try {
        $helper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-Command', "Write-Output 'TEST_READY'; [Console]::Error.WriteLine('error-line')") `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'helper.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'helper.stderr.log')
        Assert-True ((Wait-ProcessOutputMarker -Handle $helper -Marker 'TEST_READY' -TimeoutSeconds 10) -eq 'TEST_READY') 'direct log stream did not expose readiness marker'
        Assert-True ($helper.Process.WaitForExit(10000)) 'logging helper did not exit'
    }
    finally {
        try {
            Complete-TestLoggedProcess -Handle $helper
        }
        catch {
            $helperCleanupFailure = $_
        }
    }
    if ($null -ne $helperCleanupFailure) {
        throw $helperCleanupFailure
    }

    $evidenceHelper = $null
    $evidenceHelperCleanupFailure = $null
    try {
        $evidenceHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-Command', "Write-Output '[2026-07-11 12:00:00:000 INFO] 4 blocks filled'; Write-Output '[2026-07-11 12:00:00:001 INFO] Block placed'; Write-Output '[2026-07-11 12:00:00:002 INFO] There are 1/10 players online:'") `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'evidence-helper.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'evidence-helper.stderr.log')
        $markerEvidence = Wait-ProcessOutputMarker `
            -Handle $evidenceHelper `
            -Marker 'players online:' `
            -TimeoutSeconds 10 `
            -PassThruEvidence
        Assert-Equal 2 @($markerEvidence.SkippedLines).Count 'marker evidence did not retain the exact stdout interval before its fence'
        Assert-True ([string]$markerEvidence.SkippedLines[0] -like '*4 blocks filled') 'marker evidence lost the first skipped result line'
        Assert-True ([string]$markerEvidence.SkippedLines[1] -like '*Block placed') 'marker evidence lost the second skipped result line'
        Assert-True ($evidenceHelper.Process.WaitForExit(10000)) 'marker-evidence helper did not exit'
    }
    finally {
        try {
            Complete-TestLoggedProcess -Handle $evidenceHelper
        }
        catch {
            $evidenceHelperCleanupFailure = $_
        }
    }
    if ($null -ne $evidenceHelperCleanupFailure) {
        throw $evidenceHelperCleanupFailure
    }
    Assert-True ((Get-Content -Raw -LiteralPath $helper.StdoutPath).Contains('TEST_READY')) 'stdout was not preserved'
    Assert-True ((Get-Content -Raw -LiteralPath $helper.StderrPath).Contains('error-line')) 'stderr was not preserved'

    $drainPath = Join-Path $TempRoot 'drain-marker.stdout.log'
    [IO.File]::WriteAllText($drainPath, "OLD_ONE`nOLD_TWO`npartial", [Text.UTF8Encoding]::new($false))
    $drainHandle = [pscustomobject]@{
        StdoutPath = $drainPath
        StdoutMarkerCursor = [pscustomobject]@{ Offset = [long]0; PartialLine = ''; LineNumber = [uint64]0 }
    }
    $drainEvidence = Advance-ProcessOutputCursorToCurrentEnd -Handle $drainHandle
    Assert-Equal 2 $drainEvidence.complete_lines 'cursor drain did not consume every complete old stdout line'
    Assert-Equal 2 $drainHandle.StdoutMarkerCursor.LineNumber 'cursor drain did not preserve line numbering'
    Assert-Equal 'partial' $drainHandle.StdoutMarkerCursor.PartialLine 'cursor drain did not preserve an incomplete UTF-8 line'
    [IO.File]::AppendAllText($drainPath, "-tail`nCURRENT_MARKER`n", [Text.UTF8Encoding]::new($false))
    $postDrain = Wait-ProcessOutputMarker -Handle $drainHandle -Marker 'CURRENT_MARKER' -TimeoutSeconds 5 -PassThruEvidence
    Assert-Equal 4 $postDrain.LineNumber 'post-drain marker line number was not continuous'
    Assert-Equal 'partial-tail' $postDrain.SkippedLines[0] 'cursor drain discarded or duplicated the partial line'

    $utf8DrainPath = Join-Path $TempRoot 'drain-partial-utf8.stdout.log'
    $utf8Prefix = [Text.Encoding]::UTF8.GetBytes('prefix ')
    $euroBytes = [Text.Encoding]::UTF8.GetBytes([string][char]0x20ac)
    $partialUtf8 = [byte[]]::new($utf8Prefix.Length + 2)
    [Array]::Copy($utf8Prefix, $partialUtf8, $utf8Prefix.Length)
    [Array]::Copy($euroBytes, 0, $partialUtf8, $utf8Prefix.Length, 2)
    [IO.File]::WriteAllBytes($utf8DrainPath, $partialUtf8)
    $utf8DrainHandle = [pscustomobject]@{
        StdoutPath = $utf8DrainPath
        StdoutMarkerCursor = [pscustomobject]@{ Offset = [long]0; PartialLine = ''; LineNumber = [uint64]0 }
    }
    $null = Advance-ProcessOutputCursorToCurrentEnd -Handle $utf8DrainHandle
    Assert-Equal $utf8Prefix.Length $utf8DrainHandle.StdoutMarkerCursor.Offset 'cursor drain advanced past an incomplete UTF-8 code point'
    Assert-Equal 'prefix ' $utf8DrainHandle.StdoutMarkerCursor.PartialLine 'cursor drain corrupted text before an incomplete UTF-8 code point'
    $completion = [byte[]]@($euroBytes[2]) + [Text.Encoding]::UTF8.GetBytes("`nUTF8_CURRENT`n")
    $stream = [IO.FileStream]::new($utf8DrainPath, [IO.FileMode]::Append, [IO.FileAccess]::Write, [IO.FileShare]::ReadWrite)
    try { $stream.Write($completion, 0, $completion.Length) } finally { $stream.Dispose() }
    $utf8PostDrain = Wait-ProcessOutputMarker -Handle $utf8DrainHandle -Marker 'UTF8_CURRENT' -TimeoutSeconds 5 -PassThruEvidence
    Assert-Equal "prefix $([char]0x20ac)" $utf8PostDrain.SkippedLines[0] 'cursor drain did not reconstruct the split UTF-8 code point exactly once'

    $orderedMarkerHelper = $null
    $orderedMarkerCleanupFailure = $null
    try {
        $orderedMarkerHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-Command', "[Console]::Out.WriteLine('CURSOR_FIRST'); [Console]::Out.WriteLine('CURSOR_SECOND')") `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'ordered-markers.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'ordered-markers.stderr.log')
        $firstMarkerEvidence = Wait-ProcessOutputMarker -Handle $orderedMarkerHelper -Marker 'CURSOR_FIRST' -TimeoutSeconds 10 -PassThruEvidence
        $secondMarkerEvidence = Wait-ProcessOutputMarker -Handle $orderedMarkerHelper -Marker 'CURSOR_SECOND' -TimeoutSeconds 10 -PassThruEvidence
        Assert-Equal 'CURSOR_FIRST' $firstMarkerEvidence.Line 'marker cursor returned the wrong first line'
        Assert-Equal 'CURSOR_SECOND' $secondMarkerEvidence.Line 'marker cursor lost the buffered second line'
        Assert-True ([uint64]$secondMarkerEvidence.LineNumber -gt [uint64]$firstMarkerEvidence.LineNumber) 'marker cursor did not preserve increasing stdout line positions'
        Assert-True ($orderedMarkerHelper.Process.WaitForExit(10000)) 'ordered marker helper did not exit'
    }
    finally {
        try {
            Complete-TestLoggedProcess -Handle $orderedMarkerHelper
        }
        catch {
            $orderedMarkerCleanupFailure = $_
        }
    }
    if ($null -ne $orderedMarkerCleanupFailure) {
        throw $orderedMarkerCleanupFailure
    }

    $reversedMarkerHelper = $null
    $reversedMarkerCleanupFailure = $null
    try {
        $reversedMarkerHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-Command', "[Console]::Out.WriteLine('HISTORICAL_MARKER'); [Console]::Out.WriteLine('CURRENT_MARKER')") `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'reversed-markers.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'reversed-markers.stderr.log')
        $null = Wait-ProcessOutputMarker -Handle $reversedMarkerHelper -Marker 'CURRENT_MARKER' -TimeoutSeconds 10 -PassThruEvidence
        Assert-ThrowsLike {
            Wait-ProcessOutputMarker -Handle $reversedMarkerHelper -Marker 'HISTORICAL_MARKER' -TimeoutSeconds 1 -PassThruEvidence
        } "timed out waiting for 'HISTORICAL_MARKER'*" 'marker wait rescanned and accepted an earlier stdout line'
        Assert-True ($reversedMarkerHelper.Process.WaitForExit(10000)) 'reversed marker helper did not exit'
    }
    finally {
        try {
            Complete-TestLoggedProcess -Handle $reversedMarkerHelper
        }
        catch {
            $reversedMarkerCleanupFailure = $_
        }
    }
    if ($null -ne $reversedMarkerCleanupFailure) {
        throw $reversedMarkerCleanupFailure
    }

    $udpHelper = $null
    $bufferedHelper = $null
    $udpHelperCleanupFailure = $null
    $bufferedHelperCleanupFailure = $null
    try {
        $udpReservation = New-ReservedUdpPort
        $udpPort = $udpReservation.Port
        $udpReservation.Client.Dispose()
        $udpServerScript = @'
$ErrorActionPreference = 'Stop'
$udp = [Net.Sockets.UdpClient]::new(__PORT__)
$udp.Client.ReceiveTimeout = 5000
$magic = [byte[]]@(0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78)
[Console]::Out.WriteLine('UDP_READY')
[Console]::Out.Flush()
try {
    foreach ($mode in @('wrong-id', 'wrong-magic', 'valid')) {
        $remote = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
        $request = $udp.Receive([ref]$remote)
        $response = [byte[]]::new(33)
        $response[0] = if ($mode -eq 'wrong-id') { 0x1b } else { 0x1c }
        if ($request.Length -ge 9) {
            [Array]::Copy($request, 1, $response, 1, 8)
        }
        if ($mode -ne 'wrong-magic') {
            [Array]::Copy($magic, 0, $response, 17, $magic.Length)
        }
        $null = $udp.Send($response, $response.Length, $remote)
    }
}
finally {
    $udp.Dispose()
}
'@.Replace('__PORT__', $udpPort.ToString([Globalization.CultureInfo]::InvariantCulture))
        $udpServerCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($udpServerScript))
        $udpHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-EncodedCommand', $udpServerCommand) `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'udp-helper.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'udp-helper.stderr.log')
        $null = Wait-ProcessOutputMarker -Handle $udpHelper -Marker 'UDP_READY' -TimeoutSeconds 10

        $bufferedChildScript = @'
$writer = [IO.StreamWriter]::new(
    [Console]::OpenStandardOutput(),
    [Text.UTF8Encoding]::new($false),
    4096
)
$writer.AutoFlush = $false
$writer.WriteLine('BUFFERED_READY')
$null = [Console]::In.ReadLine()
$writer.Dispose()
'@
        $bufferedChildCommand = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($bufferedChildScript))
        $bufferedHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-EncodedCommand', $bufferedChildCommand) `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'buffered-helper.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'buffered-helper.stderr.log')
        $readinessProbe = {
            Test-RakNetUnconnectedPong `
                -Address '127.0.0.1' `
                -Port $udpPort `
                -TimeoutMilliseconds 500
        }.GetNewClosure()
        $observed = Wait-ProcessOutputMarker `
            -Handle $bufferedHelper `
            -Marker 'BUFFERED_READY' `
            -TimeoutSeconds 10 `
            -ReadinessProbe $readinessProbe
        Assert-Equal 'BUFFERED_READY' $observed 'RakNet readiness did not release the buffered BDS marker wait'
        Assert-True (-not $bufferedHelper.Process.HasExited) 'buffered logging helper exited before alternate readiness was observed'
        Assert-True ((Get-Item -LiteralPath $bufferedHelper.StdoutPath).Length -eq 0) 'buffered marker unexpectedly reached the log before alternate readiness'
        Assert-True ($udpHelper.Process.WaitForExit(2000)) 'invalid RakNet responses were accepted before the valid pong'
    }
    finally {
        try {
            Complete-TestLoggedProcess -Handle $bufferedHelper
        }
        catch {
            $bufferedHelperCleanupFailure = $_
        }
        try {
            Complete-TestLoggedProcess -Handle $udpHelper
        }
        catch {
            $udpHelperCleanupFailure = $_
        }
    }
    if ($null -ne $bufferedHelperCleanupFailure) {
        throw $bufferedHelperCleanupFailure
    }
    if ($null -ne $udpHelperCleanupFailure) {
        throw $udpHelperCleanupFailure
    }
    Assert-True ((Get-Content -Raw -LiteralPath $bufferedHelper.StdoutPath).Contains('BUFFERED_READY')) 'alternate readiness lost continuously captured stdout'

    $eofHelper = $null
    $eofHelperCleanupFailure = $null
    try {
        $eofHelper = Start-LoggedProcess `
            -Executable (Join-Path $PSHOME 'powershell.exe') `
            -Arguments @('-NoProfile', '-Command', '[Console]::In.ReadToEnd() | Out-Null') `
            -WorkingDirectory $TempRoot `
            -StdoutPath (Join-Path $TempRoot 'eof.stdout.log') `
            -StderrPath (Join-Path $TempRoot 'eof.stderr.log')
        Stop-BoundedProcess -Handle $eofHelper -Kind 'core'
        Assert-True $eofHelper.Process.HasExited 'core-style EOF cleanup left its child running'
    }
    finally {
        try {
            Complete-TestLoggedProcess -Handle $eofHelper
        }
        catch {
            $eofHelperCleanupFailure = $_
        }
    }
    if ($null -ne $eofHelperCleanupFailure) {
        throw $eofHelperCleanupFailure
    }

    $bdsStopState = [pscustomobject]@{
        Commands = [Collections.Generic.List[string]]::new()
        FlushCount = 0
        CloseCount = 0
        WaitTimeout = 0
    }
    $bdsStopInput = [pscustomobject]@{ State = $bdsStopState }
    $bdsStopInput | Add-Member -MemberType ScriptMethod -Name WriteLine -Value {
        param([string]$Command)
        $this.State.Commands.Add($Command)
    }
    $bdsStopInput | Add-Member -MemberType ScriptMethod -Name Flush -Value {
        $this.State.FlushCount++
    }
    $bdsStopInput | Add-Member -MemberType ScriptMethod -Name Close -Value {
        $this.State.CloseCount++
    }
    $bdsStopProcess = [pscustomobject]@{
        StandardInput = $bdsStopInput
        HasExited = $false
        State = $bdsStopState
    }
    $bdsStopProcess | Add-Member -MemberType ScriptMethod -Name WaitForExit -Value {
        param([int]$Timeout)
        $this.State.WaitTimeout = $Timeout
        $this.HasExited = $true
        return $true
    }
    $bdsStopHandle = [pscustomobject]@{ Process = $bdsStopProcess }
    $bdsStopLog = Join-Path $TempRoot 'bds-stop.console.log'
    Stop-BoundedProcess `
        -Handle $bdsStopHandle `
        -Kind 'bds' `
        -BdsConsoleLogPath $bdsStopLog
    Assert-Equal 1 $bdsStopState.Commands.Count 'BDS cleanup did not write exactly one command'
    Assert-Equal 'stop' $bdsStopState.Commands[0] 'BDS cleanup wrote the wrong command'
    Assert-Equal 1 $bdsStopState.FlushCount 'BDS cleanup did not flush standard input exactly once'
    Assert-Equal 1 $bdsStopState.CloseCount 'BDS cleanup did not close standard input exactly once'
    Assert-Equal 20000 $bdsStopState.WaitTimeout 'BDS cleanup changed its graceful wait timeout'
    $loggedStopCommands = @(Get-Content -LiteralPath $bdsStopLog)
    Assert-Equal 1 $loggedStopCommands.Count 'BDS cleanup did not log exactly one command'
    Assert-Equal 'stop' $loggedStopCommands[0] 'BDS cleanup logged the wrong command'

    $expectedAssetBlobSha256 = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
    $metrics = [ordered]@{
        session_seconds = 900.0; world_ready = $true; requested_radius_chunks = 16
        received_radius_chunks = 16; publisher_radius_chunks = 16
        mutation_coordinate = @(1, 2, 3); visible_mutation_count = 1; frame_count = 1
        p50_frame_ms = 1.0; p95_frame_ms = 2.0; p99_frame_ms = 3.0; max_frame_ms = 4.0
        max_decode_ms = 1.0; max_mesh_ms = 1.0; max_remesh_ms = 1.0
        teleport_settle_ms = $null; forced_full_view_remesh_ms = $null
        max_mutation_to_visible_ms = 50.0; decode_error_count = 0
        rendered_sub_chunks = 1; resident_sub_chunks = 1; visible_sub_chunks = 1
        peak_admitted_world_events = 1; peak_admitted_heavy_events = 1
        peak_queued_decode_jobs = 1; peak_in_flight_decode_jobs = 1
        peak_completed_decode_results = 1; peak_pending_retry_requests = 1
        peak_outbound_requests = 1; peak_pending_mesh_jobs = 1
        peak_in_flight_mesh_jobs = 1; gpu_upload_bytes = 1
        assets = [ordered]@{
            source_tag = 'v1.26.30.32-preview'
            source_sha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
            blob_sha256 = $expectedAssetBlobSha256
            texture_layers = 372
            texture_bytes_including_mips = 1000
            material_count = 405
            missing_mapping_count = 0
            diagnostic_quad_count = 12
        }
        teleport_proof = [ordered]@{
            target = '0:65:65:16'; committed = '0:65:65:16'; ms = 1500.0
            view_generation = 7; transparent_sort_generation = 11; render_ready_ms = 1200.0; publisher_ms = 100.0
            first_level_ms = 200.0; last_level_ms = 600.0; level_events = 1089
            first_sub_ms = 250.0; last_sub_ms = 900.0; sub_events = 1089
            first_frame_sequence = 41; stable_frame_sequence = 42
            first_present_ms = 1300.0; first_gpu_ms = 1350.0
            stable_present_ms = 1400.0; stable_gpu_ms = 1500.0; frame_count = 90
            expected_manifest_count = 4; expected_manifest_hash = '1111222233334444'
            first_presented_manifest_count = 4; first_presented_manifest_hash = '1111222233334444'
            stable_presented_manifest_count = 4; stable_presented_manifest_hash = '1111222233334444'
            expected = 1089; loaded_target = 1089; missing_target = 0
            foreign_loaded = 0; foreign_requested = 0; foreign_resident = 0; source_leftover = 0
            resident_count = 3; resident_hash = 'aaaabbbbccccdddd'
            known_air_count = 1; known_air_hash = 'eeeeffff00001111'
            missing_target_instances = 0; unexpected_target_instances = 0; source_instances = 0
            foreign_instances = 0; stale_generation_instances = 0; orphan_allocations = 0
        }
        forced_full_view_remesh_proof = [ordered]@{
            target = '0:65:65:16'; committed = '0:65:65:16'; ms = 1500.0
            view_generation = 8; transparent_sort_generation = 12; render_ready_ms = 0.0
            first_frame_sequence = 43; stable_frame_sequence = 44
            first_present_ms = 1200.0; first_gpu_ms = 1300.0
            stable_present_ms = 1400.0; stable_gpu_ms = 1500.0; frame_count = 90
            expected_manifest_count = 4; expected_manifest_hash = '5555666677778888'
            first_presented_manifest_count = 4; first_presented_manifest_hash = '5555666677778888'
            stable_presented_manifest_count = 4; stable_presented_manifest_hash = '5555666677778888'
            expected = 1089; loaded_target = 1089; missing_target = 0
            foreign_loaded = 0; foreign_requested = 0; foreign_resident = 0; source_leftover = 0
            resident_count = 3; resident_hash = 'aaaabbbbccccdddd'
            known_air_count = 1; known_air_hash = 'eeeeffff00001111'
            missing_target_instances = 0; unexpected_target_instances = 0; source_instances = 0
            foreign_instances = 0; stale_generation_instances = 0; orphan_allocations = 0
        }
    }
    $null = Assert-MarkerMatchesProof -Marker $teleportMarker -Proof ([pscustomobject]$metrics.teleport_proof) -Kind Teleport -Label 'teleport proof'
    $metrics.teleport_proof.transparent_sort_generation = 13
    Assert-ThrowsLike {
        Assert-MarkerMatchesProof -Marker $teleportMarker -Proof ([pscustomobject]$metrics.teleport_proof) -Kind Teleport -Label 'teleport proof'
    } 'teleport proof marker/metrics mismatch for transparent_sort_generation*' 'full-view proof accepted a different presented transparent generation'
    $metrics.teleport_proof.transparent_sort_generation = 11
    $metricsPath = Join-Path $TempRoot 'validation-metrics.json'
    $steadyResourceArtifactPath = Join-Path $TempRoot 'steady-resources.json'
    $steadyArtifactSamples = @(1..30 | ForEach-Object {
        [pscustomobject]@{
            elapsed_seconds = [double]$_
            combined_rss_bytes = 350MB
            cpu_percent = 10.0
        }
    })
    $steadyArtifactTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    $steadyArtifact = New-SteadyResourceDocument `
        -Samples $steadyArtifactSamples `
        -DurationSeconds 30 `
        -Trigger $steadyArtifactTrigger
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath

    $transparentMetrics = [ordered]@{}
    foreach ($key in $metrics.Keys) {
        $transparentMetrics[$key] = $metrics[$key]
    }
    $transparentMetrics.transparent_sort_request_generation = 4
    $transparentMetrics.transparent_sort_result_generation = 4
    $transparentMetrics.transparent_sort_committed_generation = 4
    $transparentMetrics.transparent_sort_encoded_generation = 4
    $transparentMetrics.transparent_sort_presented_generation = 4
    $transparentMetrics.transparent_sort_ref_count = 10
    $transparentMetrics.transparent_sort_cpu_ms = 0.25
    $transparentMetrics.transparent_sort_request_to_commit_ms = 3.5
    $transparentMetrics.transparent_sort_staged_bytes = 160
    $transparentMetrics.transparent_sort_upload_bytes = 160
    $transparentMetrics.transparent_sort_stale_reject_count = 0
    $transparentMetrics.transparent_sort_ceiling_reject_count = 0
    $transparentMetrics.transparent_sort_active_slot_age_frames = 2
    $transparentMetrics.transparent_water_distinct_tint_count = 1
    $transparentMetrics.nontransparent_gpu_upload_bytes = 1
    $transparentMetrics.gpu_upload_bytes = 161
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    $transparentMetrics.p99_frame_ms = 16.6
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater -MaximumP99FrameMilliseconds (1000.0 / 60.0)
    foreach ($failingP99 in @(16.7, 17.5)) {
        $transparentMetrics.p99_frame_ms = $failingP99
        $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike {
            Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater -MaximumP99FrameMilliseconds (1000.0 / 60.0)
        } 'p99_frame_ms exceeded manifested maximum*' "water acceptance accepted p99_frame_ms=$failingP99 above 60fps"
    }
    $transparentMetrics.p99_frame_ms = 3.0
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    foreach ($field in @(
        'transparent_sort_request_generation', 'transparent_sort_result_generation',
        'transparent_sort_committed_generation', 'transparent_sort_encoded_generation',
        'transparent_sort_presented_generation', 'transparent_sort_ref_count',
        'transparent_sort_cpu_ms', 'transparent_sort_request_to_commit_ms',
        'transparent_sort_staged_bytes', 'transparent_sort_upload_bytes',
        'transparent_sort_active_slot_age_frames', 'transparent_water_distinct_tint_count'
    )) {
        $saved = $transparentMetrics[$field]
        $transparentMetrics[$field] = 0
        $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike {
            Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
        } "transparent water metric $field was zero*" "water acceptance accepted zero $field"
        $transparentMetrics[$field] = $saved
    }
    $transparentMetrics.transparent_sort_result_generation = 5
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent sort generations were not monotonic*' 'water acceptance accepted a result newer than its request'
    $transparentMetrics.transparent_sort_result_generation = 4
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater -MinimumTransparentWaterDistinctTintCount 2
    } 'transparent_water_distinct_tint_count must be at least 2*' 'configurable water acceptance accepted fewer runtime tints than its manifested minimum'
    $transparentMetrics.transparent_water_distinct_tint_count = 0
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent water metric transparent_water_distinct_tint_count was zero*' 'water acceptance accepted no runtime water tint'
    $transparentMetrics.transparent_water_distinct_tint_count = 1
    $transparentMetrics.transparent_sort_presented_generation = 3
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent presented generation did not equal committed generation*' 'water acceptance accepted an unpresented committed sort'
    $transparentMetrics.transparent_sort_presented_generation = 4
    $transparentMetrics.transparent_sort_encoded_generation = 3
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent encoded generation did not equal committed generation*' 'water acceptance accepted a committed sort whose draw was not encoded'
    $transparentMetrics.transparent_sort_encoded_generation = 4
    $transparentMetrics.transparent_sort_upload_bytes = 161
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'transparent sort upload exceeded staged bytes*' 'water acceptance accepted impossible sort upload accounting'
    $transparentMetrics.transparent_sort_upload_bytes = 160
    $transparentMetrics.gpu_upload_bytes = 162
    $transparentMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $metricsPath -RequireTransparentWater
    } 'gpu_upload_bytes did not equal nontransparent plus transparent uploads*' 'water acceptance accepted inexact GPU byte accounting'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath

    $approvedOpaqueBlobSha256 = 'af98e5ddd5532972bf99b9fc3bdd3819bb06b1d8696198f135a9d96ae27ca7ba'
    $opaqueBaselineMetrics = [ordered]@{
        session_seconds = 60.0095326
        world_ready = $true
        requested_radius_chunks = 16
        received_radius_chunks = 16
        publisher_radius_chunks = 16
        mutation_coordinate = @(27, 73, 91)
        visible_mutation_count = 1
        frame_count = 5732
        p50_frame_ms = 10.1
        p95_frame_ms = 14.3
        p99_frame_ms = 17.0
        max_frame_ms = 96.7656
        max_decode_ms = 1.6392
        max_mesh_ms = 10.3533
        max_remesh_ms = 27701.8793
        max_mutation_to_visible_ms = 48.663
        decode_error_count = 0
        rendered_sub_chunks = 9495
        resident_sub_chunks = 10445
        visible_sub_chunks = 4802
        peak_admitted_world_events = 27
        peak_admitted_heavy_events = 27
        peak_queued_decode_jobs = 3
        peak_in_flight_decode_jobs = 4
        peak_completed_decode_results = 20
        peak_pending_retry_requests = 0
        peak_outbound_requests = 3
        peak_pending_mesh_jobs = 20646
        peak_in_flight_mesh_jobs = 64
        gpu_upload_bytes = 25976256
        assets = [ordered]@{
            source_tag = 'v1.26.30.32-preview'
            source_sha256 = '12d5cddc03acd507e9e0bd412f2e94d34d0a1a855758af7a9eef61b03630ad7c'
            blob_sha256 = $approvedOpaqueBlobSha256
            texture_layers = 388
            texture_bytes_including_mips = 529232
            material_count = 421
            missing_mapping_count = 0
            diagnostic_quad_count = 588885
        }
    }
    $approvedOpaqueKeys = @(
        'session_seconds', 'world_ready', 'requested_radius_chunks', 'received_radius_chunks',
        'publisher_radius_chunks', 'mutation_coordinate', 'visible_mutation_count', 'frame_count',
        'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms', 'max_decode_ms',
        'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms', 'decode_error_count',
        'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks',
        'peak_admitted_world_events', 'peak_admitted_heavy_events', 'peak_queued_decode_jobs',
        'peak_in_flight_decode_jobs', 'peak_completed_decode_results', 'peak_pending_retry_requests',
        'peak_outbound_requests', 'peak_pending_mesh_jobs', 'peak_in_flight_mesh_jobs',
        'gpu_upload_bytes', 'assets'
    )
    $approvedOpaqueAssetKeys = @(
        'source_tag', 'source_sha256', 'blob_sha256', 'texture_layers',
        'texture_bytes_including_mips', 'material_count', 'missing_mapping_count',
        'diagnostic_quad_count'
    )
    Assert-Equal 31 @($opaqueBaselineMetrics.Keys).Count 'approved opaque fixture did not have exactly 31 top-level keys'
    Assert-Equal (($approvedOpaqueKeys | Sort-Object) -join ',') (@($opaqueBaselineMetrics.Keys | Sort-Object) -join ',') 'approved opaque fixture key set changed'
    Assert-Equal 8 @($opaqueBaselineMetrics.assets.Keys).Count 'approved opaque fixture did not have exactly eight asset keys'
    Assert-Equal (($approvedOpaqueAssetKeys | Sort-Object) -join ',') (@($opaqueBaselineMetrics.assets.Keys | Sort-Object) -join ',') 'approved opaque asset key set changed'
    Assert-True (-not $opaqueBaselineMetrics.Contains('teleport_settle_ms')) 'approved opaque fixture unexpectedly gained teleport_settle_ms'
    Assert-True (-not $opaqueBaselineMetrics.Contains('forced_full_view_remesh_ms')) 'approved opaque fixture unexpectedly gained forced_full_view_remesh_ms'
    Assert-True (-not $opaqueBaselineMetrics.Contains('teleport_proof')) 'approved opaque fixture unexpectedly gained teleport_proof'
    Assert-True (-not $opaqueBaselineMetrics.Contains('forced_full_view_remesh_proof')) 'approved opaque fixture unexpectedly gained forced_full_view_remesh_proof'

    $opaqueBaselineMetricsPath = Join-Path $TempRoot 'opaque-baseline-validation-metrics.json'
    $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
    Assert-ThrowsLike {
        Assert-AcceptanceMetrics -Path $opaqueBaselineMetricsPath
    } 'acceptance metrics are missing teleport_settle_ms' 'approved base schema unexpectedly passed the current metrics path'
    $opaqueBaselineArguments = @{
        Path = $opaqueBaselineMetricsPath
        OpaqueBaselineSchema = $true
        ExpectedMutationCoordinate = @(27, 73, 91)
        RequireAssets = $true
        ExpectedAssetBlobSha256 = $approvedOpaqueBlobSha256
    }
    $originalDurationSeconds = $DurationSeconds
    $DurationSeconds = 60
    try {
        $null = Assert-AcceptanceMetrics @opaqueBaselineArguments

        $missingOpaqueField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $missingOpaqueField.PSObject.Properties.Remove('gpu_upload_bytes')
        $missingOpaqueField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline metrics schema mismatch:*missing=gpu_upload_bytes*' 'opaque baseline schema accepted a missing approved key'

        $extraOpaqueField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $extraOpaqueField | Add-Member -MemberType NoteProperty -Name unexpected_field -Value 1
        $extraOpaqueField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline metrics schema mismatch:*extra=unexpected_field*' 'opaque baseline schema accepted an unknown key'

        $currentSchemaAsOpaque = $metrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $currentSchemaAsOpaque | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline metrics schema mismatch:*extra=*teleport_settle_ms*' 'opaque baseline switch accepted the current metrics schema'

        $missingOpaqueAssetField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $missingOpaqueAssetField.assets.PSObject.Properties.Remove('diagnostic_quad_count')
        $missingOpaqueAssetField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline asset schema mismatch:*missing=diagnostic_quad_count*' 'opaque baseline schema accepted a missing asset key'

        $extraOpaqueAssetField = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        $extraOpaqueAssetField.assets | Add-Member -MemberType NoteProperty -Name unexpected_asset_field -Value 1
        $extraOpaqueAssetField | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } 'opaque baseline asset schema mismatch:*extra=unexpected_asset_field*' 'opaque baseline schema accepted an unknown asset key'

        $opaqueSafetyCases = @(
            [pscustomobject]@{ Name = 'short session'; Pattern = 'session_seconds=*expected at least 60'; Mutate = { param($m) $m.session_seconds = 59.0 } },
            [pscustomobject]@{ Name = 'world not ready'; Pattern = 'world_ready was false'; Mutate = { param($m) $m.world_ready = $false } },
            [pscustomobject]@{ Name = 'requested radius'; Pattern = 'radius gate failed:*'; Mutate = { param($m) $m.requested_radius_chunks = 15 } },
            [pscustomobject]@{ Name = 'received radius'; Pattern = 'radius gate failed:*'; Mutate = { param($m) $m.received_radius_chunks = 15 } },
            [pscustomobject]@{ Name = 'publisher radius'; Pattern = 'radius gate failed:*'; Mutate = { param($m) $m.publisher_radius_chunks = 15 } },
            [pscustomobject]@{ Name = 'wrong mutation coordinate'; Pattern = 'mutation_coordinate did not match manifested target:*'; Mutate = { param($m) $m.mutation_coordinate = @(27, 73, 92) } },
            [pscustomobject]@{ Name = 'no visible mutation'; Pattern = 'visible_mutation_count was zero for target mutation evidence'; Mutate = { param($m) $m.visible_mutation_count = 0 } },
            [pscustomobject]@{ Name = 'no frames'; Pattern = 'frame_count was zero'; Mutate = { param($m) $m.frame_count = 0 } },
            [pscustomobject]@{ Name = 'no rendered chunks'; Pattern = 'rendered_sub_chunks was zero'; Mutate = { param($m) $m.rendered_sub_chunks = 0 } },
            [pscustomobject]@{ Name = 'no resident chunks'; Pattern = 'resident_sub_chunks was zero'; Mutate = { param($m) $m.resident_sub_chunks = 0 } },
            [pscustomobject]@{ Name = 'no visible chunks'; Pattern = 'visible_sub_chunks was zero'; Mutate = { param($m) $m.visible_sub_chunks = 0 } },
            [pscustomobject]@{ Name = 'no GPU uploads'; Pattern = 'gpu_upload_bytes was zero for opaque baseline'; Mutate = { param($m) $m.gpu_upload_bytes = 0 } },
            [pscustomobject]@{ Name = 'decode errors'; Pattern = 'decode_error_count=1, expected zero'; Mutate = { param($m) $m.decode_error_count = 1 } },
            [pscustomobject]@{ Name = 'missing mapping'; Pattern = 'asset missing_mapping_count=1, expected zero'; Mutate = { param($m) $m.assets.missing_mapping_count = 1 } },
            [pscustomobject]@{ Name = 'wrong source tag'; Pattern = 'asset source_tag did not match pinned source:*'; Mutate = { param($m) $m.assets.source_tag = 'wrong' } },
            [pscustomobject]@{ Name = 'wrong source hash'; Pattern = 'asset source_sha256 did not match pinned source:*'; Mutate = { param($m) $m.assets.source_sha256 = ('0' * 64) } },
            [pscustomobject]@{ Name = 'wrong blob hash'; Pattern = 'asset blob_sha256 did not match supplied blob:*'; Mutate = { param($m) $m.assets.blob_sha256 = ('0' * 64) } },
            [pscustomobject]@{ Name = 'no texture layers'; Pattern = 'asset metrics were not populated:*'; Mutate = { param($m) $m.assets.texture_layers = 0 } },
            [pscustomobject]@{ Name = 'no mip bytes'; Pattern = 'asset metrics were not populated:*'; Mutate = { param($m) $m.assets.texture_bytes_including_mips = 0 } },
            [pscustomobject]@{ Name = 'no materials'; Pattern = 'asset metrics were not populated:*'; Mutate = { param($m) $m.assets.material_count = 0 } }
        )
        foreach ($case in $opaqueSafetyCases) {
            $candidate = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            & $case.Mutate $candidate
            $candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
            Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } $case.Pattern "opaque baseline accepted unsafe $($case.Name)"
        }

        foreach ($field in @(
            'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms',
            'max_decode_ms', 'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms'
        )) {
            $candidate = $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            $candidate.$field = 'NaN'
            $candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
            Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueBaselineArguments } "opaque baseline $field was not finite:*" "opaque baseline accepted nonfinite $field"
        }

        $opaqueBaselineMetrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $opaqueBaselineMetricsPath
        $opaqueFullViewArguments = @{}
        foreach ($key in $opaqueBaselineArguments.Keys) { $opaqueFullViewArguments[$key] = $opaqueBaselineArguments[$key] }
        $opaqueFullViewArguments['RequireFullViewTeleport'] = $true
        Assert-ThrowsLike { Assert-AcceptanceMetrics @opaqueFullViewArguments } 'OpaqueBaselineSchema cannot be combined with full-view validation' 'opaque baseline schema weakened the full-view gate'
        Assert-True `
            ([regex]::IsMatch(
                $source,
                'if \(\$LeafForestBaseline\)[\s\S]*?OpaqueBaselineSchema',
                [Text.RegularExpressions.RegexOptions]::CultureInvariant
            )) `
            'live LeafForestBaseline path did not select the explicit opaque baseline schema'
    }
    finally {
        $DurationSeconds = $originalDurationSeconds
    }

    $metrics.teleport_settle_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 1500.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $fullViewArguments = @{
        Path = $metricsPath
        RequireFullViewTeleport = $true
        TeleportMarker = $teleportMarker
        ForcedRemeshMarker = $forcedMarker
        ExpectedTargetCohort = '0:65:65:16'
        SteadyResourceArtifactPath = $steadyResourceArtifactPath
        ExpectedMutationCoordinate = @(1, 2, 3)
        RequireAssets = $true
        ExpectedAssetBlobSha256 = $expectedAssetBlobSha256
    }
    $null = Assert-AcceptanceMetrics @fullViewArguments

    $metrics.visible_mutation_count = 0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'visible_mutation_count was zero for target mutation*' 'full-view leaf evidence accepted no visible target mutation'
    $metrics.visible_mutation_count = 1
    $metrics.mutation_coordinate = @(9, 9, 9)
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'mutation_coordinate did not match manifested target*' 'full-view leaf evidence accepted the source/wrong mutation coordinate'
    $metrics.mutation_coordinate = @(1, 2, 3)
    $metrics.assets.missing_mapping_count = 1
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'asset missing_mapping_count=1, expected zero*' 'leaf evidence accepted a missing asset mapping'
    $metrics.assets.missing_mapping_count = 0
    $metrics.assets.blob_sha256 = 'dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'asset blob_sha256 did not match supplied blob*' 'leaf evidence accepted metrics from the wrong asset blob'
    $metrics.assets.blob_sha256 = $expectedAssetBlobSha256
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath

    $staleResourceArtifact = $steadyArtifact | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    $staleResourceArtifact.trigger.target = '0:66:65:16'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($staleResourceArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'steady resource artifact trigger mismatch for target*' 'stale steady-resource trigger provenance passed validation'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    $tamperedResourceArtifact = $steadyArtifact | ConvertTo-Json -Depth 10 | ConvertFrom-Json
    $tamperedResourceArtifact.summary.max_combined_rss_bytes = 1
    $tamperedResourceArtifact.summary.mean_cpu_percent = 0.0
    $tamperedResourceArtifact.summary.p95_cpu_percent = 0.0
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($tamperedResourceArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'steady resource artifact summary did not match samples*' 'tampered steady-resource summary passed validation'
    [IO.File]::WriteAllText(
        $steadyResourceArtifactPath,
        ($steadyArtifact | ConvertTo-Json -Depth 10),
        [Text.UTF8Encoding]::new($false)
    )

    $singleFrameTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('frame_count=90', 'frame_count=1') `
        -Kind Teleport
    $singleFrameArguments = $fullViewArguments.Clone()
    $singleFrameArguments.TeleportMarker = $singleFrameTeleportMarker
    $metrics.teleport_proof.frame_count = 1
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @singleFrameArguments } 'teleport_proof.frame_count must cover at least two presented frames*' 'a one-frame presented interval passed validation'
    $metrics.teleport_proof.frame_count = 90

    $changedCohortRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('resident_hash=aaaabbbbccccdddd', 'resident_hash=0000000000000001') `
        -Kind ForcedRemesh
    $changedCohortArguments = $fullViewArguments.Clone()
    $changedCohortArguments.ForcedRemeshMarker = $changedCohortRemeshMarker
    $metrics.forced_full_view_remesh_proof.resident_hash = '0000000000000001'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @changedCohortArguments } 'full-view proof cohort changed between teleport and forced remesh at resident_hash*' 'forced remesh silently accepted a changed resident cohort'
    $metrics.forced_full_view_remesh_proof.resident_hash = 'aaaabbbbccccdddd'

    $changedManifestCountMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('manifest_count=4', 'manifest_count=5') `
        -Kind ForcedRemesh
    $changedManifestCountArguments = $fullViewArguments.Clone()
    $changedManifestCountArguments.ForcedRemeshMarker = $changedManifestCountMarker
    $metrics.forced_full_view_remesh_proof.expected_manifest_count = 5
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_count = 5
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_count = 5
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @changedManifestCountArguments } 'forced remesh expected manifest count changed from teleport*' 'forced remesh silently changed its mesh-bearing key count'
    $metrics.forced_full_view_remesh_proof.expected_manifest_count = 4
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_count = 4
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_count = 4

    $earlyRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('first_frame_sequence=43 stable_frame_sequence=44', 'first_frame_sequence=42 stable_frame_sequence=43') `
        -Kind ForcedRemesh
    $earlyRemeshArguments = $fullViewArguments.Clone()
    $earlyRemeshArguments.ForcedRemeshMarker = $earlyRemeshMarker
    $metrics.forced_full_view_remesh_proof.first_frame_sequence = 42
    $metrics.forced_full_view_remesh_proof.stable_frame_sequence = 43
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @earlyRemeshArguments } 'forced remesh frames were not later than teleport frames*' 'forced remesh reused the teleport stable frame'
    $metrics.forced_full_view_remesh_proof.first_frame_sequence = 43
    $metrics.forced_full_view_remesh_proof.stable_frame_sequence = 44

    $staleGenerationRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('view_generation=8', 'view_generation=7') `
        -Kind ForcedRemesh
    $staleGenerationArguments = $fullViewArguments.Clone()
    $staleGenerationArguments.ForcedRemeshMarker = $staleGenerationRemeshMarker
    $metrics.forced_full_view_remesh_proof.view_generation = 7
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @staleGenerationArguments } 'forced remesh view generation did not advance beyond teleport*' 'forced remesh reused the teleport view generation'
    $metrics.forced_full_view_remesh_proof.view_generation = 8

    $unchangedManifestRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('5555666677778888', '1111222233334444') `
        -Kind ForcedRemesh
    $unchangedManifestArguments = $fullViewArguments.Clone()
    $unchangedManifestArguments.ForcedRemeshMarker = $unchangedManifestRemeshMarker
    $metrics.forced_full_view_remesh_proof.expected_manifest_hash = '1111222233334444'
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_hash = '1111222233334444'
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_hash = '1111222233334444'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @unchangedManifestArguments } 'forced remesh expected manifest hash did not change from teleport*' 'forced remesh did not prove new mesh generations'
    $metrics.forced_full_view_remesh_proof.expected_manifest_hash = '5555666677778888'
    $metrics.forced_full_view_remesh_proof.first_presented_manifest_hash = '5555666677778888'
    $metrics.forced_full_view_remesh_proof.stable_presented_manifest_hash = '5555666677778888'

    $metrics.teleport_settle_ms = 2000.1
    $metrics.teleport_proof.ms = 2000.1
    $metrics.teleport_proof.stable_gpu_ms = 2000.1
    $slowTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('ms=1500.0000', 'ms=2000.1000') `
        -Kind Teleport
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $slowTeleportArguments = $fullViewArguments.Clone()
    $slowTeleportArguments.TeleportMarker = $slowTeleportMarker
    Assert-ThrowsLike `
        { Assert-AcceptanceMetrics @slowTeleportArguments } `
        'teleport_settle_ms failed the 2000ms gate*' `
        'over-budget end-to-end teleport with a fast remesh passed validation'
    Assert-True (Test-Path -LiteralPath $steadyResourceArtifactPath -PathType Leaf) 'resource artifact was not retained before the teleport SLA failure surfaced'

    $metrics.teleport_settle_ms = 1500.0
    $metrics.teleport_proof.ms = 1500.0
    $metrics.teleport_proof.stable_gpu_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 2000.1
    $metrics.forced_full_view_remesh_proof.ms = 2000.1
    $metrics.forced_full_view_remesh_proof.stable_gpu_ms = 2000.1
    $slowRemeshMarker = ConvertFrom-FullViewSettleMarker `
        -Line $forcedMarkerLine.Replace('ms=1500.0000', 'ms=2000.1000') `
        -Kind ForcedRemesh
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $slowRemeshArguments = $fullViewArguments.Clone()
    $slowRemeshArguments.ForcedRemeshMarker = $slowRemeshMarker
    Assert-ThrowsLike `
        { Assert-AcceptanceMetrics @slowRemeshArguments } `
        'forced_full_view_remesh_ms failed the 2000ms gate*' `
        'over-budget forced full-view remesh with a fast teleport passed validation'
    $metrics.forced_full_view_remesh_ms = $null
    $metrics.forced_full_view_remesh_proof.ms = 1500.0
    $metrics.forced_full_view_remesh_proof.stable_gpu_ms = 1500.0

    $metrics.teleport_settle_ms = 1500.0
    $metrics.forced_full_view_remesh_ms = 1500.0
    foreach ($field in @('missing_target', 'foreign_loaded', 'foreign_requested', 'foreign_resident', 'source_leftover')) {
        $metrics.teleport_proof[$field] = 1
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike `
            { Assert-AcceptanceMetrics @fullViewArguments } `
            "teleport_proof.$field*expected zero*" `
            "non-exact teleport cohort field $field passed validation"
        $metrics.teleport_proof[$field] = 0
    }
    $metrics.teleport_proof.loaded_target = 1088
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof loaded/expected cohort counts were not exact*' 'missing destination column passed validation'
    $metrics.teleport_proof.loaded_target = 1089

    $wrongCenterArguments = $fullViewArguments.Clone()
    $wrongCenterArguments.ExpectedTargetCohort = '0:66:65:16'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @wrongCenterArguments } 'teleport_proof target cohort mismatch*' 'wrong destination center passed validation'
    $wrongRadiusArguments = $fullViewArguments.Clone()
    $wrongRadiusArguments.ExpectedTargetCohort = '0:65:65:15'
    Assert-ThrowsLike { Assert-AcceptanceMetrics @wrongRadiusArguments } 'teleport_proof target cohort mismatch*' 'wrong destination radius passed validation'

    $metrics.teleport_proof.committed = $null
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof.committed was missing*' 'missing committed cohort passed validation'
    $metrics.teleport_proof.committed = '0:65:65:16'

    $overlappingCallbackMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('first_gpu_ms=1350.0000', 'first_gpu_ms=1450.0000') `
        -Kind Teleport
    $overlappingCallbackArguments = $fullViewArguments.Clone()
    $overlappingCallbackArguments.TeleportMarker = $overlappingCallbackMarker
    $metrics.teleport_proof.first_gpu_ms = 1450.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics @overlappingCallbackArguments
    $metrics.teleport_proof.first_gpu_ms = 1350.0

    $metrics.teleport_proof.stable_gpu_ms = 'NaN'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof.stable_gpu_ms was not finite*' 'nonfinite GPU-completion timestamp passed validation'
    $metrics.teleport_proof.stable_gpu_ms = 1390.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presentation timestamps were not monotonic*' 'nonmonotonic presentation timestamps passed validation'
    $metrics.teleport_proof.stable_gpu_ms = 1500.0

    $metrics.teleport_proof.stable_frame_sequence = 43
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof frame sequences were not adjacent*' 'non-adjacent presented frames passed validation'
    $metrics.teleport_proof.stable_frame_sequence = 42

    $metrics.teleport_proof.first_presented_manifest_count = 3
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presented manifest count did not equal expected*' 'partial presented manifest count passed validation'
    $metrics.teleport_proof.first_presented_manifest_count = 4
    $metrics.teleport_proof.stable_presented_manifest_hash = '9999000011112222'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @fullViewArguments } 'teleport_proof presented manifest hash did not equal expected*' 'wrong presented manifest hash passed validation'
    $metrics.teleport_proof.stable_presented_manifest_hash = '1111222233334444'

    foreach ($field in @('missing_target_instances', 'unexpected_target_instances', 'source_instances', 'foreign_instances', 'stale_generation_instances', 'orphan_allocations')) {
        $metrics.forced_full_view_remesh_proof[$field] = 1
        $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
        Assert-ThrowsLike `
            { Assert-AcceptanceMetrics @fullViewArguments } `
            "forced_full_view_remesh_proof.$field*expected zero*" `
            "forced-remesh render counter $field passed validation"
        $metrics.forced_full_view_remesh_proof[$field] = 0
    }

    $mismatchedTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('resident_hash=aaaabbbbccccdddd', 'resident_hash=0000000000000001') `
        -Kind Teleport
    $mismatchedMarkerArguments = $fullViewArguments.Clone()
    $mismatchedMarkerArguments.TeleportMarker = $mismatchedTeleportMarker
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @mismatchedMarkerArguments } 'teleport marker/metrics mismatch for resident_hash*' 'marker/metrics mismatch passed validation'

    $overCapTeleportMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('frame_count=90', 'frame_count=92') `
        -Kind Teleport
    $overCapArguments = $fullViewArguments.Clone()
    $overCapArguments.TeleportMarker = $overCapTeleportMarker
    $metrics.teleport_proof.frame_count = 92
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @overCapArguments } 'teleport_proof exceeded its 60fps cap*' 'per-teleport interval frame cap was not enforced'
    $metrics.teleport_proof.frame_count = 90

    $lateDecodeStageMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('last_sub_ms=900.0000', 'last_sub_ms=1250.0000') `
        -Kind Teleport
    $lateDecodeStageArguments = $fullViewArguments.Clone()
    $lateDecodeStageArguments.TeleportMarker = $lateDecodeStageMarker
    $metrics.teleport_proof.last_sub_ms = 1250.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @lateDecodeStageArguments } 'teleport_proof.last_sub_ms must be JSON null or a nonnegative finite value*' 'a target decode stage after render readiness passed validation'
    $metrics.teleport_proof.last_sub_ms = 900.0

    $missingStageMarker = ConvertFrom-FullViewSettleMarker `
        -Line $teleportMarkerLine.Replace('publisher_ms=100.0000', 'publisher_ms=null') `
        -Kind Teleport
    $metrics.teleport_proof.publisher_ms = $null
    $missingStageArguments = $fullViewArguments.Clone()
    $missingStageArguments.TeleportMarker = $missingStageMarker
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics @missingStageArguments
    $metrics.teleport_proof.publisher_ms = -1.0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-ThrowsLike { Assert-AcceptanceMetrics @missingStageArguments } 'teleport_proof.publisher_ms must be JSON null or a nonnegative finite value*' 'missing stage was serialized as -1 without failing'
    $metrics.teleport_proof.publisher_ms = 100.0

    $resourceSamples = @(
        [pscustomobject]@{ combined_rss_bytes = 300MB; cpu_percent = 5.0 },
        [pscustomobject]@{ combined_rss_bytes = 400MB; cpu_percent = 10.0 },
        [pscustomobject]@{ combined_rss_bytes = 350MB; cpu_percent = 15.0 }
    )
    $resourceSummary = Get-SteadyResourceSummary -Samples $resourceSamples
    Assert-Equal (400MB) $resourceSummary.max_combined_rss_bytes 'resource summary chose the wrong RSS maximum'
    Assert-Equal 10.0 $resourceSummary.mean_cpu_percent 'resource summary chose the wrong CPU mean'
    Assert-Equal 15.0 $resourceSummary.p95_cpu_percent 'resource summary chose the wrong CPU p95'
    $resourceTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $teleportMarker `
        -ForcedRemeshMarker $forcedMarker
    $resourceDocument = New-SteadyResourceDocument `
        -Samples $resourceSamples `
        -DurationSeconds 30 `
        -Trigger $resourceTrigger
    Assert-Equal 'rust-mcbe-steady-resources-v2' $resourceDocument.schema 'steady-resource schema did not identify trigger provenance'
    Assert-Equal 'FullViewPresented' $resourceDocument.trigger.kind 'steady-resource trigger kind changed'
    Assert-Equal '0:65:65:16' $resourceDocument.trigger.target 'steady-resource trigger lost its exact target'
    Assert-Equal 7 $resourceDocument.trigger.teleport_view_generation 'steady-resource trigger lost teleport generation'
    Assert-Equal 42 $resourceDocument.trigger.teleport_stable_frame_sequence 'steady-resource trigger lost teleport stable frame'
    Assert-Equal 8 $resourceDocument.trigger.forced_remesh_view_generation 'steady-resource trigger lost forced-remesh generation'
    Assert-Equal 44 $resourceDocument.trigger.forced_remesh_stable_frame_sequence 'steady-resource trigger lost forced-remesh stable frame'

    $metrics.publisher_radius_chunks = 4
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'publisher radius below 16 passed validation'
    $metrics.publisher_radius_chunks = 16
    $metrics.frame_count = 0
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'zero frame_count passed validation'
    $metrics.frame_count = 1
    $metrics.p99_frame_ms = 'not-finite'
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'nonnumeric p99 passed validation'

}
catch {
    $testFailure = $_
}
finally {
    try {
        if (Test-Path -LiteralPath $TempRoot) {
            Remove-Item -LiteralPath $TempRoot -Recurse -Force -ErrorAction Stop
        }
        if (Test-Path -LiteralPath $TempRoot) {
            throw "acceptance test temporary directory still exists after cleanup: $TempRoot"
        }
    }
    catch {
        $tempRootCleanupFailure = $_
    }
}

if ($null -ne $testFailure) {
    if ($null -ne $tempRootCleanupFailure) {
        Write-Warning "temporary-directory cleanup also failed: $($tempRootCleanupFailure.Exception.Message)"
    }
    throw $testFailure
}
if ($null -ne $tempRootCleanupFailure) {
    throw $tempRootCleanupFailure
}

Write-Output 'acceptance.ps1 dry-run tests: PASS'
