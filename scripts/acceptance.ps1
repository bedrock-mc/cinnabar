[CmdletBinding()]
param(
    [switch]$DryRun,
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$DurationSeconds,
    [Parameter(Mandatory = $true)]
    [string]$BdsDir,
    [Parameter(Mandatory = $true)]
    [string]$MetricsOut
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$PinnedGophertunnelCommit = '9948b1729395d2e819fce28e079d4a7bfc67716c'
$PinnedValentineCommit = '6f6806e821a579c183c44d786f76d9b358a2b825'

function ConvertTo-CommandArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Format-ResolvedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments
    )

    $parts = @((ConvertTo-CommandArgument $Executable))
    $parts += @($Arguments | ForEach-Object { ConvertTo-CommandArgument $_ })
    return $parts -join ' '
}

function Assert-SafeRuntimeChild {
    param(
        [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
        [Parameter(Mandatory = $true)][string]$Candidate
    )

    $runtimeFull = [IO.Path]::GetFullPath($RuntimeDirectory).TrimEnd('\', '/')
    $candidateFull = [IO.Path]::GetFullPath($Candidate)
    $prefix = $runtimeFull + [IO.Path]::DirectorySeparatorChar
    if (-not $candidateFull.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to reset path outside stable runtime: $candidateFull"
    }
}

function Set-StableRuntime {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDirectory,
        [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
        [Parameter(Mandatory = $true)][string]$ExecutableName
    )

    $sourceFull = (Resolve-Path -LiteralPath $SourceDirectory).Path.TrimEnd('\', '/')
    $runtimeParent = (Resolve-Path -LiteralPath (Split-Path -Parent $RuntimeDirectory)).Path
    $runtimeFull = (Join-Path $runtimeParent (Split-Path -Leaf $RuntimeDirectory)).TrimEnd('\', '/')
    $sourcePrefix = $sourceFull + [IO.Path]::DirectorySeparatorChar
    $runtimePrefix = $runtimeFull + [IO.Path]::DirectorySeparatorChar
    if ($sourceFull.StartsWith($runtimePrefix, [StringComparison]::OrdinalIgnoreCase) -or
        $runtimeFull.StartsWith($sourcePrefix, [StringComparison]::OrdinalIgnoreCase) -or
        $sourceFull.Equals($runtimeFull, [StringComparison]::OrdinalIgnoreCase)) {
        throw "BDS source and stable runtime overlap: source=$sourceFull runtime=$runtimeFull"
    }

    New-Item -ItemType Directory -Path $runtimeFull -Force | Out-Null
    $runtimeInfo = Get-Item -LiteralPath $runtimeFull -Force
    if (($runtimeInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "stable runtime must not be a reparse point: $runtimeFull"
    }

    $markerPath = Join-Path $runtimeFull '.rust-mcbe-runtime-owner'
    $owner = "rust-mcbe-bds-runtime-v1`nsource=$($sourceFull.ToLowerInvariant())`n"
    $entries = @(Get-ChildItem -LiteralPath $runtimeFull -Force)
    if (Test-Path -LiteralPath $markerPath) {
        $currentOwner = [IO.File]::ReadAllText($markerPath)
        if ($currentOwner -ne $owner) {
            throw "stable runtime belongs to a different BDS source: $markerPath"
        }
    }
    elseif ($entries.Count -ne 0) {
        throw "refusing unmarked non-empty stable runtime: $runtimeFull"
    }
    else {
        [IO.File]::WriteAllText($markerPath, $owner, [Text.UTF8Encoding]::new($false))
    }

    $sourceExecutable = Join-Path $sourceFull $ExecutableName
    $runtimeExecutable = Join-Path $runtimeFull $ExecutableName
    $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $sourceExecutable).Hash
    $copyExecutable = $true
    if (Test-Path -LiteralPath $runtimeExecutable) {
        $runtimeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtimeExecutable).Hash
        $copyExecutable = $runtimeHash -ne $sourceHash
    }
    if ($copyExecutable) {
        $temporaryExecutable = Join-Path $runtimeFull ("bedrock-server-exe-{0}.tmp" -f [guid]::NewGuid().ToString('N'))
        Copy-Item -LiteralPath $sourceExecutable -Destination $temporaryExecutable
        try {
            if (Test-Path -LiteralPath $runtimeExecutable) {
                [IO.File]::Replace($temporaryExecutable, $runtimeExecutable, $null)
            }
            else {
                [IO.File]::Move($temporaryExecutable, $runtimeExecutable)
            }
        }
        finally {
            if (Test-Path -LiteralPath $temporaryExecutable) {
                Remove-Item -LiteralPath $temporaryExecutable -Force
            }
        }
    }

    foreach ($entry in @(Get-ChildItem -LiteralPath $runtimeFull -Force)) {
        if ($entry.Name -eq $ExecutableName -or $entry.Name -eq '.rust-mcbe-runtime-owner') {
            continue
        }
        Assert-SafeRuntimeChild -RuntimeDirectory $runtimeFull -Candidate $entry.FullName
        Remove-Item -LiteralPath $entry.FullName -Recurse -Force
    }
    foreach ($entry in @(Get-ChildItem -LiteralPath $sourceFull -Force)) {
        if ($entry.Name -eq $ExecutableName) {
            continue
        }
        Copy-Item -LiteralPath $entry.FullName -Destination (Join-Path $runtimeFull $entry.Name) -Recurse
    }
    return $runtimeExecutable
}

function Set-ServerProperties {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][int]$Port,
        [Parameter(Mandatory = $true)][int]$PortV6
    )

    $wanted = [ordered]@{
        'server-port' = $Port.ToString([Globalization.CultureInfo]::InvariantCulture)
        'server-portv6' = $PortV6.ToString([Globalization.CultureInfo]::InvariantCulture)
        'online-mode' = 'false'
        'allow-list' = 'false'
        'enable-lan-visibility' = 'false'
    }
    $lines = @([IO.File]::ReadAllLines($Path))
    foreach ($key in $wanted.Keys) {
        $matches = @()
        for ($index = 0; $index -lt $lines.Count; $index++) {
            if ($lines[$index] -match ('^' + [regex]::Escape($key) + '=')) {
                $matches += $index
            }
        }
        if ($matches.Count -ne 1) {
            throw "server.properties must contain exactly one $key entry"
        }
        $lines[$matches[0]] = "$key=$($wanted[$key])"
    }
    [IO.File]::WriteAllLines($Path, $lines, [Text.UTF8Encoding]::new($false))
}

function New-ReservedUdpPort {
    $client = [Net.Sockets.UdpClient]::new(0)
    [pscustomobject]@{
        Client = $client
        Port = ([Net.IPEndPoint]$client.Client.LocalEndPoint).Port
    }
}

function Invoke-CheckedBuild {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    Push-Location -LiteralPath $WorkingDirectory
    try {
        & $Executable @Arguments 2>&1 | Tee-Object -FilePath $LogPath
        $exitCode = $LASTEXITCODE
    }
    finally {
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
    }
}

function Wait-ProcessOutputMarker {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][string]$Marker,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $offset = [long]0
    $partialLine = ''
    $buffer = [byte[]]::new(65536)
    while ([DateTime]::UtcNow -lt $deadline) {
        $reader = [IO.FileStream]::new(
            $Handle.StdoutPath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite
        )
        try {
            $null = $reader.Seek($offset, [IO.SeekOrigin]::Begin)
            while ($reader.Position -lt $reader.Length) {
                $wanted = [Math]::Min($buffer.Length, $reader.Length - $reader.Position)
                $read = $reader.Read($buffer, 0, [int]$wanted)
                if ($read -eq 0) {
                    break
                }
                $offset += $read
                $partialLine += [Text.Encoding]::UTF8.GetString($buffer, 0, $read)
                while (($newline = $partialLine.IndexOf("`n", [StringComparison]::Ordinal)) -ge 0) {
                    $line = $partialLine.Substring(0, $newline).TrimEnd("`r")
                    $partialLine = $partialLine.Substring($newline + 1)
                    if ($line.Contains($Marker)) {
                        return $line
                    }
                }
                if ($partialLine.Length -gt 131072) {
                    $partialLine = $partialLine.Substring($partialLine.Length - 131072)
                }
            }
        }
        finally {
            $reader.Dispose()
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
        [Parameter(Mandatory = $true)][ValidateSet('app', 'core', 'bds')][string]$Kind
    )

    if ($null -eq $Handle -or $Handle.Process.HasExited) {
        return
    }
    if ($Kind -eq 'bds') {
        try {
            $Handle.Process.StandardInput.WriteLine('stop')
            $Handle.Process.StandardInput.Flush()
            $Handle.Process.StandardInput.Close()
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

function Get-OptionalCimValue {
    param([string]$ClassName, [string]$Property)
    try {
        return @((Get-CimInstance -ClassName $ClassName -ErrorAction Stop) | ForEach-Object { $_.$Property })
    }
    catch {
        return @("unavailable: $($_.Exception.Message)")
    }
}

function Assert-AcceptanceMetrics {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "app did not write acceptance metrics: $Path"
    }
    $metrics = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    $required = @(
        'session_seconds', 'world_ready', 'requested_radius_chunks', 'received_radius_chunks',
        'publisher_radius_chunks', 'mutation_coordinate', 'visible_mutation_count', 'frame_count',
        'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms', 'max_decode_ms',
        'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms', 'decode_error_count',
        'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks',
        'peak_admitted_world_events', 'peak_admitted_heavy_events', 'peak_queued_decode_jobs',
        'peak_in_flight_decode_jobs', 'peak_completed_decode_results', 'peak_pending_retry_requests',
        'peak_outbound_requests', 'peak_pending_mesh_jobs', 'peak_in_flight_mesh_jobs',
        'gpu_upload_bytes'
    )
    foreach ($field in $required) {
        if ($null -eq $metrics.PSObject.Properties[$field]) {
            throw "acceptance metrics are missing $field"
        }
    }
    if ([double]$metrics.session_seconds -lt $DurationSeconds) {
        throw "session_seconds=$($metrics.session_seconds), expected at least $DurationSeconds"
    }
    if (-not [bool]$metrics.world_ready) {
        throw 'world_ready was false'
    }
    if ([int]$metrics.requested_radius_chunks -ne 16 -or
        [int]$metrics.received_radius_chunks -ne 16 -or
        [int]$metrics.publisher_radius_chunks -ne 16) {
        throw "radius gate failed: requested=$($metrics.requested_radius_chunks) received=$($metrics.received_radius_chunks) publisher=$($metrics.publisher_radius_chunks)"
    }
    if ([uint64]$metrics.frame_count -eq 0) {
        throw 'frame_count was zero'
    }
    $p99 = [double]$metrics.p99_frame_ms
    if ([double]::IsNaN($p99) -or [double]::IsInfinity($p99)) {
        throw "p99_frame_ms was not finite: $($metrics.p99_frame_ms)"
    }
    if ([uint64]$metrics.decode_error_count -ne 0) {
        throw "decode_error_count=$($metrics.decode_error_count), expected zero"
    }
    foreach ($field in @('rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks', 'visible_mutation_count')) {
        if ([uint64]$metrics.$field -eq 0) {
            throw "$field was zero"
        }
    }
    if ([double]$metrics.max_mutation_to_visible_ms -gt 100.0) {
        throw "max_mutation_to_visible_ms=$($metrics.max_mutation_to_visible_ms), expected <= 100"
    }
    return $metrics
}

if ($env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -eq '1') {
    return
}

if ($DurationSeconds -lt 60) {
    throw 'DurationSeconds must be at least 60'
}
if ([string]::IsNullOrWhiteSpace($MetricsOut)) {
    throw 'MetricsOut must not be empty'
}
if (-not (Test-Path -LiteralPath $BdsDir -PathType Container)) {
    throw "BDS directory does not exist: $BdsDir"
}
$BdsDir = (Resolve-Path -LiteralPath $BdsDir).Path
$BdsExecutableName = 'bedrock_server.exe'
$BdsSourceExecutable = Join-Path $BdsDir $BdsExecutableName
if (-not (Test-Path -LiteralPath $BdsSourceExecutable -PathType Leaf)) {
    throw "BDS executable does not exist: $BdsSourceExecutable"
}

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$MetricsOut = [IO.Path]::GetFullPath($MetricsOut)
$RuntimeDirectory = Join-Path (Join-Path $ProjectRoot '.local\bds-runtime') (Split-Path -Leaf $BdsDir)
$RunName = if ($DryRun) { 'dry-run' } else { "{0}-{1}" -f [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'), $PID }
$RunDirectory = Join-Path (Join-Path $ProjectRoot '.local\acceptance') $RunName
$SocketDirectory = Join-Path $RunDirectory 'socket'
$CanonicalMetrics = Join-Path $RunDirectory 'app-metrics.json'
$BdsExecutable = Join-Path $RuntimeDirectory $BdsExecutableName
$CoreExecutable = Join-Path $ProjectRoot 'target\release\bedrock-core.exe'
$AppExecutable = Join-Path $ProjectRoot 'target\release\bedrock-client.exe'
$Upstream = '127.0.0.1:19132'
$BdsArguments = @()
$CoreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Upstream)
$AppArguments = @(
    '--socket-dir', $SocketDirectory,
    '--acceptance-seconds', $DurationSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
    '--metrics-out', $CanonicalMetrics,
    '--auto-fly',
    '--no-vsync'
)
$BdsCommand = Format-ResolvedCommand $BdsExecutable $BdsArguments
$CoreCommand = Format-ResolvedCommand $CoreExecutable $CoreArguments
$AppCommand = Format-ResolvedCommand $AppExecutable $AppArguments

if ($DryRun) {
    Write-Output "BDS_COMMAND=$BdsCommand"
    Write-Output "CORE_COMMAND=$CoreCommand"
    Write-Output "APP_COMMAND=$AppCommand"
    exit 0
}

$lease = $null
$portReservation = $null
$portV6Reservation = $null
$bdsHandle = $null
$coreHandle = $null
$appHandle = $null
$runFailure = $null
$metadata = $null

try {
    New-Item -ItemType Directory -Path $RunDirectory -Force | Out-Null

    $repoCommit = (& git -C $ProjectRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw 'failed to resolve repository commit'
    }
    $metadata = [ordered]@{
        status = 'preparing'
        started_utc = [DateTime]::UtcNow.ToString('o')
        repo_commit = $repoCommit
        pinned_gophertunnel_commit = $PinnedGophertunnelCommit
        pinned_valentine_commit = $PinnedValentineCommit
        bds_source = $BdsDir
        bds_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $BdsSourceExecutable).Hash.ToLowerInvariant()
        duration_seconds = $DurationSeconds
        build_app_command = 'cargo build --release -p bedrock-client --locked'
        build_core_command = Format-ResolvedCommand 'go' @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core')
        bds_command = $BdsCommand
        core_command = $CoreCommand
        app_command = $AppCommand
        machine = $env:COMPUTERNAME
        operating_system = Get-OptionalCimValue 'Win32_OperatingSystem' 'Caption'
        cpu = Get-OptionalCimValue 'Win32_Processor' 'Name'
        gpu = Get-OptionalCimValue 'Win32_VideoController' 'Name'
        display = Get-OptionalCimValue 'Win32_VideoController' 'VideoModeDescription'
    }
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    New-Item -ItemType Directory -Path (Split-Path -Parent $RuntimeDirectory) -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null

    $lockPath = $RuntimeDirectory + '.lock'
    $lease = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $BdsExecutable = Set-StableRuntime -SourceDirectory $BdsDir -RuntimeDirectory $RuntimeDirectory -ExecutableName $BdsExecutableName

    $portReservation = New-ReservedUdpPort
    $portV6Reservation = New-ReservedUdpPort
    $Upstream = "127.0.0.1:$($portReservation.Port)"
    Set-ServerProperties -Path (Join-Path $RuntimeDirectory 'server.properties') -Port $portReservation.Port -PortV6 $portV6Reservation.Port
    $CoreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Upstream)
    $BdsCommand = Format-ResolvedCommand $BdsExecutable $BdsArguments
    $CoreCommand = Format-ResolvedCommand $CoreExecutable $CoreArguments
    $AppCommand = Format-ResolvedCommand $AppExecutable $AppArguments
    Write-Output "BDS_COMMAND=$BdsCommand"
    Write-Output "CORE_COMMAND=$CoreCommand"
    Write-Output "APP_COMMAND=$AppCommand"

    $metadata['status'] = 'building'
    $metadata['bds_command'] = $BdsCommand
    $metadata['core_command'] = $CoreCommand
    $metadata['app_command'] = $AppCommand
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    Invoke-CheckedBuild -Executable 'cargo' -Arguments @('build', '--release', '-p', 'bedrock-client', '--locked') -LogPath (Join-Path $RunDirectory 'build-app.log') -WorkingDirectory $ProjectRoot
    Invoke-CheckedBuild -Executable 'go' -Arguments @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core') -LogPath (Join-Path $RunDirectory 'build-core.log') -WorkingDirectory $ProjectRoot

    $metadata['status'] = 'launching'
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    $portReservation.Client.Close()
    $portReservation = $null
    $portV6Reservation.Client.Close()
    $portV6Reservation = $null

    $bdsHandle = Start-LoggedProcess -Executable $BdsExecutable -Arguments $BdsArguments -WorkingDirectory $RuntimeDirectory -StdoutPath (Join-Path $RunDirectory 'bds.stdout.log') -StderrPath (Join-Path $RunDirectory 'bds.stderr.log')
    $null = Wait-ProcessOutputMarker -Handle $bdsHandle -Marker 'Server started.' -TimeoutSeconds 120

    $coreHandle = Start-LoggedProcess -Executable $CoreExecutable -Arguments $CoreArguments -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'core.stdout.log') -StderrPath (Join-Path $RunDirectory 'core.stderr.log')
    $endpointPath = Join-Path $SocketDirectory 'game.addr'
    $endpointDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $endpointPath -PathType Leaf)) {
        if ($coreHandle.Process.HasExited) {
            throw "core exited before endpoint publication with code $($coreHandle.Process.ExitCode)"
        }
        if ([DateTime]::UtcNow -ge $endpointDeadline) {
            throw "timed out waiting for core endpoint: $endpointPath"
        }
        Start-Sleep -Milliseconds 100
    }

    $appHandle = Start-LoggedProcess -Executable $AppExecutable -Arguments $AppArguments -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'app.stdout.log') -StderrPath (Join-Path $RunDirectory 'app.stderr.log')
    $coordinateMarker = Wait-ProcessOutputMarker -Handle $appHandle -Marker 'RUST_MCBE_MUTATION_COORDINATE=' -TimeoutSeconds 180
    $null = Wait-ProcessOutputMarker -Handle $appHandle -Marker 'RUST_MCBE_WORLD_READY ' -TimeoutSeconds 180

    if ($coordinateMarker -notmatch '^RUST_MCBE_MUTATION_COORDINATE=(-?\d+),(-?\d+),(-?\d+)$') {
        throw "invalid mutation marker: $coordinateMarker"
    }
    $coordinate = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
    $blocks = @('minecraft:gold_block', 'minecraft:diamond_block')
    $blockIndex = 0
    $nextMutation = [DateTime]::UtcNow
    $appDeadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds + 90)
    while (-not $appHandle.Process.HasExited) {
        if ([DateTime]::UtcNow -ge $appDeadline) {
            throw "app exceeded acceptance deadline of $($DurationSeconds + 90) seconds"
        }
        if ([DateTime]::UtcNow -ge $nextMutation) {
            $command = "setblock $($coordinate[0]) $($coordinate[1]) $($coordinate[2]) $($blocks[$blockIndex])"
            $bdsHandle.Process.StandardInput.WriteLine($command)
            $bdsHandle.Process.StandardInput.Flush()
            [IO.File]::AppendAllText((Join-Path $RunDirectory 'bds.console.log'), $command + [Environment]::NewLine)
            $blockIndex = ($blockIndex + 1) % $blocks.Count
            $nextMutation = [DateTime]::UtcNow.AddSeconds(2)
        }
        Start-Sleep -Milliseconds 100
    }
    if ($appHandle.Process.ExitCode -ne 0) {
        throw "app exited with code $($appHandle.Process.ExitCode)"
    }

    $metrics = Assert-AcceptanceMetrics -Path $CanonicalMetrics
    $metrics | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'validated-metrics.json') -Encoding UTF8
    $metadata['status'] = 'passed'
    $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
    Write-Output "ACCEPTANCE_ARTIFACTS=$RunDirectory"
    Write-Output "ACCEPTANCE_P99_FRAME_MS=$($metrics.p99_frame_ms)"
}
catch {
    $runFailure = $_
    if ($null -ne $metadata) {
        try {
            $metadata['status'] = 'failed'
            $metadata['failure'] = $_.Exception.Message
            $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
            $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        }
        catch {
            Write-Warning "failed to update failure metadata: $_"
        }
    }
    throw
}
finally {
    $cleanupErrors = [Collections.Generic.List[string]]::new()
    foreach ($child in @(
        [pscustomobject]@{ Handle = $appHandle; Kind = 'app' },
        [pscustomobject]@{ Handle = $coreHandle; Kind = 'core' },
        [pscustomobject]@{ Handle = $bdsHandle; Kind = 'bds' }
    )) {
        try {
            Stop-BoundedProcess -Handle $child.Handle -Kind $child.Kind
        }
        catch {
            $cleanupErrors.Add("stop $($child.Kind): $($_.Exception.Message)")
            Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
        }
    }
    foreach ($handle in @($appHandle, $coreHandle, $bdsHandle)) {
        try {
            Complete-ProcessLogs $handle
        }
        catch {
            $cleanupErrors.Add("complete child logs: $($_.Exception.Message)")
            Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
        }
    }
    foreach ($reservation in @($portReservation, $portV6Reservation)) {
        if ($null -ne $reservation) {
            try {
                $reservation.Client.Close()
            }
            catch {
                $cleanupErrors.Add("close UDP reservation: $($_.Exception.Message)")
            }
        }
    }
    if ($null -ne $lease) {
        try {
            $lease.Dispose()
        }
        catch {
            $cleanupErrors.Add("release BDS runtime lease: $($_.Exception.Message)")
        }
    }
    try {
        if (Test-Path -LiteralPath $CanonicalMetrics -PathType Leaf) {
            New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null
            Copy-Item -LiteralPath $CanonicalMetrics -Destination $MetricsOut -Force
        }
    }
    catch {
        $cleanupErrors.Add("copy requested metrics: $($_.Exception.Message)")
    }
    try {
        if (($null -ne $runFailure -or $cleanupErrors.Count -ne 0) -and (Test-Path -LiteralPath $RunDirectory -PathType Container)) {
            $failureDetails = @()
            if ($null -ne $runFailure) {
                $failureDetails += ($runFailure | Out-String)
            }
            $failureDetails += $cleanupErrors
            ($failureDetails -join [Environment]::NewLine) | Set-Content -LiteralPath (Join-Path $RunDirectory 'failure.txt') -Encoding UTF8
        }
    }
    catch {
        Write-Warning "failed to write cleanup report: $_"
    }
    if ($null -eq $runFailure -and $cleanupErrors.Count -ne 0) {
        if ($null -ne $metadata) {
            try {
                $metadata['status'] = 'failed'
                $metadata['failure'] = "cleanup: $($cleanupErrors -join '; ')"
                $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
                $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
            }
            catch {
                Write-Warning "failed to update cleanup-failure metadata: $_"
            }
        }
        throw "acceptance cleanup failed: $($cleanupErrors -join '; ')"
    }
}
