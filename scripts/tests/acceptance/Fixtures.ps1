function New-TestCrossCropAssets {
    param(
        [Parameter(Mandatory = $true)][string]$RegistryPath,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $registryBytes = [IO.File]::ReadAllBytes($RegistryPath)
    $visualCount = [BitConverter]::ToUInt32($registryBytes, 16)
    $bytes = [byte[]]::new(200 + 40 * $visualCount)
    [Text.Encoding]::ASCII.GetBytes('MCBEAS05').CopyTo($bytes, 0)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, 8)
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
    $templateCount = 28
    $materialCount = 1
    $quadCount = 49
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
    [Text.Encoding]::ASCII.GetBytes('MCBEAS05').CopyTo($bytes, 0)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, 8)
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
        if ($entry.family -notin @(7, 8) -and $entry.name -cne 'minecraft:vine') { continue }
        $offset = $visualOffset + 40 * [int]$entry.sequential_id
        $bytes[$offset + 25] = 3
        if ($entry.name -ceq 'minecraft:vine') {
            $mask = [int](($entry.canonical_state | ConvertFrom-Json).vine_direction_bits.value)
            $template = 12 + $mask
        }
        else {
            $half = if ($entry.family -eq 8) { [int](($entry.canonical_state | ConvertFrom-Json).upside_down_bit.value) } else { 0 }
            $template = if ($entry.family -eq 7) { 0 } elseif ($half -eq 0) { 1 } else { 6 }
        }
        [BitConverter]::GetBytes([uint32]$template).CopyTo($bytes, $offset + 28)
    }
    [BitConverter]::GetBytes([uint32]::MaxValue).CopyTo($bytes, $materialOffset + 8)
    for ($index = 0; $index -lt 12; $index++) {
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
    $vineQuadStart = 17
    for ($mask = 0; $mask -lt 16; $mask++) {
        $quadCountForMask = 0
        foreach ($bit in @(1, 2, 4, 8)) {
            if (($mask -band $bit) -ne 0) { $quadCountForMask++ }
        }
        $offset = $templateOffset + 12 * (12 + $mask)
        [BitConverter]::GetBytes([uint32]$vineQuadStart).CopyTo($bytes, $offset)
        [BitConverter]::GetBytes([uint32]$quadCountForMask).CopyTo($bytes, $offset + 4)
        [BitConverter]::GetBytes([uint32]0).CopyTo($bytes, $offset + 8)
        $vineQuadStart += $quadCountForMask
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

function Set-TestMcbeas05Seal {
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
    [Text.Encoding]::ASCII.GetBytes('MCBEAS05').CopyTo($bytes, 0)
    [BitConverter]::GetBytes([uint32]5).CopyTo($bytes, 8)
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
            $lines.Add('NO LOG FILE! - [2026-07-11 12:00:00:000 INFO] Block placed')
            continue
        }
        if ($command -notmatch '^fill (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) ') {
            throw "test helper cannot model fixture command: $command"
        }
        $volume = ([Math]::Abs([int]$Matches[4] - [int]$Matches[1]) + 1) *
            ([Math]::Abs([int]$Matches[5] - [int]$Matches[2]) + 1) *
            ([Math]::Abs([int]$Matches[6] - [int]$Matches[3]) + 1)
        $lines.Add("NO LOG FILE! - [2026-07-11 12:00:00:000 INFO] $volume blocks filled")
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
