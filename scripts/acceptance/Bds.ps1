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
        'gamemode' = 'creative'
        'force-gamemode' = 'true'
        'allow-cheats' = 'true'
        'view-distance' = '16'
        'player-idle-timeout' = '0'
        'default-player-permission-level' = 'operator'
        'client-side-chunk-generation-enabled' = 'false'
    }
    $lines = @([IO.File]::ReadAllLines($Path))
    foreach ($key in $wanted.Keys) {
        $matchingIndices = @()
        for ($index = 0; $index -lt $lines.Count; $index++) {
            if ($lines[$index] -match ('^' + [regex]::Escape($key) + '=')) {
                $matchingIndices += $index
            }
        }
        if ($matchingIndices.Count -ne 1) {
            throw "server.properties must contain exactly one $key entry"
        }
        $lines[$matchingIndices[0]] = "$key=$($wanted[$key])"
    }
    [IO.File]::WriteAllLines($Path, $lines, [Text.UTF8Encoding]::new($false))
}

function Get-BdsWorldIdentityChild {
    param(
        [Parameter(Mandatory = $true)][string]$ParentPath,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Label,
        [switch]$AllowMissing
    )

    $path = [IO.Path]::Combine($ParentPath, $Name)
    try {
        return Get-Item -LiteralPath $path -Force -ErrorAction Stop
    }
    catch {
        if ($_.Exception -isnot [Management.Automation.ItemNotFoundException]) {
            throw
        }
        $unresolvedEntries = @(Get-ChildItem -LiteralPath $ParentPath -Force -ErrorAction Stop | Where-Object Name -CEQ $Name)
        if ($unresolvedEntries.Count -ne 0) {
            throw "$Label exists but could not be resolved safely: $path"
        }
        if ($AllowMissing) {
            return $null
        }
        throw "$Label does not exist: $path"
    }
}

function Get-BdsSourceWorldIdentity {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDirectory,
        [switch]$AllowMissingWorld
    )

    $sourceInputInfo = Get-Item -LiteralPath $SourceDirectory -Force -ErrorAction Stop
    if (-not $sourceInputInfo.PSIsContainer -or
        (($sourceInputInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "BDS source must be a non-reparse directory: $SourceDirectory"
    }
    $sourceFull = Get-CanonicalExistingDirectoryPath -Path $SourceDirectory
    $propertiesPath = [IO.Path]::Combine($sourceFull, 'server.properties')
    $propertiesInfo = Get-Item -LiteralPath $propertiesPath -Force -ErrorAction Stop
    if ($propertiesInfo.PSIsContainer -or
        (($propertiesInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "invalid BDS source server.properties: $propertiesPath"
    }
    $levelNameValues = [Collections.Generic.List[string]]::new()
    foreach ($line in [IO.File]::ReadAllLines($propertiesPath)) {
        if ($line.StartsWith('level-name=', [StringComparison]::Ordinal)) {
            $levelNameValues.Add($line.Substring('level-name='.Length))
        }
    }
    if ($levelNameValues.Count -ne 1 -or
        [string]::IsNullOrWhiteSpace($levelNameValues[0]) -or
        $levelNameValues[0] -in @('.', '..') -or
        $levelNameValues[0].IndexOfAny([char[]]@('\', '/')) -ge 0) {
        throw 'BDS source server.properties must contain exactly one safe nonempty level-name'
    }
    $levelName = $levelNameValues[0]
    $worldsPath = [IO.Path]::Combine($sourceFull, 'worlds')
    $worldsInfo = Get-BdsWorldIdentityChild `
        -ParentPath $sourceFull `
        -Name 'worlds' `
        -Label 'BDS source worlds directory' `
        -AllowMissing:$AllowMissingWorld
    if ($null -eq $worldsInfo) {
        return $null
    }
    if (-not $worldsInfo.PSIsContainer -or
        (($worldsInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "invalid BDS source worlds directory: $worldsPath"
    }
    $worldPath = [IO.Path]::Combine($worldsPath, $levelName)
    $worldInfo = Get-BdsWorldIdentityChild `
        -ParentPath $worldsPath `
        -Name $levelName `
        -Label 'BDS source world directory' `
        -AllowMissing:$AllowMissingWorld
    if ($null -eq $worldInfo) {
        return $null
    }
    if (-not $worldInfo.PSIsContainer -or
        (($worldInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "invalid BDS source world directory: $worldPath"
    }
    $worldFull = Get-CanonicalExistingDirectoryPath -Path $worldPath
    if (-not (Test-RuntimePathContains -Parent $worldsPath -Candidate $worldFull)) {
        throw "BDS source world escaped the worlds directory: $worldFull"
    }

    $entriesByPath = [Collections.Generic.Dictionary[string, object]]::new([StringComparer]::Ordinal)
    $directories = [Collections.Generic.Queue[string]]::new()
    $directories.Enqueue($worldFull)
    $fileCount = [uint64]0
    $totalBytes = [uint64]0
    while ($directories.Count -ne 0) {
        $directory = $directories.Dequeue()
        foreach ($entry in @(Get-ChildItem -LiteralPath $directory -Force)) {
            if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
                throw "BDS source world must not contain reparse points: $($entry.FullName)"
            }
            $relativePath = $entry.FullName.Substring($worldFull.Length).TrimStart([char[]]@('\', '/')).Replace('\', '/')
            if ([string]::IsNullOrWhiteSpace($relativePath) -or $entriesByPath.ContainsKey($relativePath)) {
                throw "invalid or duplicate BDS source world entry: $($entry.FullName)"
            }
            if ($entry.PSIsContainer) {
                $entriesByPath.Add($relativePath, [pscustomobject][ordered]@{
                    path = $relativePath
                    kind = 'directory'
                })
                $directories.Enqueue($entry.FullName)
            }
            else {
                $length = [uint64]$entry.Length
                $sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $entry.FullName).Hash.ToLowerInvariant()
                $entriesByPath.Add($relativePath, [pscustomobject][ordered]@{
                    path = $relativePath
                    kind = 'file'
                    length = $length
                    sha256 = $sha256
                })
                $fileCount = $fileCount + 1
                $totalBytes = $totalBytes + $length
            }
        }
    }
    $relativePaths = [string[]]@($entriesByPath.Keys)
    [Array]::Sort($relativePaths, [StringComparer]::Ordinal)
    $canonicalEntries = @($relativePaths | ForEach-Object { $entriesByPath[$_] })
    $levelDatPath = [IO.Path]::Combine($worldFull, 'level.dat')
    $levelDatInfo = Get-Item -LiteralPath $levelDatPath -Force -ErrorAction Stop
    if ($levelDatInfo.PSIsContainer -or
        (($levelDatInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "invalid BDS source level.dat: $levelDatPath"
    }
    $levelDatSha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $levelDatPath).Hash.ToLowerInvariant()
    $canonicalTree = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-bds-source-world-tree-v1'
        level_name = $levelName
        entries = $canonicalEntries
    }
    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-bds-source-world-identity-v1'
        level_name = $levelName
        world_path = $worldFull
        file_count = $fileCount
        total_bytes = $totalBytes
        level_dat_sha256 = $levelDatSha256
        sha256 = Get-CanonicalObjectHash -Value $canonicalTree
    }
}

function Assert-BdsSourceWorldIdentityUnchanged {
    param(
        [Parameter(Mandatory = $true)]$Expected,
        [Parameter(Mandatory = $true)][string]$SourceDirectory
    )

    $actual = Get-BdsSourceWorldIdentity -SourceDirectory $SourceDirectory
    foreach ($field in @('schema', 'level_name', 'file_count', 'total_bytes', 'level_dat_sha256', 'sha256')) {
        if ([string]$actual.$field -cne [string]$Expected.$field) {
            throw "BDS source world identity changed: field=$field expected=$($Expected.$field) actual=$($actual.$field)"
        }
    }
}

function New-ReservedUdpPort {
    $client = [Net.Sockets.UdpClient]::new(0)
    [pscustomobject]@{
        Client = $client
        Port = ([Net.IPEndPoint]$client.Client.LocalEndPoint).Port
    }
}

function Assert-BdsTickingAreaPreloadResult {
    param(
        [Parameter(Mandatory = $true)][string]$Line,
        [Parameter(Mandatory = $true)]$ExpectedMinimum,
        [Parameter(Mandatory = $true)]$ExpectedMaximum
    )

    $pattern = '^(?:NO LOG FILE! - )?\[[^\]\r\n]+ INFO\] Added ticking area from (-?\d+), (-?\d+), (-?\d+) to (-?\d+), (-?\d+), (-?\d+) marked for preload\.$'
    if ($Line -notmatch $pattern) {
        throw "invalid ticking-area preload acknowledgement: $Line"
    }
    $area = [pscustomobject][ordered]@{
        min_x = [int]$Matches[1]
        min_y = [int]$Matches[2]
        min_z = [int]$Matches[3]
        max_x = [int]$Matches[4]
        max_y = [int]$Matches[5]
        max_z = [int]$Matches[6]
        stdout = $Line
    }
    $expectedMinX = [int]([Math]::Floor([double][int]$ExpectedMinimum.x / 16.0) * 16.0)
    $expectedMinZ = [int]([Math]::Floor([double][int]$ExpectedMinimum.z / 16.0) * 16.0)
    $expectedMaxX = [int]([Math]::Floor([double][int]$ExpectedMaximum.x / 16.0) * 16.0 + 15.0)
    $expectedMaxZ = [int]([Math]::Floor([double][int]$ExpectedMaximum.z / 16.0) * 16.0 + 15.0)
    if ($area.min_x -ne $expectedMinX -or
        $area.min_z -ne $expectedMinZ -or
        $area.max_x -ne $expectedMaxX -or
        $area.max_z -ne $expectedMaxZ) {
        throw "ticking-area acknowledgement did not match exact chunk-snapped fixture bounds: expected=$expectedMinX,$expectedMinZ..$expectedMaxX,$expectedMaxZ requested=$($ExpectedMinimum.x),$($ExpectedMinimum.z)..$($ExpectedMaximum.x),$($ExpectedMaximum.z) acknowledged=$($area.min_x),$($area.min_z)..$($area.max_x),$($area.max_z)"
    }
    return $area
}

function Assert-BdsFixtureCommandResults {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Commands,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Lines
    )

    $results = [Collections.Generic.List[object]]::new()
    foreach ($line in $Lines) {
        if ($line -notmatch '^(?:NO LOG FILE! - )?\[[^\]\r\n]+ (?<level>INFO|ERROR)\] (?<message>.*)$') {
            if ($line.Contains(' ERROR] ')) {
                throw "BDS fixture command failed: $line"
            }
            continue
        }
        $level = [string]$Matches['level']
        $message = [string]$Matches['message']
        if ($level -ceq 'ERROR' -and $message -cne '0 blocks filled') {
            throw "BDS fixture command failed: $line"
        }
        if ($message -ceq 'Block placed') {
            $results.Add([pscustomobject][ordered]@{
                kind = 'setblock'
                changed_count = $null
                stdout = $line
            })
        }
        elseif ($message -match '^(?<count>\d+) blocks filled$') {
            $results.Add([pscustomobject][ordered]@{
                kind = 'fill'
                changed_count = [uint64]$Matches['count']
                stdout = $line
            })
        }
    }
    if ($results.Count -ne $Commands.Count) {
        throw "BDS fixture result count mismatch: expected=$($Commands.Count) actual=$($results.Count)"
    }
    for ($index = 0; $index -lt $Commands.Count; $index++) {
        $command = $Commands[$index]
        $result = $results[$index]
        $number = $index + 1
        if ($command -match '^setblock -?\d+ -?\d+ -?\d+ ') {
            if ([string]$result.kind -cne 'setblock') {
                throw "BDS fixture result did not match command ${number}: expected=setblock actual=$($result.kind) command=$command stdout=$($result.stdout)"
            }
            continue
        }
        if ($command -notmatch '^fill (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) (-?\d+) ') {
            throw "unsupported schema-v2 fixture command ${number}: $command"
        }
        if ([string]$result.kind -cne 'fill') {
            throw "BDS fixture result did not match command ${number}: expected=fill actual=$($result.kind) command=$command stdout=$($result.stdout)"
        }
        $volume = ([Math]::Abs([int64]$Matches[4] - [int64]$Matches[1]) + 1) *
            ([Math]::Abs([int64]$Matches[5] - [int64]$Matches[2]) + 1) *
            ([Math]::Abs([int64]$Matches[6] - [int64]$Matches[3]) + 1)
        if ([uint64]$result.changed_count -gt [uint64]$volume) {
            throw "BDS fill result exceeded declared command volume: command=$number volume=$volume changed=$($result.changed_count) stdout=$($result.stdout)"
        }
    }
    return [pscustomobject][ordered]@{
        result_count = $results.Count
        results = @($results)
        stdout_sha256 = Get-Utf8Sha256 -Text ($Lines -join "`n")
    }
}

function Assert-BdsCameraResortResult {
    param([Parameter(Mandatory = $true)]$Evidence)

    $skippedProperty = $Evidence.PSObject.Properties['SkippedLines']
    if ($null -eq $skippedProperty) {
        throw 'BDS camera resort command failed: post-resort fence did not retain intervening output'
    }
    $lines = @($skippedProperty.Value | ForEach-Object { [string]$_ })
    $rejected = @($lines | Where-Object { $_ -match '(?i)\bERROR\b' })
    if ($rejected.Count -ne 0) {
        throw "BDS camera resort command failed: $($rejected -join ' | ')"
    }
    $accepted = @($lines | Where-Object { $_ -match '(?i)\bTeleported\b' })
    if ($accepted.Count -ne 1) {
        throw "BDS camera resort command failed: expected one teleport acknowledgement before fence, found $($accepted.Count)"
    }
    return $accepted[0]
}

function Get-RequiredBdsMarkerEvidence {
    param(
        [Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)][string]$Context,
        [switch]$RequireSkippedLines
    )

    if ($null -eq $Evidence -or
        $null -eq $Evidence.PSObject.Properties['Line'] -or
        [string]::IsNullOrWhiteSpace([string]$Evidence.Line)) {
        throw "$Context did not return marker-line evidence"
    }
    if ($RequireSkippedLines -and $null -eq $Evidence.PSObject.Properties['SkippedLines']) {
        throw "$Context did not retain the exact stdout interval before its marker"
    }
    return $Evidence
}
