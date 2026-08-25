function ConvertTo-Phase3Target {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')]
        [string]$Target
    )
    switch ($Target.ToLowerInvariant()) {
        'bds' { return 'Bds' }
        'lunar' { return 'Lunar' }
        'zeqa' { return 'Zeqa' }
        'lbsg' { return 'Lbsg' }
        'zeno' { return 'Zeno' }
        'venity' { return 'Venity' }
    }
}

function ConvertTo-Phase3Scenario {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateSet('CandidatePhysics', 'FastTransferWitness', 'FreeCameraSilence')]
        [string]$Scenario
    )
    switch ($Scenario.ToLowerInvariant()) {
        'candidatephysics' { return 'CandidatePhysics' }
        'fasttransferwitness' { return 'FastTransferWitness' }
        'freecamerasilence' { return 'FreeCameraSilence' }
    }
}

function Get-Phase3TargetEndpoint {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')][string]$Target,
        [string]$BdsEndpoint = '127.0.0.1:19132'
    )
    switch ($Target) {
        'Bds' { return $BdsEndpoint }
        'Lunar' { return 'pvp.lunarbedrock.com:19134' }
        'Zeqa' { return 'zeqa.net:19132' }
        'Lbsg' { return 'play.lbsg.net:19132' }
        'Zeno' { return 'zenomc.org:19197' }
        'Venity' { return 'play.venitymc.com:19132' }
    }
}

function Resolve-Phase3CoreExtraArguments {
    # Validates the optional bounded bedrock-core argument passthrough before any
    # build or run. Each element must be either a lowercase long-form flag token
    # matching '^-[a-z][a-z0-9-]*$' or a value token matching
    # '^[A-Za-z0-9._/:=-]+$'. A flag that takes a value is therefore supplied as
    # two elements; combined '-flag=value' tokens, shorthand '--flag' tokens,
    # uppercase flags, backslashes, whitespace, quotes, and shell metacharacters
    # are rejected so every accepted element always stays exactly one verbatim
    # process token.
    param([AllowEmptyCollection()][string[]]$CoreExtraArgs)

    if ($null -eq $CoreExtraArgs) { return @() }
    if (@($CoreExtraArgs).Count -gt 16) {
        throw 'Phase 3 core extra arguments exceed the 16-token bound'
    }
    foreach ($token in @($CoreExtraArgs)) {
        if ([string]::IsNullOrEmpty($token)) {
            throw 'Phase 3 core extra arguments contain an empty token'
        }
        if ($token.Length -gt 256) {
            throw 'Phase 3 core extra argument exceeds the 256-character bound'
        }
        if ($token.StartsWith('-')) {
            if ($token -cnotmatch '^-[a-z][a-z0-9-]*$') {
                throw "Phase 3 core extra argument flag token is not allowlisted: $token"
            }
        }
        elseif ($token -cnotmatch '^[A-Za-z0-9._/:=-]+$') {
            throw "Phase 3 core extra argument value token is not allowlisted: $token"
        }
    }
    return [string[]]$CoreExtraArgs
}

function New-Phase3LaunchPlan {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')][string]$Target,
        [Parameter(Mandatory = $true)][string]$Endpoint,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$SocketDirectory,
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)][ValidateSet('CandidatePhysics', 'FastTransferWitness', 'FreeCameraSilence')]
        [string]$Scenario,
        [string]$AuthCache,
        [string]$Assets,
        [string[]]$CoreExtraArgs = @()
    )
    $Target = ConvertTo-Phase3Target -Target $Target
    $Scenario = ConvertTo-Phase3Scenario -Scenario $Scenario
    if ($RunId -cnotmatch '^[0-9a-f]{32}$') { throw 'Phase 3 run ID must be exact lowercase 32-hex' }
    if ($Endpoint -cnotmatch '^[^\s:]+:[1-9][0-9]{0,4}$') { throw 'Phase 3 endpoint is invalid' }
    $remote = $Target -ine 'Bds'
    if ($remote -and [string]::IsNullOrWhiteSpace($AuthCache)) {
        throw "Phase 3 $Target requires an authenticated -AuthCache path; offline remote evidence is forbidden"
    }
    if ($remote -and $DurationSeconds -lt 300) {
        throw "Phase 3 $Target requires at least 300 seconds of live evidence"
    }
    if ($Scenario -ieq 'FastTransferWitness') {
        if ($Target -ine 'Lbsg' -or $Endpoint -cne 'play.lbsg.net:19132') {
            throw 'FastTransferWitness is fixed to the authenticated LBSG endpoint'
        }
        if ($DurationSeconds -lt 600) {
            throw 'FastTransferWitness requires at least 600 seconds for interactive transfer, screenshots, and movement'
        }
        if ([string]::IsNullOrWhiteSpace($Assets)) {
            throw 'FastTransferWitness requires the compiled vanilla asset carrier'
        }
    }
    $coreExtraArguments = @(Resolve-Phase3CoreExtraArguments -CoreExtraArgs $CoreExtraArgs)
    $coreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Endpoint)
    if ($remote) { $coreArguments += @('-auth-cache', $AuthCache) }
    if ($coreExtraArguments.Count -gt 0) { $coreArguments += $coreExtraArguments }
    $appArguments = @(
        '--socket-dir', $SocketDirectory,
        '--acceptance-seconds', $DurationSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--metrics-out', $MetricsPath,
        '--phase3-evidence-target', $Target
    )
    if ($Scenario -ieq 'CandidatePhysics' -or $Scenario -ieq 'FastTransferWitness') { $appArguments += '--phase3-candidate-physics' }
    elseif ($Scenario -ieq 'FreeCameraSilence') { $appArguments += '--auto-fly' }
    if (-not [string]::IsNullOrWhiteSpace($Assets)) { $appArguments += @('--assets', $Assets) }
    return [pscustomobject][ordered]@{
        Target = $Target
        Endpoint = $Endpoint
        RunId = $RunId
        Scenario = $Scenario
        CoreArguments = $coreArguments
        CoreExtraArguments = $coreExtraArguments
        AppArguments = $appArguments
    }
}

function Assert-Phase3CleanTrackedSource {
    param([Parameter(Mandatory = $true)][string]$ProjectRoot)
    $lines = @(& git -C $ProjectRoot status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0) { throw 'failed to inspect Phase 3 source provenance' }
    if ($lines.Count -ne 0) {
        throw 'Phase 3 candidate evidence refuses a dirty source tree; commit reviewed changes first'
    }
}

function Assert-Phase3ExactCleanHead {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedCommit
    )
    Assert-Phase3CleanTrackedSource -ProjectRoot $ProjectRoot
    $actual = (& git -C $ProjectRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -cne $ExpectedCommit) {
        throw "Phase 3 source HEAD changed: expected=$ExpectedCommit actual=$actual"
    }
}

function Resolve-Phase3ContainedPath {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][ValidateSet('Local', 'Acceptance')][string]$Scope,
        [switch]$RequireLeaf
    )
    $scopeRelative = if ($Scope -ceq 'Acceptance') { '.local\acceptance' } else { '.local' }
    $scopeRoot = [IO.Path]::GetFullPath((Join-Path $ProjectRoot $scopeRelative))
    $candidate = if ([IO.Path]::IsPathRooted($Path)) {
        [IO.Path]::GetFullPath($Path)
    }
    else {
        [IO.Path]::GetFullPath((Join-Path $ProjectRoot $Path))
    }
    $comparison = if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        [StringComparison]::OrdinalIgnoreCase
    }
    else { [StringComparison]::Ordinal }
    $prefix = $scopeRoot.TrimEnd([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar) +
        [IO.Path]::DirectorySeparatorChar
    if (-not $candidate.StartsWith($prefix, $comparison)) {
        throw "Phase 3 $Scope path escapes its contained root: $candidate"
    }
    $scopeCursor = [IO.Path]::GetFullPath($ProjectRoot)
    $scopePaths = [Collections.Generic.List[string]]::new()
    $scopePaths.Add($scopeCursor)
    foreach ($segment in $scopeRelative.Split(@('\', '/'), [StringSplitOptions]::RemoveEmptyEntries)) {
        $scopeCursor = Join-Path $scopeCursor $segment
        $scopePaths.Add($scopeCursor)
    }
    foreach ($existingScopePath in $scopePaths) {
        if (Test-Path -LiteralPath $existingScopePath) {
            $scopeItem = Get-Item -LiteralPath $existingScopePath -Force
            if (($scopeItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Phase 3 contained scope crosses a reparse point: $existingScopePath"
            }
        }
    }
    $cursor = $scopeRoot
    foreach ($segment in $candidate.Substring($prefix.Length).Split(
        @([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar),
        [StringSplitOptions]::RemoveEmptyEntries
    )) {
        $cursor = Join-Path $cursor $segment
        if (Test-Path -LiteralPath $cursor) {
            $item = Get-Item -LiteralPath $cursor -Force
            if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "Phase 3 contained path crosses a reparse point: $cursor"
            }
        }
    }
    if ($RequireLeaf) {
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "Phase 3 required contained file does not exist: $candidate"
        }
        $leaf = Get-Item -LiteralPath $candidate -Force
        if (($leaf.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "Phase 3 required contained file is a reparse point: $candidate"
        }
    }
    return $candidate
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
