function Assert-Phase2Evidence {
    param(
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][string]$ResourcesPath,
        [Parameter(Mandatory = $true)][string]$ClientLogPath,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode,
        [Parameter(Mandatory = $true)][double]$JoinMilliseconds,
        [switch]$RequireFullView
    )

    $metrics = Get-Content -Raw -LiteralPath $MetricsPath | ConvertFrom-Json
    $frameLimits = [ordered]@{
        p95_frame_ms = 16.6666666667
        p99_frame_ms = 16.6666666667
        max_frame_ms = 50.0
    }
    foreach ($field in $frameLimits.Keys) {
        if ($null -eq $metrics.PSObject.Properties[$field]) {
            throw "Phase 2 metrics are missing $field"
        }
        $value = $metrics.$field
        if ($null -eq $value -or $value -is [string] -or $value -is [bool]) {
            throw "Phase 2 frame gate contains a nonnumeric $field"
        }
        $number = [double]$value
        if ([double]::IsNaN($number) -or [double]::IsInfinity($number) -or
            $number -lt 0.0 -or $number -gt [double]$frameLimits[$field]) {
            throw "Phase 2 frame gate failed for $field`: $number"
        }
    }
    if ([double]::IsNaN($JoinMilliseconds) -or [double]::IsInfinity($JoinMilliseconds) -or
        $JoinMilliseconds -lt 0.0 -or $JoinMilliseconds -gt 2000.0) {
        throw "Phase 2 join gate failed: $JoinMilliseconds ms"
    }
    if ($RequireFullView) {
        foreach ($field in @('teleport_settle_ms', 'forced_full_view_remesh_ms')) {
            if ($null -eq $metrics.PSObject.Properties[$field] -or $null -eq $metrics.$field -or
                [double]$metrics.$field -gt 2000.0) {
                throw "Phase 2 full-view lifecycle gate failed for $field"
            }
        }
    }

    $resources = Get-Content -Raw -LiteralPath $ResourcesPath | ConvertFrom-Json
    $samples = @($resources.samples)
    if ([string]$resources.schema -cne 'rust-mcbe-phase2-resources-v1' -or
        [int]$resources.warmup_seconds -ne 30 -or
        [int]$resources.duration_seconds -ne 120 -or
        [int]$resources.processor_count -le 0 -or
        $samples.Count -ne 120 -or
        [int]$resources.summary.sample_count -ne 120) {
        throw 'Phase 2 resource evidence must contain a 30-second warmup and exactly 120 one-second samples'
    }
    $previous = 0.0
    $rss = [Collections.Generic.List[uint64]]::new()
    $cpu = [Collections.Generic.List[double]]::new()
    foreach ($sample in $samples) {
        $elapsed = [double]$sample.elapsed_seconds
        $delta = $elapsed - $previous
        if ($delta -lt 0.5 -or $delta -gt 1.5) {
            throw "Phase 2 resource cadence was not one second: delta=$delta"
        }
        $rss.Add([uint64]$sample.combined_rss_bytes)
        $cpu.Add([double]$sample.cpu_percent)
        $previous = $elapsed
    }
    $maxRss = [uint64](($rss | Measure-Object -Maximum).Maximum)
    $meanCpu = [double](($cpu | Measure-Object -Average).Average)
    $sortedCpu = @($cpu | Sort-Object)
    $p95Cpu = [double]$sortedCpu[[Math]::Ceiling(($sortedCpu.Count - 1) * 0.95)]
    if ($maxRss -gt 650MB -or $meanCpu -gt 15.0 -or $p95Cpu -gt 15.0) {
        throw "Phase 2 resource gate failed: rss=$maxRss mean_cpu=$meanCpu p95_cpu=$p95Cpu"
    }
    if ([uint64]$resources.summary.max_combined_rss_bytes -ne $maxRss -or
        [Math]::Abs([double]$resources.summary.mean_cpu_percent - $meanCpu) -gt 0.000001 -or
        [Math]::Abs([double]$resources.summary.p95_cpu_percent - $p95Cpu) -gt 0.000001) {
        throw 'Phase 2 resource summary does not match its samples'
    }

    $publicationLines = @(Get-Content -LiteralPath $ClientLogPath | Where-Object {
        $_.StartsWith('PHASE2_PUBLICATION=', [StringComparison]::Ordinal)
    })
    if ($publicationLines.Count -eq 0) {
        throw 'client log contains no PHASE2_PUBLICATION evidence'
    }
    $publication = $publicationLines[-1].Substring('PHASE2_PUBLICATION='.Length) | ConvertFrom-Json
    Assert-Phase2PublicationRecord -Record $publication -ExpectedPresentMode $ExpectedPresentMode
    $presentation = $publication.presentation
    if ([string]$presentation.build_profile -cne 'release' -or
        [string]$presentation.requested_present_mode -cne $ExpectedPresentMode.ToLowerInvariant() -or
        [string]$presentation.effective_present_mode -cne $ExpectedPresentMode.ToLowerInvariant() -or
        $presentation.present_mode_proven -isnot [bool] -or
        -not [bool]$presentation.present_mode_proven -or
        [string]$presentation.graphics_identity_sha256 -notmatch '^[0-9a-f]{64}$' -or
        [string]$presentation.assets_manifest_sha256 -notmatch '^[0-9a-f]{64}$') {
        throw 'PHASE2_PUBLICATION did not prove release build, effective present mode, graphics identity, and asset identity'
    }
    $publicationText = $publicationLines[-1].ToLowerInvariant()
    foreach ($forbidden in @('"path"', '"token"', '"auth"', '"payload"', '"credential"')) {
        if ($publicationText.Contains($forbidden)) {
            throw "PHASE2_PUBLICATION leaked forbidden field $forbidden"
        }
    }
    return [pscustomobject][ordered]@{
        metrics = $metrics
        resources = $resources
        publication = $publication
        join_milliseconds = $JoinMilliseconds
    }
}

function Measure-Phase2Resources {
    param(
        [Parameter(Mandatory = $true)]$ClientHandle,
        [Parameter(Mandatory = $true)]$CoreHandle,
        [Parameter(Mandatory = $true)][string]$OutputPath
    )

    $client = $ClientHandle.Process
    $core = $CoreHandle.Process
    $client.Refresh()
    $core.Refresh()
    $previousCpu = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
    $previousWall = 0.0
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $samples = [Collections.Generic.List[object]]::new()
    for ($second = 1; $second -le 150; $second++) {
        Start-Sleep -Seconds 1
        if ($client.HasExited -or $core.HasExited) {
            throw 'client or core exited before the Phase 2 performance window completed'
        }
        $client.Refresh()
        $core.Refresh()
        $wall = $stopwatch.Elapsed.TotalSeconds
        $cpuTotal = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
        $wallDelta = $wall - $previousWall
        $cpuDelta = $cpuTotal - $previousCpu
        if ($second -gt 30) {
            $samples.Add([pscustomobject][ordered]@{
                elapsed_seconds = $wall - 30.0
                combined_rss_bytes = [uint64]($client.WorkingSet64 + $core.WorkingSet64)
                cpu_percent = [Math]::Max(0.0, 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount))
            })
        }
        $previousWall = $wall
        $previousCpu = $cpuTotal
    }
    $stopwatch.Stop()
    $rss = @($samples | ForEach-Object { [uint64]$_.combined_rss_bytes })
    $cpu = @($samples | ForEach-Object { [double]$_.cpu_percent } | Sort-Object)
    $document = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-phase2-resources-v1'
        warmup_seconds = 30
        duration_seconds = 120
        processor_count = [Environment]::ProcessorCount
        samples = @($samples)
        summary = [pscustomobject][ordered]@{
            sample_count = $samples.Count
            max_combined_rss_bytes = [uint64](($rss | Measure-Object -Maximum).Maximum)
            mean_cpu_percent = [double](($cpu | Measure-Object -Average).Average)
            p95_cpu_percent = [double]$cpu[[Math]::Ceiling(($cpu.Count - 1) * 0.95)]
        }
    }
    Write-Phase2Json -Path $OutputPath -Value $document
    return $document
}
