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
        $rakNetUnconnectedPong = ${function:Test-RakNetUnconnectedPong}
        Assert-True ($null -ne $rakNetUnconnectedPong) 'RakNet readiness helper was not imported into the acceptance test scope'
        $readinessProbe = {
            & $rakNetUnconnectedPong `
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
