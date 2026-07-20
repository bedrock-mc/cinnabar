function ConvertFrom-TransparentSortCommittedMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    if ($Line -notmatch '^RUST_MCBE_TRANSPARENT_SORT_COMMITTED generation=(\d+) ref_count=(\d+)$') {
        throw "invalid transparent sort committed marker: $Line"
    }
    $generation = [uint64]0
    $refCount = [uint64]0
    if (-not [uint64]::TryParse($Matches[1], [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$generation) -or
        -not [uint64]::TryParse($Matches[2], [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$refCount) -or
        $generation -eq 0 -or $refCount -eq 0) {
        throw "invalid transparent sort committed marker: $Line"
    }
    return [pscustomobject][ordered]@{
        generation = $generation
        ref_count = $refCount
    }
}

function ConvertFrom-TransparentWitnessCompleteMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    if ($Line -notmatch '^RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE revision=(\d+) sequence=(\d+) generation=(\d+) key_count=(\d+) consecutive=(\d+)$') {
        throw "invalid transparent witness complete marker: $Line"
    }
    $values = [Collections.Generic.List[uint64]]::new()
    foreach ($text in $Matches[1..5]) {
        $value = [uint64]0
        if (-not [uint64]::TryParse($text, [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$value)) {
            throw "invalid transparent witness complete marker: $Line"
        }
        $values.Add($value)
    }
    if ($values[0] -eq 0 -or $values[1] -eq 0 -or $values[2] -eq 0 -or
        $values[3] -eq 0 -or $values[3] -gt 64 -or $values[4] -lt 1 -or $values[4] -gt 2) {
        throw "invalid transparent witness complete marker: $Line"
    }
    return [pscustomobject][ordered]@{
        revision = $values[0]
        sequence = $values[1]
        generation = $values[2]
        key_count = $values[3]
        consecutive = [int]$values[4]
    }
}

function Assert-StableTransparentWitnessEvidence {
    param(
        [Parameter(Mandatory = $true)]$Request,
        [Parameter(Mandatory = $true)]$First,
        [Parameter(Mandatory = $true)]$Second
    )

    $expectedRevision = [uint64]$Request.revision
    $expectedKeyCount = [uint64]@($Request.sub_chunks).Count
    if ([uint64]$First.revision -ne $expectedRevision -or [uint64]$Second.revision -ne $expectedRevision -or
        [uint64]$First.key_count -ne $expectedKeyCount -or [uint64]$Second.key_count -ne $expectedKeyCount -or
        [int]$First.consecutive -ne 1 -or [int]$Second.consecutive -ne 2 -or
        [uint64]$Second.sequence -le [uint64]$First.sequence -or
        [uint64]$Second.generation -lt [uint64]$First.generation) {
        throw "transparent witness did not complete twice consecutively: revision=$expectedRevision key_count=$expectedKeyCount first=$($First | ConvertTo-Json -Compress) second=$($Second | ConvertTo-Json -Compress)"
    }
    return $Second
}

function Stop-ProtocolMetadataProcessTree {
    param([Parameter(Mandatory = $true)][Diagnostics.Process]$Process)

    if (-not $Process.HasExited -and [Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        $treeKill = [Diagnostics.Process]::new()
        try {
            $treeKill.StartInfo.FileName = 'taskkill.exe'
            $treeKill.StartInfo.Arguments = "/PID $([int]$Process.Id) /T /F"
            $treeKill.StartInfo.UseShellExecute = $false
            $treeKill.StartInfo.CreateNoWindow = $true
            if (-not $treeKill.Start()) {
                throw 'could not start exact Cargo process-tree termination'
            }
            if (-not $treeKill.WaitForExit(10000)) {
                try { $treeKill.Kill() } catch { }
                throw 'exact Cargo process-tree termination timed out'
            }
            $treeKill.WaitForExit()
            if ($treeKill.ExitCode -ne 0 -and -not $Process.HasExited) {
                throw "exact Cargo process-tree termination failed with exit code $($treeKill.ExitCode)"
            }
        }
        catch {
            if (-not $Process.HasExited) {
                try { $Process.Kill() } catch {
                    throw "could not terminate timed-out Cargo process: $($_.Exception.Message)"
                }
            }
        }
        finally {
            $treeKill.Dispose()
        }
    }
    elseif (-not $Process.HasExited) {
        try { $Process.Kill() } catch {
            throw "could not terminate timed-out Cargo process: $($_.Exception.Message)"
        }
    }

    if (-not $Process.WaitForExit(10000)) {
        throw 'timed-out Cargo process did not exit within 10 seconds of termination'
    }
}

function Wait-ProtocolMetadataCopyTasks {
    param(
        [Parameter(Mandatory = $true)][Threading.Tasks.Task[]]$Tasks,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$TimeoutMilliseconds
    )

    if (-not [Threading.Tasks.Task]::WaitAll($Tasks, $TimeoutMilliseconds)) {
        throw "Cargo metadata output streams did not drain within $TimeoutMilliseconds milliseconds"
    }
}

function Assert-ProtocolDependencyProvenance {
    param(
        [Parameter(Mandatory = $true)][string]$ProjectRoot,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedForkRevision,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{40}$')][string]$ExpectedUpstreamRevision,
        [Parameter(Mandatory = $true)][ValidatePattern('^[0-9a-f]{64}$')][string]$ExpectedLicenseSha256
    )

    $manifestPath = Join-Path $ProjectRoot 'crates\protocol\Cargo.toml'
    $lockPath = Join-Path $ProjectRoot 'Cargo.lock'
    $upstreamPath = Join-Path $ProjectRoot 'crates\protocol\vendor\UPSTREAM.md'
    $licensePath = Join-Path $ProjectRoot 'crates\protocol\vendor\LICENSE'
    foreach ($requiredPath in @($manifestPath, $lockPath, $upstreamPath, $licensePath)) {
        if (-not (Test-Path -LiteralPath $requiredPath -PathType Leaf)) {
            throw "protocol dependency provenance input is missing: $requiredPath"
        }
    }

    $lock = Get-Content -Raw -LiteralPath $lockPath
    $upstream = Get-Content -Raw -LiteralPath $upstreamPath
    $metadataStdoutPath = [IO.Path]::GetTempFileName()
    $metadataStderrPath = [IO.Path]::GetTempFileName()
    $metadataProcess = $null
    $metadataStdoutStream = $null
    $metadataStderrStream = $null
    try {
        $metadataProcess = [Diagnostics.Process]::new()
        $metadataProcess.StartInfo.FileName = 'cargo'
        $metadataProcess.StartInfo.Arguments = 'metadata --locked --offline --no-deps --format-version 1 --manifest-path crates/protocol/Cargo.toml'
        $metadataProcess.StartInfo.WorkingDirectory = [IO.Path]::GetFullPath($ProjectRoot)
        $metadataProcess.StartInfo.UseShellExecute = $false
        $metadataProcess.StartInfo.CreateNoWindow = $true
        $metadataProcess.StartInfo.RedirectStandardOutput = $true
        $metadataProcess.StartInfo.RedirectStandardError = $true
        $metadataStdoutStream = [IO.File]::Open($metadataStdoutPath, [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $metadataStderrStream = [IO.File]::Open($metadataStderrPath, [IO.FileMode]::Create, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        if (-not $metadataProcess.Start()) {
            throw 'could not start cargo metadata for protocol provenance'
        }
        $metadataStdoutCopy = $metadataProcess.StandardOutput.BaseStream.CopyToAsync($metadataStdoutStream)
        $metadataStderrCopy = $metadataProcess.StandardError.BaseStream.CopyToAsync($metadataStderrStream)
        if (-not $metadataProcess.WaitForExit(120000)) {
            Stop-ProtocolMetadataProcessTree -Process $metadataProcess
            Wait-ProtocolMetadataCopyTasks `
                -Tasks @($metadataStdoutCopy, $metadataStderrCopy) -TimeoutMilliseconds 10000
            throw 'cargo metadata timed out after 120 seconds'
        }
        $metadataProcess.WaitForExit()
        [Threading.Tasks.Task]::WaitAll([Threading.Tasks.Task[]]@($metadataStdoutCopy, $metadataStderrCopy))
        $metadataStdoutStream.Flush()
        $metadataStderrStream.Flush()
        $metadataStdoutStream.Dispose()
        $metadataStdoutStream = $null
        $metadataStderrStream.Dispose()
        $metadataStderrStream = $null
        $metadataJson = Read-BoundedProtocolMetadataFile `
            -Path $metadataStdoutPath -MaximumBytes 4194304 -Label 'stdout'
        $metadataError = Read-BoundedProtocolMetadataFile `
            -Path $metadataStderrPath -MaximumBytes 262144 -Label 'stderr'
        if ($metadataProcess.ExitCode -ne 0) {
            throw "cargo metadata failed for protocol provenance: $($metadataError.Trim())"
        }
    }
    finally {
        if ($null -ne $metadataStdoutStream) { $metadataStdoutStream.Dispose() }
        if ($null -ne $metadataStderrStream) { $metadataStderrStream.Dispose() }
        if ($null -ne $metadataProcess) { $metadataProcess.Dispose() }
        Remove-Item -LiteralPath $metadataStdoutPath, $metadataStderrPath -Force -ErrorAction SilentlyContinue
    }
    try {
        $metadata = $metadataJson | ConvertFrom-Json
    }
    catch {
        throw "cargo metadata returned invalid JSON for protocol provenance: $($_.Exception.Message)"
    }
    $canonicalManifestPath = (Resolve-Path -LiteralPath $manifestPath).ProviderPath
    $pathComparison = if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
        [StringComparison]::OrdinalIgnoreCase
    }
    else {
        [StringComparison]::Ordinal
    }
    $protocolPackages = @($metadata.packages | Where-Object {
        $_.PSObject.Properties.Name -contains 'manifest_path' -and
        [string]::Equals(
            [IO.Path]::GetFullPath([string]$_.manifest_path),
            $canonicalManifestPath,
            $pathComparison
        )
    })
    if ($protocolPackages.Count -ne 1) {
        throw "cargo metadata must contain exactly one canonical protocol package, found $($protocolPackages.Count)"
    }
    $protocolPackage = $protocolPackages[0]
    $expectedDependencies = [ordered]@{
        valentine = @('bedrock_1_26_30')
        jolyne = @('client')
    }
    foreach ($dependencyName in $expectedDependencies.Keys) {
        $matches = @($protocolPackage.dependencies | Where-Object {
            ([string]$_.name -ceq $dependencyName) -or ([string]$_.rename -ceq $dependencyName)
        })
        if ($matches.Count -ne 1) {
            throw "protocol dependency provenance drifted: $dependencyName must resolve exactly once from the canonical protocol manifest"
        }
        $dependency = $matches[0]
        foreach ($field in @('name', 'source', 'kind', 'rename', 'optional', 'uses_default_features', 'features', 'target', 'path')) {
            if ($dependency.PSObject.Properties.Name -cnotcontains $field) {
                throw "cargo metadata dependency $dependencyName is missing $field"
            }
        }
        if ([string]$dependency.name -cne $dependencyName -or $null -ne $dependency.rename) {
            throw "protocol dependency provenance drifted: $dependencyName must not be renamed"
        }
        if ($null -ne $dependency.source -or $null -ne $dependency.kind -or $null -ne $dependency.target -or
            $dependency.optional -isnot [bool] -or [bool]$dependency.optional -or
            $dependency.uses_default_features -isnot [bool] -or [bool]$dependency.uses_default_features) {
            throw "protocol dependency provenance drifted: $dependencyName must be one normal non-target non-optional local dependency with default features disabled"
        }
        $vendoredManifest = Join-Path $ProjectRoot "crates\protocol\vendor\$dependencyName\Cargo.toml"
        if (-not (Test-Path -LiteralPath $vendoredManifest -PathType Leaf)) {
            throw "protocol dependency provenance drifted: $dependencyName vendored path has no Cargo.toml"
        }
        $expectedPath = (Resolve-Path -LiteralPath (Split-Path -Parent $vendoredManifest)).ProviderPath
        if ($null -eq $dependency.path -or -not [string]::Equals(
            [IO.Path]::GetFullPath([string]$dependency.path),
            $expectedPath,
            $pathComparison
        )) {
            throw "protocol dependency provenance drifted: $dependencyName does not resolve to its canonical vendored path"
        }
        $expectedFeatures = @($expectedDependencies[$dependencyName])
        $actualFeatures = @($dependency.features)
        if ($actualFeatures.Count -ne $expectedFeatures.Count -or
            @($actualFeatures | Where-Object { $expectedFeatures -cnotcontains [string]$_ }).Count -ne 0) {
            throw "protocol dependency provenance drifted: $dependencyName resolved feature set is not exact"
        }
    }

    $forkLine = "- Reviewed fork revision: ``$ExpectedForkRevision``"
    if ([regex]::Matches($upstream, '(?m)^' + [regex]::Escape($forkLine) + '\r?$').Count -ne 1) {
        throw "protocol dependency provenance drifted: vendored fork revision is not $ExpectedForkRevision"
    }
    $upstreamLine = "- Upstream snapshot revision: ``$ExpectedUpstreamRevision``"
    if ([regex]::Matches($upstream, '(?m)^' + [regex]::Escape($upstreamLine) + '\r?$').Count -ne 1) {
        throw "protocol dependency provenance drifted: upstream revision is not $ExpectedUpstreamRevision"
    }
    $licenseLine = "- Retained license: MIT at ``crates/protocol/vendor/LICENSE`` (normalized SHA-256 ``$ExpectedLicenseSha256``)"
    if ([regex]::Matches($upstream, '(?m)^' + [regex]::Escape($licenseLine) + '\r?$').Count -ne 1) {
        throw 'protocol dependency provenance drifted: retained license metadata is missing or ambiguous'
    }

    $licenseText = [IO.File]::ReadAllText($licensePath).Replace("`r`n", "`n").Replace("`r", "`n")
    $licenseBytes = [Text.UTF8Encoding]::new($false).GetBytes($licenseText)
    $sha256 = [Security.Cryptography.SHA256]::Create()
    try {
        $licenseSha256 = ([BitConverter]::ToString($sha256.ComputeHash($licenseBytes))).Replace('-', '').ToLowerInvariant()
    }
    finally {
        $sha256.Dispose()
    }
    if ($licenseSha256 -cne $ExpectedLicenseSha256) {
        throw "protocol dependency retained license SHA-256 drifted: expected $ExpectedLicenseSha256, got $licenseSha256"
    }

    $resolvedLocalPackages = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    $packageBlocks = [regex]::Matches($lock, '(?ms)^\[\[package\]\]\r?\n(?<body>.*?)(?=^\[\[package\]\]|\z)')
    foreach ($packageBlock in $packageBlocks) {
        $body = $packageBlock.Groups['body'].Value
        $nameMatch = [regex]::Match($body, '(?m)^name\s*=\s*"(?<name>[^"]+)"\r?$')
        if (-not $nameMatch.Success) { continue }
        $name = $nameMatch.Groups['name'].Value
        if ($name -eq 'jolyne' -or $name.StartsWith('valentine', [StringComparison]::Ordinal)) {
            $null = $resolvedLocalPackages.Add($name)
            $resolutionKey = [regex]::Match($body, '(?m)^\s*(?<key>source|checksum)\s*=')
            if ($resolutionKey.Success) {
                throw "Cargo.lock local package $name has a $($resolutionKey.Groups['key'].Value) entry"
            }
        }
    }
    foreach ($dependency in @('valentine', 'jolyne')) {
        if (-not $resolvedLocalPackages.Contains($dependency)) {
            throw "Cargo.lock does not contain local package $dependency"
        }
    }
    return $ExpectedForkRevision
}

function Read-BoundedProtocolMetadataFile {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][ValidateRange(1, [long]::MaxValue)][long]$MaximumBytes,
        [Parameter(Mandatory = $true)][string]$Label
    )
    $file = Get-Item -LiteralPath $Path
    if ($file.Length -gt $MaximumBytes) {
        throw "cargo metadata $Label exceeds the $MaximumBytes-byte provenance bound"
    }
    return [IO.File]::ReadAllText($file.FullName)
}

function Get-ProtocolDependencyProvenanceMetadata {
    return [ordered]@{
        protocol_dependency_resolution = 'vendored-path'
        pinned_valentine_fork_commit = $PinnedValentineForkCommit
        pinned_valentine_upstream_commit = $PinnedValentineUpstreamCommit
        pinned_valentine_license_sha256 = $PinnedValentineLicenseSha256
    }
}

function ConvertFrom-GalleryAnchorReadyMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    $pattern = '^RUST_MCBE_GALLERY_ANCHOR_READY coordinate=(-?\d+),(-?\d+),(-?\d+) rendered=true visible=(true|false) clean=true$'
    if ($Line -notmatch $pattern) {
        throw "invalid gallery anchor ready marker: $Line"
    }
    $coordinate = [Collections.Generic.List[int]]::new()
    foreach ($index in 1..3) {
        $value = [int]0
        if (-not [int]::TryParse($Matches[$index], [Globalization.NumberStyles]::Integer, [Globalization.CultureInfo]::InvariantCulture, [ref]$value)) {
            throw "invalid gallery anchor ready marker: $Line"
        }
        $coordinate.Add($value)
    }
    return [pscustomobject][ordered]@{
        coordinate = @($coordinate)
        visible = [string]$Matches[4] -ceq 'true'
    }
}

function ConvertFrom-CameraCommittedMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    $number = '[-+]?(?:\d+(?:\.\d*)?|\.\d+)(?:[eE][-+]?\d+)?'
    $pattern = "^RUST_MCBE_CAMERA_COMMITTED sequence=(\d+) position=($number),($number),($number) yaw=($number) pitch=($number)$"
    if ($Line -notmatch $pattern) {
        throw "invalid camera committed marker: $Line"
    }
    $sequence = [uint64]0
    if (-not [uint64]::TryParse($Matches[1], [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$sequence) -or $sequence -eq 0) {
        throw "invalid camera committed marker: $Line"
    }
    $values = [Collections.Generic.List[double]]::new()
    foreach ($index in 2..6) {
        $value = [double]0
        if (-not [double]::TryParse($Matches[$index], [Globalization.NumberStyles]::Float, [Globalization.CultureInfo]::InvariantCulture, [ref]$value) -or
            [double]::IsNaN($value) -or [double]::IsInfinity($value)) {
            throw "invalid camera committed marker: $Line"
        }
        $values.Add($value)
    }
    return [pscustomobject][ordered]@{
        sequence = $sequence
        position = @($values[0], $values[1], $values[2])
        yaw = $values[3]
        pitch = $values[4]
    }
}

function Assert-ModelGalleryCommittedCamera {
    param(
        [Parameter(Mandatory = $true)]$Committed,
        [Parameter(Mandatory = $true)]$Target
    )

    # Bedrock's tp command centers integral horizontal block coordinates.
    $expectedX = [double]$Target.x + 0.5
    $expectedY = [double]$Target.y + 1.62001
    $expectedZ = [double]$Target.z + 0.5
    if ([Math]::Abs([double]$Committed.position[0] - $expectedX) -gt 0.01 -or
        [Math]::Abs([double]$Committed.position[1] - $expectedY) -gt 0.01 -or
        [Math]::Abs([double]$Committed.position[2] - $expectedZ) -gt 0.01) {
        throw "committed client camera did not match the model gallery target: expected=$expectedX,$expectedY,$expectedZ actual=$(@($Committed.position) -join ',')"
    }
    return [pscustomobject][ordered]@{
        x = $expectedX
        y = $expectedY
        z = $expectedZ
    }
}

function ConvertFrom-ModelWitnessCompleteMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    $pattern = '^RUST_MCBE_MODEL_WITNESS_COMPLETE revision=(\d+) request_sha256=([0-9a-f]{64}) sequence=(\d+) view_generation=(\d+) key_count=(\d+) model_ref_count=(\d+) manifest_count=(\d+) manifest_sha256=([0-9a-f]{64}) missing=(\d+) stale=(\d+) wrong_stream=(\d+) zero_ref=(\d+) draw_mismatch=(\d+) consecutive=(\d+)$'
    if ($Line -notmatch $pattern) {
        throw "invalid model witness complete marker: $Line"
    }
    $numbers = [Collections.Generic.List[uint64]]::new()
    foreach ($index in @(1, 3, 4, 5, 6, 7, 9, 10, 11, 12, 13, 14)) {
        $value = [uint64]0
        if (-not [uint64]::TryParse($Matches[$index], [Globalization.NumberStyles]::None, [Globalization.CultureInfo]::InvariantCulture, [ref]$value)) {
            throw "invalid model witness complete marker: $Line"
        }
        $numbers.Add($value)
    }
    if ($numbers[0] -eq 0 -or $numbers[1] -eq 0 -or $numbers[2] -eq 0 -or
        $numbers[3] -eq 0 -or $numbers[3] -gt 64 -or $numbers[4] -eq 0 -or
        $numbers[5] -ne $numbers[3] -or
        @($numbers[6..10] | Where-Object { $_ -ne 0 }).Count -ne 0 -or
        $numbers[11] -lt 1 -or $numbers[11] -gt 2) {
        throw "invalid model witness complete marker: $Line"
    }
    return [pscustomobject][ordered]@{
        revision = $numbers[0]
        request_sha256 = $Matches[2]
        sequence = $numbers[1]
        view_generation = $numbers[2]
        key_count = $numbers[3]
        model_ref_count = $numbers[4]
        manifest_count = $numbers[5]
        manifest_sha256 = $Matches[8]
        missing = $numbers[6]
        stale = $numbers[7]
        wrong_stream = $numbers[8]
        zero_ref = $numbers[9]
        draw_mismatch = $numbers[10]
        consecutive = [int]$numbers[11]
    }
}

function ConvertFrom-ActorPoseWitnessMarker {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line)

    $prefix = 'RUST_MCBE_ACTOR_POSE_WITNESS='
    if (-not $Line.StartsWith($prefix, [StringComparison]::Ordinal) -or $Line.Length -gt 16384) {
        throw "invalid actor pose witness marker"
    }
    try {
        $payload = $Line.Substring($prefix.Length) | ConvertFrom-Json -ErrorAction Stop
    }
    catch {
        throw "invalid actor pose witness marker JSON: $($_.Exception.Message)"
    }
    $required = @('session_id', 'dimension', 'sequence', 'runtime_id', 'packet', 'store', 'presented', 'drops')
    if (@($required | Where-Object { -not ($payload.PSObject.Properties.Name -contains $_) }).Count -ne 0 -or
        [uint64]$payload.session_id -eq 0 -or [uint64]$payload.sequence -eq 0 -or
        [uint64]$payload.runtime_id -eq 0 -or [uint64]$payload.store.spawn_revision -eq 0 -or
        [uint64]$payload.store.movement_revision -ne [uint64]$payload.sequence -or
        -not [bool]$payload.presented.consecutive -or
        [uint64]$payload.presented.first_frame_sequence + 1 -ne [uint64]$payload.presented.second_frame_sequence) {
        throw "invalid actor pose witness marker contract"
    }
    return $payload
}

function Assert-StableModelWitnessEvidence {
    param(
        [Parameter(Mandatory = $true)]$Request,
        [Parameter(Mandatory = $true)]$First,
        [Parameter(Mandatory = $true)]$Second
    )

    $expectedRevision = [uint64]$Request.revision
    $expectedHash = [string]$Request.request_sha256
    $expectedKeyCount = [uint64]@($Request.sub_chunks).Count
    $zeroCounters = @('missing', 'stale', 'wrong_stream', 'zero_ref', 'draw_mismatch')
    $hasMismatch = @($zeroCounters | Where-Object {
        [uint64]$First.$_ -ne 0 -or [uint64]$Second.$_ -ne 0
    }).Count -ne 0
    if ([uint64]$First.revision -ne $expectedRevision -or [uint64]$Second.revision -ne $expectedRevision -or
        [string]$First.request_sha256 -cne $expectedHash -or [string]$Second.request_sha256 -cne $expectedHash -or
        [uint64]$First.key_count -ne $expectedKeyCount -or [uint64]$Second.key_count -ne $expectedKeyCount -or
        [uint64]$First.manifest_count -ne $expectedKeyCount -or [uint64]$Second.manifest_count -ne $expectedKeyCount -or
        [uint64]$First.model_ref_count -eq 0 -or [uint64]$First.model_ref_count -ne [uint64]$Second.model_ref_count -or
        [string]$First.manifest_sha256 -cne [string]$Second.manifest_sha256 -or
        [uint64]$First.view_generation -eq 0 -or [uint64]$First.view_generation -ne [uint64]$Second.view_generation -or
        [int]$First.consecutive -ne 1 -or [int]$Second.consecutive -ne 2 -or
        [uint64]$First.sequence + 1 -ne [uint64]$Second.sequence -or $hasMismatch) {
        throw "model witness did not form an adjacent stable exact pair: revision=$expectedRevision key_count=$expectedKeyCount first=$($First | ConvertTo-Json -Compress) second=$($Second | ConvertTo-Json -Compress)"
    }
    return $Second
}

function Assert-NewerTransparentSortCommit {
    param(
        [Parameter(Mandatory = $true)]$Initial,
        [Parameter(Mandatory = $true)][uint64]$InitialLineNumber,
        [Parameter(Mandatory = $true)]$Resort,
        [Parameter(Mandatory = $true)][uint64]$ResortLineNumber
    )

    if ([uint64]$Resort.generation -le [uint64]$Initial.generation) {
        throw "camera resort did not commit a newer transparent sort: initial=$($Initial.generation) resort=$($Resort.generation)"
    }
    if ($ResortLineNumber -le $InitialLineNumber) {
        throw "camera resort transparent sort marker was not later in stdout: initial=$InitialLineNumber resort=$ResortLineNumber"
    }
    return $Resort
}

function ConvertFrom-AcceptanceRuntimeMetadataMarker {
    param([Parameter(Mandatory = $true)][string]$Line)

    $prefix = 'RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA='
    if (-not $Line.StartsWith($prefix, [StringComparison]::Ordinal)) {
        throw 'invalid acceptance runtime metadata marker prefix'
    }
    try {
        $document = $Line.Substring($prefix.Length) | ConvertFrom-Json
    }
    catch {
        throw "invalid acceptance runtime metadata JSON: $($_.Exception.Message)"
    }
    foreach ($field in @(
        'build_profile', 'requested_present_mode', 'effective_present_mode',
        'present_mode_proven', 'backend', 'adapter', 'driver', 'driver_info'
    )) {
        if ($null -eq $document.PSObject.Properties[$field] -or
            [string]::IsNullOrWhiteSpace([string]$document.$field)) {
            throw "acceptance runtime metadata is missing $field"
        }
    }
    if ($document.present_mode_proven -ne $true) {
        throw 'acceptance runtime metadata does not prove the configured present mode'
    }
    return $document
}

function Read-AcceptanceRuntimeMetadata {
    param([Parameter(Mandatory = $true)][string]$Path)

    $lines = @(
        Get-Content -LiteralPath $Path |
            Where-Object { $_.StartsWith('RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA=', [StringComparison]::Ordinal) }
    )
    if ($lines.Count -ne 1) {
        throw "expected exactly one acceptance runtime metadata marker, found $($lines.Count)"
    }
    return ConvertFrom-AcceptanceRuntimeMetadataMarker -Line $lines[0]
}

function ConvertFrom-WorldPublicationSnapshotMarker {
    param(
        [Parameter(Mandatory = $true)][string]$Line,
        [Parameter(Mandatory = $true)][ValidateSet('debug', 'release')][string]$ExpectedBuildProfile,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode
    )

    $prefix = 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT='
    if (-not $Line.StartsWith($prefix, [StringComparison]::Ordinal)) {
        throw 'invalid world publication snapshot marker prefix'
    }
    $json = $Line.Substring($prefix.Length)
    try {
        $document = $json | ConvertFrom-Json
    }
    catch {
        throw "invalid world publication snapshot JSON: $($_.Exception.Message)"
    }
    if ($null -eq $document -or $document -isnot [psobject]) {
        throw 'world publication snapshot JSON must be an object'
    }

    $seenFields = [Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal)
    foreach ($match in [regex]::Matches($json, '(?<!\\)"((?:\\.|[^"\\])*)"\s*:')) {
        try {
            $field = ('"' + $match.Groups[1].Value + '"') | ConvertFrom-Json
        }
        catch {
            throw 'world publication snapshot contains an invalid field name'
        }
        if (-not $seenFields.Add([string]$field)) {
            throw "world publication snapshot contains duplicate field $field"
        }
    }

    $integerFields = @(
        'accepted_light_jobs', 'noop_light_jobs', 'value_changed_light_jobs',
        'provenance_only_light_jobs', 'light_mesh_invalidations', 'stale_light_jobs',
        'stale_mesh_jobs', 'queued_decode_jobs', 'in_flight_decode_jobs',
        'pending_light_jobs', 'in_flight_light_jobs', 'pending_mesh_jobs',
        'in_flight_mesh_jobs', 'upload_queue_items', 'upload_queue_bytes',
        'gpu_upload_bytes', 'frame_generation', 'pose_generation', 'view_generation'
    )
    $durationFields = @(
        'max_decode_queue_wait_ms', 'max_light_queue_wait_ms', 'max_mesh_queue_wait_ms',
        'max_decode_worker_ms', 'max_light_worker_ms', 'max_mesh_worker_ms'
    )
    $identityFields = @(
        'draw_mode', 'build_profile', 'requested_present_mode', 'effective_present_mode',
        'present_mode_proven', 'backend', 'adapter', 'driver', 'driver_info'
    )
    $required = @($integerFields + $durationFields + $identityFields)
    $actual = @($document.PSObject.Properties.Name)
    $missing = @($required | Where-Object { -not ($actual -ccontains $_) } | Sort-Object)
    $extra = @($actual | Where-Object { -not ($required -ccontains $_) } | Sort-Object)
    if ($missing.Count -ne 0 -or $extra.Count -ne 0) {
        $missingText = if ($missing.Count -eq 0) { '<none>' } else { $missing -join ',' }
        $extraText = if ($extra.Count -eq 0) { '<none>' } else { $extra -join ',' }
        throw "world publication snapshot schema mismatch: missing=$missingText extra=$extraText"
    }
    $jsonNumber = '-?(?:0|[1-9][0-9]*)(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?'
    $jsonScalar = '"(?:\\.|[^"\\])*"|true|false|null|' + $jsonNumber
    foreach ($field in $integerFields) {
        $fieldPattern = '(?<!\\)"' + [regex]::Escape($field) + '"\s*:\s*(?<value>' + $jsonScalar + ')(?=\s*[,}])'
        $valueMatch = [regex]::Match($json, $fieldPattern)
        if (-not $valueMatch.Success -or $valueMatch.Groups['value'].Value -notmatch '^(?:0|[1-9][0-9]*)$') {
            throw "world publication snapshot field $field must be a JSON nonnegative integer"
        }
        try {
            $value = [decimal]$document.$field
        }
        catch {
            throw "world publication snapshot field $field must be a nonnegative integer"
        }
        if ($value -lt 0 -or [decimal]::Truncate($value) -ne $value -or $value -gt [decimal][uint64]::MaxValue) {
            throw "world publication snapshot field $field must be a nonnegative uint64"
        }
    }
    foreach ($field in $durationFields) {
        $fieldPattern = '(?<!\\)"' + [regex]::Escape($field) + '"\s*:\s*(?<value>' + $jsonScalar + ')(?=\s*[,}])'
        $valueMatch = [regex]::Match($json, $fieldPattern)
        if (-not $valueMatch.Success -or $valueMatch.Groups['value'].Value -notmatch ('^(?:' + $jsonNumber + ')$')) {
            throw "world publication snapshot field $field must be a JSON number"
        }
        $value = [double]$document.$field
        if ([double]::IsNaN($value) -or [double]::IsInfinity($value) -or $value -lt 0) {
            throw "world publication snapshot field $field must be finite and nonnegative"
        }
    }
    if (@('Direct', 'MultiDrawIndirect') -cnotcontains [string]$document.draw_mode) {
        throw "world publication snapshot has invalid draw_mode $($document.draw_mode)"
    }
    foreach ($field in @('draw_mode', 'build_profile', 'requested_present_mode', 'effective_present_mode', 'backend', 'adapter', 'driver', 'driver_info')) {
        if ($document.$field -isnot [string]) {
            throw "world publication snapshot field $field must be a JSON string"
        }
    }
    if ([string]$document.build_profile -cne $ExpectedBuildProfile) {
        throw "world publication snapshot build profile mismatch: expected=$ExpectedBuildProfile observed=$($document.build_profile)"
    }
    if ([string]$document.requested_present_mode -cne $ExpectedPresentMode -or
        [string]$document.effective_present_mode -cne $ExpectedPresentMode) {
        throw "world publication snapshot present mode mismatch: expected=$ExpectedPresentMode requested=$($document.requested_present_mode) effective=$($document.effective_present_mode)"
    }
    $proofPattern = '(?<!\\)"present_mode_proven"\s*:\s*(?<value>' + $jsonScalar + ')(?=\s*[,}])'
    $proofMatch = [regex]::Match($json, $proofPattern)
    if (-not $proofMatch.Success -or $proofMatch.Groups['value'].Value -cne 'true' -or $document.present_mode_proven -isnot [bool]) {
        throw 'world publication snapshot does not prove the configured present mode'
    }
    foreach ($field in @('backend', 'adapter', 'driver', 'driver_info')) {
        if ([string]::IsNullOrWhiteSpace([string]$document.$field)) {
            throw "world publication snapshot is missing $field"
        }
    }
    return $document
}

function Read-WorldPublicationSnapshots {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][ValidateSet('debug', 'release')][string]$ExpectedBuildProfile,
        [Parameter(Mandatory = $true)][ValidateSet('Fifo', 'Immediate')][string]$ExpectedPresentMode
    )

    $lines = @(
        Get-Content -LiteralPath $Path |
            Where-Object { $_.StartsWith('RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=', [StringComparison]::Ordinal) }
    )
    if ($lines.Count -eq 0) {
        throw 'expected at least one world publication snapshot marker, found none'
    }
    $snapshots = @($lines | ForEach-Object {
        ConvertFrom-WorldPublicationSnapshotMarker `
            -Line $_ `
            -ExpectedBuildProfile $ExpectedBuildProfile `
            -ExpectedPresentMode $ExpectedPresentMode
    })
    $first = $snapshots[0]
    foreach ($snapshot in $snapshots | Select-Object -Skip 1) {
        if ([string]$snapshot.draw_mode -cne [string]$first.draw_mode) {
            throw "world publication snapshot draw mode changed within one run: first=$($first.draw_mode) observed=$($snapshot.draw_mode)"
        }
        foreach ($field in @('build_profile', 'requested_present_mode', 'effective_present_mode', 'backend', 'adapter', 'driver', 'driver_info')) {
            if ([string]$snapshot.$field -cne [string]$first.$field) {
                throw "world publication snapshot identity field $field changed within one run"
            }
        }
    }
    return $snapshots[-1]
}
