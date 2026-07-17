function Get-AcceptanceLibraryPaths {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $root = Join-Path (Split-Path -Parent $EntryPath) 'acceptance'
    return @(
        'Common.ps1',
        'RuntimePaths.ps1',
        'Process.ps1',
        'Bds.ps1',
        'Markers.ps1',
        'Galleries\Common.ps1',
        'Galleries\Leaves.ps1',
        'Galleries\CrossCrop.ps1',
        'Galleries\Aquatic.ps1',
        'Galleries\Water.ps1',
        'Galleries\FlowerBed.ps1',
        'Galleries\SlabStair.ps1',
        'Galleries\Vine.ps1',
        'Proofs.ps1',
        'Resources.ps1',
        'Metrics.ps1',
        'Orchestration\Validate.ps1',
        'Orchestration\Execute.ps1',
        'Orchestrator.ps1'
    ) | ForEach-Object { Join-Path $root $_ }
}

function Get-AcceptanceCompositeSource {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $parts = @([IO.File]::ReadAllText($EntryPath))
    $parts += @(
        Get-AcceptanceLibraryPaths -EntryPath $EntryPath |
            ForEach-Object { [IO.File]::ReadAllText($_) }
    )
    return $parts -join "`n"
}

function Get-Phase2ProjectRoot {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    return [IO.Path]::GetFullPath((Join-Path (Split-Path -Parent $EntryPath) '..'))
}

function Assert-Phase2RunId {
    param([Parameter(Mandatory = $true)][string]$RunId)

    if ($RunId -notmatch '^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$' -or
        $RunId -eq '.' -or $RunId -eq '..') {
        throw "invalid Phase 2 RunId: $RunId"
    }
}

function Assert-Phase2Duration {
    param([Parameter(Mandatory = $true)][int]$DurationSeconds)

    if ($DurationSeconds -lt 150) {
        throw 'DurationSeconds must be at least 150 for the 30-second warmup and uninterrupted 120-second sample'
    }
}

function Resolve-Phase2ContainedPath {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][ValidateSet('Local', 'Phase2')][string]$Scope
    )

    $root = if ($Scope -eq 'Phase2') {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot '.local\phase2'))
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot '.local'))
    }
    $candidate = if ([IO.Path]::IsPathRooted($Path)) {
        [IO.Path]::GetFullPath($Path)
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot $Path))
    }
    $prefix = $root.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) + [IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "path must remain beneath $root"
    }
    return $candidate
}

function New-Phase2RunDirectory {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][ValidateSet('remote', 'galleries', 'motion')][string]$Kind,
        [Parameter(Mandatory = $true)][string]$RunId
    )

    Assert-Phase2RunId -RunId $RunId
    $root = Resolve-Phase2ContainedPath -ProjectRoot $ProjectRoot -Path ".local\phase2\$Kind\placeholder" -Scope Phase2
    $root = Split-Path -Parent $root
    $runDirectory = Join-Path $root $RunId
    if (Test-Path -LiteralPath $runDirectory) {
        throw "Phase 2 RunId already exists: $RunId"
    }
    New-Item -ItemType Directory -Path $runDirectory -ErrorAction Stop | Out-Null
    return $runDirectory
}

function New-Phase2PerformanceContract {
    return [pscustomobject][ordered]@{
        warmup_seconds = 30
        steady_seconds = 120
        p95_frame_ms_max = 16.6666666667
        p99_frame_ms_max = 16.6666666667
        max_frame_ms_max = 50.0
        lifecycle_ms_max = 2000.0
        resource_sample_count = 120
        max_combined_rss_bytes = 681574400
        mean_cpu_percent_max = 15.0
        p95_cpu_percent_max = 15.0
    }
}

function Write-Phase2Json {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value
    )

    $encoded = $Value | ConvertTo-Json -Depth 20
    [IO.File]::WriteAllText($Path, $encoded + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
}

function Find-Phase2CompletedLunarDiagnostic {
    param([Parameter(Mandatory = $true)][string]$RemoteRoot)

    foreach ($file in @(Get-ChildItem -LiteralPath $RemoteRoot -Filter manifest.json -File -Recurse -ErrorAction SilentlyContinue)) {
        try {
            $candidate = Get-Content -Raw -LiteralPath $file.FullName | ConvertFrom-Json
            if ([string]$candidate.schema -cne 'rust-mcbe-phase2-remote-v1' -or
                [string]$candidate.server -cne 'Lunar' -or
                [string]$candidate.mode -cne 'Diagnostic' -or
                [string]$candidate.status -cne 'passed' -or
                $candidate.diagnostic_complete -isnot [bool] -or
                -not [bool]$candidate.diagnostic_complete) {
                continue
            }
            $hash = (Get-FileHash -LiteralPath $file.FullName -Algorithm SHA256).Hash
            if ($hash -notmatch '^[0-9A-F]{64}$') {
                continue
            }
            return [pscustomobject][ordered]@{
                Path = $file.FullName
                Sha256 = $hash
            }
        }
        catch { continue }
    }
    return $null
}

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
