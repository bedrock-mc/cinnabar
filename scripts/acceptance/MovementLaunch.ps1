function ConvertTo-EvidenceTarget {
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

function ConvertTo-MovementScenario {
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

function Get-EvidenceTargetEndpoint {
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

function New-MovementLaunchPlan {
    param(
        [Parameter(Mandatory = $true)][ValidateSet('Bds', 'Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')][string]$Target,
        [Parameter(Mandatory = $true)][string]$Endpoint,
        [Parameter(Mandatory = $true)][string]$RunId,
        [Parameter(Mandatory = $true)][string]$MetricsPath,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)][ValidateSet('CandidatePhysics', 'FastTransferWitness', 'FreeCameraSilence')]
        [string]$Scenario,
        [string]$Assets
    )
    $Target = ConvertTo-EvidenceTarget -Target $Target
    $Scenario = ConvertTo-MovementScenario -Scenario $Scenario
    if ($RunId -cnotmatch '^[0-9a-f]{32}$') { throw 'movement run ID must be exact lowercase 32-hex' }
    if ($Endpoint -cnotmatch '^[^\s:]+:[1-9][0-9]{0,4}$') { throw 'movement endpoint is invalid' }
    $remote = $Target -ine 'Bds'
    if ($remote -and $DurationSeconds -lt 300) {
        throw "movement acceptance on $Target requires at least 300 seconds of live evidence"
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
    $appArguments = @(
        '--address', $Endpoint,
        '--acceptance-seconds', $DurationSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
        '--metrics-out', $MetricsPath,
        '--movement-evidence-target', $Target
    )
    if ($Target -ieq 'Bds') { $appArguments += @('--native-networking', 'raknet', '--display-name', 'AcceptanceGuest') }
    if ($Scenario -ieq 'CandidatePhysics' -or $Scenario -ieq 'FastTransferWitness') { $appArguments += '--movement-candidate-physics' }
    elseif ($Scenario -ieq 'FreeCameraSilence') { $appArguments += '--auto-fly' }
    if (-not [string]::IsNullOrWhiteSpace($Assets)) { $appArguments += @('--assets', $Assets) }
    return [pscustomobject][ordered]@{
        Target = $Target
        Endpoint = $Endpoint
        RunId = $RunId
        Scenario = $Scenario
        AppArguments = $appArguments
    }
}

function Assert-MovementCleanTrackedSource {
    param([Parameter(Mandatory = $true)][string]$ProjectRoot)
    $lines = @(& git -C $ProjectRoot status --porcelain --untracked-files=normal)
    if ($LASTEXITCODE -ne 0) { throw 'failed to inspect movement source provenance' }
    if ($lines.Count -ne 0) {
        throw 'movement candidate evidence refuses a dirty source tree; commit reviewed changes first'
    }
}

function Assert-MovementExactCleanHead {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedCommit
    )
    Assert-MovementCleanTrackedSource -ProjectRoot $ProjectRoot
    $actual = (& git -C $ProjectRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0 -or $actual -cne $ExpectedCommit) {
        throw "movement source HEAD changed: expected=$ExpectedCommit actual=$actual"
    }
}

function Resolve-MovementContainedPath {
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
        throw "movement $Scope path escapes its contained root: $candidate"
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
                throw "movement contained scope crosses a reparse point: $existingScopePath"
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
                throw "movement contained path crosses a reparse point: $cursor"
            }
        }
    }
    if ($RequireLeaf) {
        if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {
            throw "movement required contained file does not exist: $candidate"
        }
        $leaf = Get-Item -LiteralPath $candidate -Force
        if (($leaf.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "movement required contained file is a reparse point: $candidate"
        }
    }
    return $candidate
}

function Initialize-MovementRunDirectory {
    param([Parameter(Mandatory = $true)][string]$Path)

    $fullPath = [IO.Path]::GetFullPath($Path)
    if (Test-Path -LiteralPath $fullPath) {
        $item = Get-Item -LiteralPath $fullPath -Force
        if (-not $item.PSIsContainer) { throw "movement run path is not a directory: $fullPath" }
        if (($item.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw "movement run directory cannot be a reparse point: $fullPath"
        }
        if (@(Get-ChildItem -LiteralPath $fullPath -Force).Count -ne 0) {
            throw "movement run directory must be create-new or empty: $fullPath"
        }
    }
    else {
        New-Item -ItemType Directory -Path $fullPath | Out-Null
    }
    return $fullPath
}
