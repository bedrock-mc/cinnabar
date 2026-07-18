function Get-Phase3TargetEndpoint {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg')][string]$Target,
        [string]$BdsEndpoint = '127.0.0.1:19132'
    )
    switch ($Target) {
        'Bds' { return $BdsEndpoint }
        'Lunar' { return 'pvp.lunarbedrock.com:19134' }
        'Zeqa' { return 'zeqa.net:19132' }
        'Lbsg' { return 'play.lbsg.net:19132' }
    }
}

function New-Phase3LaunchPlan {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg')][string]$Target,
        [Parameter(Mandatory = $true)][string]$Endpoint,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$SocketDirectory,
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)][ValidateSet('CandidatePhysics', 'FreeCameraSilence')]
        [string]$Scenario,
        [string]$AuthCache,
        [string]$Assets
    )
    if ($RunId -cnotmatch '^[0-9a-f]{32}$') { throw 'Phase 3 run ID must be exact lowercase 32-hex' }
    if ($Endpoint -cnotmatch '^[^\s:]+:[1-9][0-9]{0,4}$') { throw 'Phase 3 endpoint is invalid' }
    $remote = $Target -cne 'Bds'
    if ($remote -and [string]::IsNullOrWhiteSpace($AuthCache)) {
        throw "Phase 3 $Target requires an authenticated -AuthCache path; offline remote evidence is forbidden"
    }
    if ($remote -and $DurationSeconds -lt 300) {
        throw "Phase 3 $Target requires at least 300 seconds of live evidence"
    }
    $coreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Endpoint)
    if ($remote) { $coreArguments += @('-auth-cache', $AuthCache) }
    $appArguments = @(
        '--socket-dir', $SocketDirectory,
        '--acceptance-seconds', $DurationSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--metrics-out', $MetricsPath,
        '--phase3-evidence-target', $Target
    )
    if ($Scenario -ceq 'CandidatePhysics') { $appArguments += '--phase3-candidate-physics' }
    else { $appArguments += '--auto-fly' }
    if (-not [string]::IsNullOrWhiteSpace($Assets)) { $appArguments += @('--assets', $Assets) }
    return [pscustomobject][ordered]@{
        Target = $Target
        Endpoint = $Endpoint
        RunId = $RunId
        Scenario = $Scenario
        CoreArguments = $coreArguments
        AppArguments = $appArguments
    }
}

function Assert-Phase3CleanTrackedSource {
    param([Parameter(Mandatory = $true)][string]$ProjectRoot)
    $lines = @(& git -C $ProjectRoot status --porcelain --untracked-files=no)
    if ($LASTEXITCODE -ne 0) { throw 'failed to inspect Phase 3 source provenance' }
    if ($lines.Count -ne 0) {
        throw 'Phase 3 candidate evidence refuses a dirty tracked source tree; commit reviewed changes first'
    }
}

function Initialize-Phase3RunDirectory {
    param([Parameter(Mandatory = $true)][string]$Path)

    $fullPath = [IO.Path]::GetFullPath($Path)
    if (Test-Path -LiteralPath $fullPath) {
        $item = Get-Item -LiteralPath $fullPath -Force
        if (-not $item.PSIsContainer) { throw "Phase 3 run path is not a directory: $fullPath" }
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Phase 3 run directory cannot be a reparse point: $fullPath"
        }
        if (@(Get-ChildItem -LiteralPath $fullPath -Force).Count -ne 0) {
            throw "Phase 3 run directory must be create-new or empty: $fullPath"
        }
    }
    else {
        New-Item -ItemType Directory -Path $fullPath | Out-Null
    }
    return $fullPath
}

function New-Phase3EndpointPublicationGuard {
    param([Parameter(Mandatory = $true)][string]$SocketDirectory)

    $endpointPath = Join-Path ([IO.Path]::GetFullPath($SocketDirectory)) 'game.addr'
    if (Test-Path -LiteralPath $endpointPath) {
        throw "Phase 3 refuses a stale bridge endpoint: $endpointPath"
    }
    return [pscustomobject][ordered]@{
        EndpointPath = $endpointPath
        ObservedAbsentAtUtc = [DateTime]::UtcNow.ToString('o')
    }
}

function Wait-Phase3BridgeEndpoint {
    param(
        [Parameter(Mandatory = $true)]$Guard,
        [Parameter(Mandatory = $true)]$CoreHandle,
        [Parameter(Mandatory = $true)][ValidateRange(1, 300)][int]$TimeoutSeconds
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    while (-not (Test-Path -LiteralPath $Guard.EndpointPath -PathType Leaf)) {
        if ($CoreHandle.Process.HasExited) {
            throw "core exited before fresh endpoint publication with code $($CoreHandle.Process.ExitCode)"
        }
        if ([DateTime]::UtcNow -ge $deadline) {
            throw "timed out waiting for fresh endpoint $($Guard.EndpointPath)"
        }
        Start-Sleep -Milliseconds 100
    }
    if ($CoreHandle.Process.HasExited) {
        throw "core exited while publishing bridge endpoint with code $($CoreHandle.Process.ExitCode)"
    }
    $endpoint = (Get-Content -Raw -LiteralPath $Guard.EndpointPath).Trim()
    if ($endpoint -cnotmatch '^[^\s:]+:([1-9][0-9]{0,4})$' -or [int]$Matches[1] -gt 65535) {
        throw 'core published an invalid fresh bridge endpoint'
    }
    return [pscustomobject][ordered]@{
        Endpoint = $endpoint
        EndpointPath = [string]$Guard.EndpointPath
        CoreProcessId = [int]$CoreHandle.Process.Id
        ObservedAbsentAtUtc = [string]$Guard.ObservedAbsentAtUtc
        PublishedAtUtc = [DateTime]::UtcNow.ToString('o')
    }
}
