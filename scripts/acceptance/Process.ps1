function Invoke-CheckedBuild {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $previousErrorAction = $ErrorActionPreference
    Push-Location -LiteralPath $WorkingDirectory
    try {
        # Windows PowerShell 5.1 represents a native process's stderr as non-terminating
        # ErrorRecords. Compilers routinely use stderr for progress even when they exit 0.
        $ErrorActionPreference = 'Continue'
        & $Executable @Arguments 2>&1 |
            ForEach-Object { $_.ToString() } |
            Tee-Object -FilePath $LogPath
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
        Pop-Location
    }
    if ($exitCode -ne 0) {
        throw "build failed ($exitCode): $(Format-ResolvedCommand $Executable $Arguments)"
    }
}

function Start-LoggedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath
    )

    $fileOptions = [IO.FileOptions]::Asynchronous -bor [IO.FileOptions]::WriteThrough
    $stdoutStream = [IO.FileStream]::new(
        $StdoutPath,
        [IO.FileMode]::Create,
        [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite,
        1,
        $fileOptions
    )
    $stderrStream = [IO.FileStream]::new(
        $StderrPath,
        [IO.FileMode]::Create,
        [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite,
        1,
        $fileOptions
    )
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.Arguments = (@($Arguments | ForEach-Object { ConvertTo-CommandArgument $_ }) -join ' ')
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.CreateNoWindow = $false
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "failed to start $Executable"
        }
    }
    catch {
        $stdoutStream.Dispose()
        $stderrStream.Dispose()
        throw
    }
    return [pscustomobject]@{
        Process = $process
        StdoutPath = $StdoutPath
        StderrPath = $StderrPath
        StdoutStream = $stdoutStream
        StderrStream = $stderrStream
        StdoutCopy = $process.StandardOutput.BaseStream.CopyToAsync($stdoutStream)
        StderrCopy = $process.StandardError.BaseStream.CopyToAsync($stderrStream)
        StdoutMarkerCursor = [pscustomobject]@{
            Offset = [long]0
            PartialLine = ''
            LineNumber = [uint64]0
        }
    }
}

function Test-RakNetUnconnectedPong {
    param(
        [Parameter(Mandatory = $true)][string]$Address,
        [Parameter(Mandatory = $true)][ValidateRange(1, 65535)][int]$Port,
        [Parameter(Mandatory = $true)][ValidateRange(1, 5000)][int]$TimeoutMilliseconds
    )

    $magic = [byte[]]@(
        0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe,
        0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78
    )
    $probe = [byte[]]::new(33)
    $probe[0] = 0x01
    $sentAt = [BitConverter]::GetBytes([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    $guid = [BitConverter]::GetBytes([DateTime]::UtcNow.Ticks)
    if ([BitConverter]::IsLittleEndian) {
        [Array]::Reverse($sentAt)
        [Array]::Reverse($guid)
    }
    [Array]::Copy($sentAt, 0, $probe, 1, $sentAt.Length)
    [Array]::Copy($magic, 0, $probe, 9, $magic.Length)
    [Array]::Copy($guid, 0, $probe, 25, $guid.Length)

    $client = [Net.Sockets.UdpClient]::new()
    try {
        $client.Client.ReceiveTimeout = $TimeoutMilliseconds
        $client.Connect($Address, $Port)
        $null = $client.Send($probe, $probe.Length)
        $remote = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
        $response = $client.Receive([ref]$remote)
        if ($response.Length -lt 33 -or $response[0] -ne 0x1c) {
            return $false
        }
        for ($index = 0; $index -lt $magic.Length; $index++) {
            if ($response[17 + $index] -ne $magic[$index]) {
                return $false
            }
        }
        return $true
    }
    catch [Net.Sockets.SocketException] {
        return $false
    }
    finally {
        $client.Dispose()
    }
}

function Get-ContiguousProcessLogByteCount {
    param(
        [Parameter(Mandatory = $true)][byte[]]$Buffer,
        [Parameter(Mandatory = $true)][ValidateRange(0, [int]::MaxValue)][int]$Count
    )

    if ($Count -gt $Buffer.Length) {
        throw "process-log byte count exceeds buffer length: count=$Count length=$($Buffer.Length)"
    }
    for ($index = 0; $index -lt $Count; $index++) {
        if ($Buffer[$index] -eq 0) {
            return $index
        }
    }
    return $Count
}

function Advance-ProcessOutputCursorToCurrentEnd {
    param([Parameter(Mandatory = $true)]$Handle)

    $cursorProperty = $Handle.PSObject.Properties['StdoutMarkerCursor']
    if ($null -eq $cursorProperty) {
        $Handle | Add-Member -MemberType NoteProperty -Name StdoutMarkerCursor -Value ([pscustomobject]@{
            Offset = [long]0
            PartialLine = ''
            LineNumber = [uint64]0
        })
    }
    $cursor = $Handle.StdoutMarkerCursor
    $startOffset = [long]$cursor.Offset
    $reader = [IO.FileStream]::new(
        $Handle.StdoutPath,
        [IO.FileMode]::Open,
        [IO.FileAccess]::Read,
        [IO.FileShare]::ReadWrite
    )
    try {
        $null = $reader.Seek($startOffset, [IO.SeekOrigin]::Begin)
        $remaining = $reader.Length - $reader.Position
        if ($remaining -lt 0 -or $remaining -gt [int]::MaxValue) {
            throw "process stdout drain length was invalid: offset=$startOffset length=$($reader.Length)"
        }
        $bytes = [byte[]]::new([int]$remaining)
        $read = 0
        while ($read -lt $bytes.Length) {
            $count = $reader.Read($bytes, $read, $bytes.Length - $read)
            if ($count -eq 0) { break }
            $read += $count
        }
        $contiguous = if ($read -eq 0) { 0 } else { Get-ContiguousProcessLogByteCount -Buffer $bytes -Count $read }
        $decodeCount = $contiguous
        $strictUtf8 = [Text.UTF8Encoding]::new($false, $true)
        $decoded = $null
        while ($decodeCount -ge [Math]::Max(0, $contiguous - 3)) {
            try {
                $decoded = $strictUtf8.GetString($bytes, 0, $decodeCount)
                break
            }
            catch [Text.DecoderFallbackException] {
                $decodeCount--
            }
        }
        if ($null -eq $decoded) {
            throw 'process stdout drain encountered invalid UTF-8 outside a trailing partial code point'
        }
        $cursor.Offset = $startOffset + $decodeCount
        $cursor.PartialLine += $decoded
    }
    finally {
        $reader.Dispose()
    }

    $completeLines = 0
    while (($newline = $cursor.PartialLine.IndexOf("`n", [StringComparison]::Ordinal)) -ge 0) {
        $cursor.PartialLine = $cursor.PartialLine.Substring($newline + 1)
        $cursor.LineNumber = [uint64]$cursor.LineNumber + 1
        $completeLines++
    }
    return [pscustomobject][ordered]@{
        start_offset = $startOffset
        end_offset = [long]$cursor.Offset
        complete_lines = $completeLines
        line_number = [uint64]$cursor.LineNumber
        partial_line_length = $cursor.PartialLine.Length
    }
}

function Wait-ProcessOutputMarker {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][string]$Marker,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds,
        [scriptblock]$ReadinessProbe,
        [string]$RejectMarker,
        [switch]$PassThruEvidence
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $cursorProperty = $Handle.PSObject.Properties['StdoutMarkerCursor']
    if ($null -eq $cursorProperty) {
        $Handle | Add-Member -MemberType NoteProperty -Name StdoutMarkerCursor -Value ([pscustomobject]@{
            Offset = [long]0
            PartialLine = ''
            LineNumber = [uint64]0
        })
    }
    $cursor = $Handle.StdoutMarkerCursor
    $buffer = [byte[]]::new(65536)
    $skippedLines = [Collections.Generic.List[string]]::new()
    $finalFlushPerformed = $false
    while ([DateTime]::UtcNow -lt $deadline) {
        while (($newline = $cursor.PartialLine.IndexOf("`n", [StringComparison]::Ordinal)) -ge 0) {
            $line = $cursor.PartialLine.Substring(0, $newline).TrimEnd("`r")
            $cursor.PartialLine = $cursor.PartialLine.Substring($newline + 1)
            $cursor.LineNumber = [uint64]$cursor.LineNumber + 1
            if ($line.Contains($Marker)) {
                $evidence = [pscustomobject][ordered]@{
                    Line = $line
                    Marker = $Marker
                    LineNumber = [uint64]$cursor.LineNumber
                    ReadOffset = [long]$cursor.Offset
                    ObservedAtUtc = [DateTime]::UtcNow.ToString('o')
                    SkippedLines = @($skippedLines)
                }
                if ($PassThruEvidence) {
                    return $evidence
                }
                return $line
            }
            if (-not [string]::IsNullOrEmpty($RejectMarker) -and $line.Contains($RejectMarker)) {
                throw "observed rejected process output while waiting for '$Marker': $line"
            }
            $skippedLines.Add($line)
        }
        $reader = [IO.FileStream]::new(
            $Handle.StdoutPath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite
        )
        try {
            $null = $reader.Seek([long]$cursor.Offset, [IO.SeekOrigin]::Begin)
            while ($reader.Position -lt $reader.Length) {
                $wanted = [Math]::Min($buffer.Length, $reader.Length - $reader.Position)
                $read = $reader.Read($buffer, 0, [int]$wanted)
                if ($read -eq 0) {
                    break
                }
                $contiguousRead = Get-ContiguousProcessLogByteCount -Buffer $buffer -Count $read
                if ($contiguousRead -gt 0) {
                    $cursor.Offset = [long]$cursor.Offset + $contiguousRead
                    $cursor.PartialLine += [Text.Encoding]::UTF8.GetString($buffer, 0, $contiguousRead)
                }
                if ($contiguousRead -lt $read) {
                    break
                }
                if ($cursor.PartialLine.Length -gt 131072) {
                    $cursor.PartialLine = $cursor.PartialLine.Substring($cursor.PartialLine.Length - 131072)
                }
            }
        }
        finally {
            $reader.Dispose()
        }
        while (($newline = $cursor.PartialLine.IndexOf("`n", [StringComparison]::Ordinal)) -ge 0) {
            $line = $cursor.PartialLine.Substring(0, $newline).TrimEnd("`r")
            $cursor.PartialLine = $cursor.PartialLine.Substring($newline + 1)
            $cursor.LineNumber = [uint64]$cursor.LineNumber + 1
            if ($line.Contains($Marker)) {
                $evidence = [pscustomobject][ordered]@{
                    Line = $line
                    Marker = $Marker
                    LineNumber = [uint64]$cursor.LineNumber
                    ReadOffset = [long]$cursor.Offset
                    ObservedAtUtc = [DateTime]::UtcNow.ToString('o')
                    SkippedLines = @($skippedLines)
                }
                if ($PassThruEvidence) {
                    return $evidence
                }
                return $line
            }
            if (-not [string]::IsNullOrEmpty($RejectMarker) -and $line.Contains($RejectMarker)) {
                throw "observed rejected process output while waiting for '$Marker': $line"
            }
            $skippedLines.Add($line)
        }
        if (-not $Handle.Process.HasExited -and
            $null -ne $ReadinessProbe -and
            (& $ReadinessProbe)) {
            if ($PassThruEvidence) {
                return [pscustomobject][ordered]@{
                    Line = $Marker
                    Marker = $Marker
                    LineNumber = [uint64]$cursor.LineNumber
                    ReadOffset = [long]$cursor.Offset
                    ObservedAtUtc = [DateTime]::UtcNow.ToString('o')
                    SkippedLines = @($skippedLines)
                }
            }
            return $Marker
        }
        if ($Handle.Process.HasExited -and $Handle.StdoutCopy.IsCompleted -and -not $finalFlushPerformed) {
            $Handle.StdoutStream.Flush()
            $finalFlushPerformed = $true
            continue
        }
        if ($Handle.Process.HasExited -and $Handle.StdoutCopy.IsCompleted) {
            break
        }
        Start-Sleep -Milliseconds 100
    }
    throw "timed out waiting for '$Marker'; process exit=$($Handle.Process.HasExited) log=$($Handle.StdoutPath)"
}

function Complete-ProcessLogs {
    param($Handle)
    if ($null -eq $Handle) {
        return
    }
    foreach ($copy in @($Handle.StdoutCopy, $Handle.StderrCopy)) {
        try {
            if (-not $copy.Wait(10000)) {
                Write-Warning 'timed out flushing a child log stream'
            }
        }
        catch {
            Write-Warning "child log stream failed: $_"
        }
    }
    foreach ($stream in @($Handle.StdoutStream, $Handle.StderrStream)) {
        try {
            $stream.Flush($true)
        }
        finally {
            $stream.Dispose()
        }
    }
}

function Stop-BoundedProcess {
    param(
        $Handle,
        [Parameter(Mandatory = $true)][ValidateSet('app', 'core', 'bds')][string]$Kind,
        [string]$BdsConsoleLogPath
    )

    if ($null -eq $Handle -or $Handle.Process.HasExited) {
        return
    }
    if ($Kind -eq 'bds') {
        try {
            try {
                Write-BdsConsoleCommand `
                    -Handle $Handle `
                    -Command 'stop' `
                    -LogPath $BdsConsoleLogPath
            }
            finally {
                $Handle.Process.StandardInput.Close()
            }
        }
        catch {
            Write-Warning "BDS graceful stop failed: $_"
        }
        $timeout = 20000
    }
    elseif ($Kind -eq 'core') {
        try {
            $Handle.Process.StandardInput.Close()
        }
        catch {
            Write-Warning "core EOF stop failed: $_"
        }
        $timeout = 10000
    }
    else {
        try {
            $null = $Handle.Process.CloseMainWindow()
        }
        catch {
            Write-Warning "app close request failed: $_"
        }
        $timeout = 10000
    }
    try {
        $exited = $Handle.Process.WaitForExit($timeout)
    }
    catch {
        if ($Handle.Process.HasExited) {
            return
        }
        throw
    }
    if (-not $exited) {
        Write-Warning "$Kind did not stop gracefully in $timeout ms; forcing termination"
        try {
            $Handle.Process.Kill()
        }
        catch {
            if (-not $Handle.Process.HasExited) {
                throw
            }
        }
        if (-not $Handle.Process.WaitForExit(10000)) {
            throw "$Kind remained alive after forced termination"
        }
    }
}
