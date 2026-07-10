$ErrorActionPreference = 'Stop'

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
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

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$AcceptanceScript = Join-Path $ProjectRoot 'scripts\acceptance.ps1'
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("rust-mcbe acceptance tests {0}" -f [guid]::NewGuid().ToString('N'))
$BdsDir = Join-Path $TempRoot 'bds source'
$MetricsOut = Join-Path $TempRoot 'metrics output\metrics.json'
$DryRunDirectory = Join-Path $ProjectRoot '.local\acceptance\dry-run'

try {
    New-Item -ItemType Directory -Path $BdsDir -Force | Out-Null
    Set-Content -LiteralPath (Join-Path $BdsDir 'bedrock_server.exe') -Value 'fixture' -NoNewline
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) "pre-existing dry-run artifact prevents an immutability assertion: $DryRunDirectory"

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
    foreach ($flag in @('--socket-dir', '--acceptance-seconds 900', '--metrics-out', '--auto-fly', '--no-vsync')) {
        Assert-True ($commands[2].Contains($flag)) "app command is missing $flag"
    }
    Assert-True ($commands[0].Contains('"')) 'path containing spaces was not quoted'
    Assert-True (-not (Test-Path -LiteralPath $DryRunDirectory)) 'dry-run created its run directory'
    Assert-True (-not (Test-Path -LiteralPath $MetricsOut)) 'dry-run wrote metrics'

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
    Assert-True ($source.Contains('CopyToAsync')) 'child logs are not streamed directly to files'
    Assert-True ($source.Contains('[IO.FileOptions]::WriteThrough')) 'child log files are not write-through'
    Assert-True (-not $source.Contains('ReadToEndAsync')) 'child logs are retained in memory'
    Assert-True ($source.Contains('-WorkingDirectory $ProjectRoot')) 'builds are not rooted at the project directory'
    Assert-True ($source.Contains("'9948b1729395d2e819fce28e079d4a7bfc67716c'")) 'gophertunnel metadata commit is not the repository pin'
    Assert-True ($source.Contains("'6f6806e821a579c183c44d786f76d9b358a2b825'")) 'Valentine metadata commit is not the repository pin'

    $env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY = '1'
    try {
        . $AcceptanceScript -DryRun -DurationSeconds 900 -BdsDir $BdsDir -MetricsOut $MetricsOut
    }
    finally {
        Remove-Item Env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -ErrorAction SilentlyContinue
    }

    $serverPropertiesPath = Join-Path $TempRoot 'server.properties'
    [IO.File]::WriteAllLines(
        $serverPropertiesPath,
        @(
            'server-port=19132'
            'server-portv6=19133'
            'online-mode=true'
            'allow-list=true'
            'enable-lan-visibility=true'
            'server-name=fixture'
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
        'server-name=fixture'
    )) {
        Assert-True ($serverProperties -contains $expectedProperty) "missing rewritten property: $expectedProperty"
    }

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

    Invoke-CheckedBuild `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "if ((Get-Location).Path -ne '$TempRoot') { exit 9 }") `
        -LogPath (Join-Path $TempRoot 'working-directory.log') `
        -WorkingDirectory $TempRoot

    $helper = Start-LoggedProcess `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', "Write-Output 'TEST_READY'; [Console]::Error.WriteLine('error-line')") `
        -WorkingDirectory $TempRoot `
        -StdoutPath (Join-Path $TempRoot 'helper.stdout.log') `
        -StderrPath (Join-Path $TempRoot 'helper.stderr.log')
    Assert-True ((Wait-ProcessOutputMarker -Handle $helper -Marker 'TEST_READY' -TimeoutSeconds 10) -eq 'TEST_READY') 'direct log stream did not expose readiness marker'
    Assert-True ($helper.Process.WaitForExit(10000)) 'logging helper did not exit'
    Complete-ProcessLogs $helper
    Assert-True ((Get-Content -Raw -LiteralPath $helper.StdoutPath).Contains('TEST_READY')) 'stdout was not preserved'
    Assert-True ((Get-Content -Raw -LiteralPath $helper.StderrPath).Contains('error-line')) 'stderr was not preserved'

    $eofHelper = Start-LoggedProcess `
        -Executable (Join-Path $PSHOME 'powershell.exe') `
        -Arguments @('-NoProfile', '-Command', '[Console]::In.ReadToEnd() | Out-Null') `
        -WorkingDirectory $TempRoot `
        -StdoutPath (Join-Path $TempRoot 'eof.stdout.log') `
        -StderrPath (Join-Path $TempRoot 'eof.stderr.log')
    Stop-BoundedProcess -Handle $eofHelper -Kind 'core'
    Assert-True $eofHelper.Process.HasExited 'core-style EOF cleanup left its child running'
    Complete-ProcessLogs $eofHelper

    $metrics = [ordered]@{
        session_seconds = 900.0; world_ready = $true; requested_radius_chunks = 16
        received_radius_chunks = 16; publisher_radius_chunks = 16
        mutation_coordinate = @(1, 2, 3); visible_mutation_count = 1; frame_count = 1
        p50_frame_ms = 1.0; p95_frame_ms = 2.0; p99_frame_ms = 3.0; max_frame_ms = 4.0
        max_decode_ms = 1.0; max_mesh_ms = 1.0; max_remesh_ms = 1.0
        max_mutation_to_visible_ms = 50.0; decode_error_count = 0
        rendered_sub_chunks = 1; resident_sub_chunks = 1; visible_sub_chunks = 1
        peak_admitted_world_events = 1; peak_admitted_heavy_events = 1
        peak_queued_decode_jobs = 1; peak_in_flight_decode_jobs = 1
        peak_completed_decode_results = 1; peak_pending_retry_requests = 1
        peak_outbound_requests = 1; peak_pending_mesh_jobs = 1
        peak_in_flight_mesh_jobs = 1; gpu_upload_bytes = 1
    }
    $metricsPath = Join-Path $TempRoot 'validation-metrics.json'
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    $null = Assert-AcceptanceMetrics -Path $metricsPath

    $metrics.publisher_radius_chunks = 4
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'publisher radius below 16 passed validation'
    $metrics.publisher_radius_chunks = 16
    $metrics.frame_count = 0
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'zero frame_count passed validation'
    $metrics.frame_count = 1
    $metrics.p99_frame_ms = 'not-finite'
    $metrics | ConvertTo-Json | Set-Content -LiteralPath $metricsPath
    Assert-Throws { Assert-AcceptanceMetrics -Path $metricsPath } 'nonnumeric p99 passed validation'

    Write-Output 'acceptance.ps1 dry-run tests: PASS'
}
finally {
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force
    }
}
