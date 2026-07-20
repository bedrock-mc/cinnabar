function Get-OptionalCimValue {
    param([string]$ClassName, [string]$Property)
    try {
        return @((Get-CimInstance -ClassName $ClassName -ErrorAction Stop) | ForEach-Object { $_.$Property })
    }
    catch {
        return @("unavailable: $($_.Exception.Message)")
    }
}

function Get-SteadyResourceSummary {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][object[]]$Samples)

    $rssValues = @($Samples | ForEach-Object { [uint64]$_.combined_rss_bytes })
    $cpuValues = @($Samples | ForEach-Object { [double]$_.cpu_percent } | Sort-Object)
    $p95Index = [Math]::Ceiling(($cpuValues.Count - 1) * 0.95)
    return [pscustomobject][ordered]@{
        sample_count = $Samples.Count
        max_combined_rss_bytes = [uint64](($rssValues | Measure-Object -Maximum).Maximum)
        mean_cpu_percent = [double](($cpuValues | Measure-Object -Average).Average)
        p95_cpu_percent = [double]$cpuValues[$p95Index]
    }
}

function New-SteadyResourceTriggerEvidence {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('WorldReady', 'VisualFixtureReady', 'FullViewPresented')]
        [string]$Kind,
        [string]$WorldReadyMarker,
        $FixturePublication,
        $TeleportMarker,
        $ForcedRemeshMarker
    )

    switch ($Kind) {
        'WorldReady' {
            if ([string]::IsNullOrWhiteSpace($WorldReadyMarker) -or
                -not $WorldReadyMarker.StartsWith('RUST_MCBE_WORLD_READY ', [StringComparison]::Ordinal)) {
                throw 'WorldReady trigger requires the exact RUST_MCBE_WORLD_READY marker'
            }
            return [pscustomobject][ordered]@{
                kind = 'WorldReady'
                marker_sha256 = Get-Utf8Sha256 -Text $WorldReadyMarker
            }
        }
        'VisualFixtureReady' {
            if ($null -eq $FixturePublication) {
                throw 'VisualFixtureReady trigger requires FixturePublication'
            }
            foreach ($field in @('ManifestSha256', 'LayoutHash', 'Pose')) {
                $property = $FixturePublication.PSObject.Properties[$field]
                if ($null -eq $property -or [string]::IsNullOrWhiteSpace([string]$property.Value)) {
                    throw "VisualFixtureReady trigger requires FixturePublication.$field"
                }
            }
            foreach ($field in @('ManifestSha256', 'LayoutHash')) {
                if ([string]$FixturePublication.$field -notmatch '^[0-9a-f]{64}$') {
                    throw "VisualFixtureReady trigger received invalid FixturePublication.$field"
                }
            }
            return [pscustomobject][ordered]@{
                kind = 'VisualFixtureReady'
                pose = [string]$FixturePublication.Pose
                manifest_sha256 = [string]$FixturePublication.ManifestSha256
                fixture_layout_hash = [string]$FixturePublication.LayoutHash
            }
        }
        'FullViewPresented' {
            if ($null -eq $TeleportMarker) {
                throw 'FullViewPresented trigger requires TeleportMarker'
            }
            if ($null -eq $ForcedRemeshMarker) {
                throw 'FullViewPresented trigger requires ForcedRemeshMarker'
            }
            return New-FullViewResourceTrigger `
                -TeleportMarker $TeleportMarker `
                -ForcedRemeshMarker $ForcedRemeshMarker
        }
    }
}

function New-SteadyResourceDocument {
    param(
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][object[]]$Samples,
        [Parameter(Mandatory = $true)][ValidateRange(1, 300)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)]$Trigger
    )

    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-steady-resources-v2'
        trigger = $Trigger
        duration_seconds = $DurationSeconds
        processor_count = [Environment]::ProcessorCount
        samples = @($Samples)
        summary = Get-SteadyResourceSummary -Samples $Samples
    }
}

function Assert-SteadyResourceArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        $ExpectedTrigger,
        $TeleportMarker,
        $ForcedRemeshMarker
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "steady resource artifact was not written before full-view SLA validation: $Path"
    }
    $document = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    if ([string]$document.schema -cne 'rust-mcbe-steady-resources-v2') {
        throw "steady resource artifact schema was not rust-mcbe-steady-resources-v2: $($document.schema)"
    }
    if ([int]$document.duration_seconds -ne 30 -or @($document.samples).Count -ne 30) {
        throw "steady resource artifact did not contain 30 one-second samples: duration=$($document.duration_seconds) samples=$(@($document.samples).Count)"
    }
    if ([int]$document.processor_count -le 0) {
        throw "steady resource artifact processor_count was not positive: $($document.processor_count)"
    }
    if ($null -eq $document.summary) {
        throw 'steady resource artifact summary was missing'
    }
    if ([int]$document.summary.sample_count -ne 30) {
        throw "steady resource artifact summary sample_count was not 30: $($document.summary.sample_count)"
    }

    $expectedTrigger = if ($null -ne $ExpectedTrigger) {
        $ExpectedTrigger
    }
    else {
        New-SteadyResourceTriggerEvidence `
            -Kind FullViewPresented `
            -TeleportMarker $TeleportMarker `
            -ForcedRemeshMarker $ForcedRemeshMarker
    }
    if ($null -eq $document.trigger) {
        throw 'steady resource artifact trigger was missing'
    }
    foreach ($expectedProperty in $expectedTrigger.PSObject.Properties) {
        $actualProperty = $document.trigger.PSObject.Properties[$expectedProperty.Name]
        $actual = if ($null -eq $actualProperty) { '<missing>' } else { [string]$actualProperty.Value }
        $expected = [string]$expectedProperty.Value
        if ($actual -cne $expected) {
            throw "steady resource artifact trigger mismatch for $($expectedProperty.Name): expected=$expected actual=$actual"
        }
    }
    if (@($document.trigger.PSObject.Properties).Count -ne @($expectedTrigger.PSObject.Properties).Count) {
        $actualTriggerJson = $document.trigger | ConvertTo-Json -Compress -Depth 8
        $expectedTriggerJson = $expectedTrigger | ConvertTo-Json -Compress -Depth 8
        throw "steady resource artifact trigger shape mismatch: expected=$expectedTriggerJson actual=$actualTriggerJson"
    }

    $samples = @($document.samples)
    $previousElapsed = 0.0
    for ($index = 0; $index -lt $samples.Count; $index++) {
        $sample = $samples[$index]
        $elapsed = ConvertTo-EvidenceDouble `
            -Value (Get-RequiredEvidenceProperty -Evidence $sample -Name 'elapsed_seconds' -Label "steady resource sample $index") `
            -Field "steady resource sample $index elapsed_seconds"
        $rss = ConvertTo-EvidenceUInt64 `
            -Value (Get-RequiredEvidenceProperty -Evidence $sample -Name 'combined_rss_bytes' -Label "steady resource sample $index") `
            -Field "steady resource sample $index combined_rss_bytes"
        $cpu = ConvertTo-EvidenceDouble `
            -Value (Get-RequiredEvidenceProperty -Evidence $sample -Name 'cpu_percent' -Label "steady resource sample $index") `
            -Field "steady resource sample $index cpu_percent"
        $elapsedDelta = $elapsed - $previousElapsed
        if ($elapsedDelta -lt 0.5 -or $elapsedDelta -gt 2.5) {
            throw "steady resource sample cadence was not one second at index ${index}: delta=$elapsedDelta"
        }
        if ($rss -eq 0 -or $cpu -lt 0.0) {
            throw "steady resource sample $index contained an impossible value: rss=$rss cpu=$cpu"
        }
        $previousElapsed = $elapsed
    }

    $recomputed = Get-SteadyResourceSummary -Samples $samples
    $storedMaxRss = ConvertTo-EvidenceUInt64 `
        -Value $document.summary.max_combined_rss_bytes `
        -Field 'steady resource artifact max_combined_rss_bytes'
    $storedMeanCpu = ConvertTo-EvidenceDouble `
        -Value $document.summary.mean_cpu_percent `
        -Field 'steady resource artifact mean_cpu_percent'
    $storedP95Cpu = ConvertTo-EvidenceDouble `
        -Value $document.summary.p95_cpu_percent `
        -Field 'steady resource artifact p95_cpu_percent'
    if ($storedMaxRss -ne [uint64]$recomputed.max_combined_rss_bytes -or
        [Math]::Abs($storedMeanCpu - [double]$recomputed.mean_cpu_percent) -gt 0.000001 -or
        [Math]::Abs($storedP95Cpu - [double]$recomputed.p95_cpu_percent) -gt 0.000001) {
        throw "steady resource artifact summary did not match samples: stored_rss=$storedMaxRss recomputed_rss=$($recomputed.max_combined_rss_bytes) stored_mean=$storedMeanCpu recomputed_mean=$($recomputed.mean_cpu_percent) stored_p95=$storedP95Cpu recomputed_p95=$($recomputed.p95_cpu_percent)"
    }
    if ([uint64]$recomputed.max_combined_rss_bytes -gt 650MB) {
        throw "combined steady RSS exceeded 650 MiB: $($recomputed.max_combined_rss_bytes) bytes"
    }
    if ([double]$recomputed.mean_cpu_percent -gt 15.0 -or
        [double]$recomputed.p95_cpu_percent -gt 15.0) {
        throw "steady CPU exceeded 15%: mean=$($recomputed.mean_cpu_percent) p95=$($recomputed.p95_cpu_percent)"
    }
}

function Measure-SteadyResources {
    param(
        [Parameter(Mandatory = $true)]$ClientHandle,
        [Parameter(Mandatory = $true)]$CoreHandle,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        $Trigger,
        $TeleportMarker,
        $ForcedRemeshMarker,
        [ValidateRange(1, 300)][int]$DurationSeconds = 30
    )

    $client = $ClientHandle.Process
    $core = $CoreHandle.Process
    $client.Refresh()
    $core.Refresh()
    $previousCpuSeconds = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
    $previousWallSeconds = 0.0
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $samples = [Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $DurationSeconds; $index++) {
        Start-Sleep -Seconds 1
        if ($client.HasExited -or $core.HasExited) {
            throw 'client or core exited during steady resource sampling'
        }
        $client.Refresh()
        $core.Refresh()
        $wallSeconds = $stopwatch.Elapsed.TotalSeconds
        $cpuSeconds = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
        $wallDelta = $wallSeconds - $previousWallSeconds
        $cpuDelta = $cpuSeconds - $previousCpuSeconds
        $cpuPercent = 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount)
        $samples.Add([pscustomobject][ordered]@{
            elapsed_seconds = $wallSeconds
            combined_rss_bytes = [uint64]($client.WorkingSet64 + $core.WorkingSet64)
            cpu_percent = [Math]::Max(0.0, $cpuPercent)
        })
        $previousWallSeconds = $wallSeconds
        $previousCpuSeconds = $cpuSeconds
    }
    $stopwatch.Stop()

    $trigger = if ($null -ne $Trigger) {
        $Trigger
    }
    else {
        New-SteadyResourceTriggerEvidence `
            -Kind FullViewPresented `
            -TeleportMarker $TeleportMarker `
            -ForcedRemeshMarker $ForcedRemeshMarker
    }
    $document = New-SteadyResourceDocument `
        -Samples @($samples) `
        -DurationSeconds $DurationSeconds `
        -Trigger $trigger
    $summary = $document.summary
    $path = Join-Path $RunDirectory 'steady-resources.json'
    [IO.File]::WriteAllText(
        $path,
        ($document | ConvertTo-Json -Depth 6),
        [Text.UTF8Encoding]::new($false)
    )
    if ([uint64]$summary.max_combined_rss_bytes -gt 650MB) {
        throw "combined steady RSS exceeded 650 MiB: $($summary.max_combined_rss_bytes) bytes"
    }
    if ([double]$summary.mean_cpu_percent -gt 15.0 -or [double]$summary.p95_cpu_percent -gt 15.0) {
        throw "steady CPU exceeded 15%: mean=$($summary.mean_cpu_percent) p95=$($summary.p95_cpu_percent)"
    }
    return $document
}
