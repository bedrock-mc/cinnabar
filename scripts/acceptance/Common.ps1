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

function Get-Utf8Sha256 {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Text)

    $sha = [Security.Cryptography.SHA256]::Create()
    try {
        $bytes = [Text.UTF8Encoding]::new($false).GetBytes($Text)
        return -join ($sha.ComputeHash($bytes) | ForEach-Object { $_.ToString('x2') })
    }
    finally {
        $sha.Dispose()
    }
}

function Get-CanonicalObjectHash {
    param([Parameter(Mandatory = $true)]$Value)

    return Get-Utf8Sha256 -Text ($Value | ConvertTo-Json -Compress -Depth 16)
}

function Write-AtomicJsonArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Value,
        [ValidateRange(1, 32)][int]$Depth = 16
    )

    $parent = Split-Path -Parent $Path
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $temporaryPath = "$Path.partial-$PID-$([guid]::NewGuid().ToString('N'))"
    try {
        $json = $Value | ConvertTo-Json -Depth $Depth
        [IO.File]::WriteAllText($temporaryPath, $json, [Text.UTF8Encoding]::new($false))
        Move-Item -LiteralPath $temporaryPath -Destination $Path
    }
    finally {
        if (Test-Path -LiteralPath $temporaryPath) {
            Remove-Item -LiteralPath $temporaryPath -Force
        }
    }
    return (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
}

function Write-AcceptanceMetadataStatus {
    param(
        [Parameter(Mandatory = $true)][Collections.IDictionary]$Metadata,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [Parameter(Mandatory = $true)][ValidateSet('failed', 'passed')][string]$Status,
        [AllowNull()][string]$Failure,
        [ValidateRange(1, 32)][int]$Depth = 6
    )

    $Metadata['status'] = $Status
    if ($PSBoundParameters.ContainsKey('Failure')) {
        $Metadata['failure'] = $Failure
    }
    $Metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
    $Metadata | ConvertTo-Json -Depth $Depth | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
}

function Write-AcceptanceEvent {
    param(
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Event,
        [Collections.IDictionary]$Fields = [ordered]@{}
    )

    $path = Join-Path $RunDirectory 'acceptance-events.jsonl'
    $sequence = if (Test-Path -LiteralPath $path -PathType Leaf) {
        [IO.File]::ReadAllLines($path).Count + 1
    }
    else {
        1
    }
    $record = [ordered]@{
        sequence = $sequence
        event = $Event
        at_utc = [DateTime]::UtcNow.ToString('o')
    }
    foreach ($key in $Fields.Keys) {
        $record[$key] = $Fields[$key]
    }
    $line = ([pscustomobject]$record | ConvertTo-Json -Compress -Depth 12) + [Environment]::NewLine
    [IO.File]::AppendAllText($path, $line, [Text.UTF8Encoding]::new($false))
}

function Publish-FixtureManifest {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $manifestSha256 = Write-AtomicJsonArtifact -Path $Path -Value $Plan.Manifest
    $layoutProperty = $Plan.Manifest.PSObject.Properties['fixture_layout_hash']
    $layoutHash = if ($null -eq $layoutProperty) { $null } else { [string]$layoutProperty.Value }
    [Console]::Out.WriteLine("VISUAL_FIXTURE_READY=$Path")
    [Console]::Out.WriteLine("VISUAL_FIXTURE_SHA256=$manifestSha256")
    if (-not [string]::IsNullOrWhiteSpace($layoutHash)) {
        [Console]::Out.WriteLine("VISUAL_FIXTURE_LAYOUT_SHA256=$layoutHash")
    }
    return [pscustomobject][ordered]@{
        Path = $Path
        ManifestSha256 = $manifestSha256
        LayoutHash = $layoutHash
        Pose = [string]$Plan.Pose
    }
}

function Assert-PublishedTargetMutation {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$Expected
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "published target mutation manifest was missing: $Path"
    }
    $manifest = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    if ([string]$manifest.schema -cne 'rust-mcbe-visual-fixture-v2' -or
        $null -eq $manifest.PSObject.Properties['target_mutation'] -or
        $null -eq $manifest.target_mutation) {
        throw "published target mutation manifest was invalid: $Path"
    }
    $expectedCoordinate = @([int]$Expected.x, [int]$Expected.y, [int]$Expected.z)
    $actualCoordinate = @(
        [int]$manifest.target_mutation.x,
        [int]$manifest.target_mutation.y,
        [int]$manifest.target_mutation.z
    )
    if (($actualCoordinate -join ',') -cne ($expectedCoordinate -join ',')) {
        throw "published target mutation did not match plan: expected=$($expectedCoordinate -join ',') actual=$($actualCoordinate -join ',') path=$Path"
    }
}
