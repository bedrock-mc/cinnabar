$expectedForkRevision = '6cd8087fc3f0b500e41708a8afc94a0fa3291525'
$expectedUpstreamRevision = '6f6806e821a579c183c44d786f76d9b358a2b825'
$expectedLicenseSha256 = '62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184'

. (Join-Path $ProjectRoot 'scripts\acceptance\Markers.ps1')

$PinnedValentineForkCommit = $expectedForkRevision
$PinnedValentineUpstreamCommit = $expectedUpstreamRevision
$PinnedValentineLicenseSha256 = $expectedLicenseSha256
$protocolMetadata = Get-ProtocolDependencyProvenanceMetadata
Assert-Equal 4 $protocolMetadata.Count 'protocol provenance metadata added or omitted a field'
Assert-Equal 'vendored-path' $protocolMetadata.protocol_dependency_resolution 'protocol dependency resolution metadata drifted'
Assert-Equal $expectedForkRevision $protocolMetadata.pinned_valentine_fork_commit 'reviewed fork metadata drifted'
Assert-Equal $expectedUpstreamRevision $protocolMetadata.pinned_valentine_upstream_commit 'upstream snapshot metadata drifted'
Assert-Equal $expectedLicenseSha256 $protocolMetadata.pinned_valentine_license_sha256 'retained license metadata drifted'

function Copy-ProtocolDependencyProvenanceFixture {
    param(
        [Parameter(Mandatory = $true)][string]$SourceRoot,
        [Parameter(Mandatory = $true)][string]$DestinationRoot
    )

    New-Item -ItemType Directory -Path $DestinationRoot -Force | Out-Null
    Copy-Item -LiteralPath (Join-Path $SourceRoot 'Cargo.toml') -Destination $DestinationRoot
    Copy-Item -LiteralPath (Join-Path $SourceRoot 'Cargo.lock') -Destination $DestinationRoot
    foreach ($workspaceDirectory in @('app', 'crates', 'tools')) {
        Copy-Item -LiteralPath (Join-Path $SourceRoot $workspaceDirectory) `
            -Destination $DestinationRoot -Recurse
    }
}

function Assert-TestProtocolDependencyProvenance {
    param([Parameter(Mandatory = $true)][string]$Root)

    Assert-ProtocolDependencyProvenance `
        -ProjectRoot $Root `
        -ExpectedForkRevision $expectedForkRevision `
        -ExpectedUpstreamRevision $expectedUpstreamRevision `
        -ExpectedLicenseSha256 $expectedLicenseSha256
}

$null = Assert-TestProtocolDependencyProvenance -Root $ProjectRoot

New-Item -ItemType Directory -Path $TempRoot -Force | Out-Null
$oversizedMetadata = Join-Path $TempRoot 'oversized cargo metadata.json'
[IO.File]::WriteAllBytes($oversizedMetadata, [byte[]](0..32))
Assert-ThrowsLike {
    Read-BoundedProtocolMetadataFile -Path $oversizedMetadata -MaximumBytes 32 -Label 'test output'
} '*exceeds*32-byte*bound*' 'protocol provenance accepted oversized Cargo metadata output'

$fixtureRoot = Join-Path $TempRoot 'protocol dependency provenance'
Copy-ProtocolDependencyProvenanceFixture -SourceRoot $ProjectRoot -DestinationRoot $fixtureRoot
$null = Assert-TestProtocolDependencyProvenance -Root $fixtureRoot

$manifestPath = Join-Path $fixtureRoot 'crates\protocol\Cargo.toml'
$canonicalManifest = Get-Content -Raw -LiteralPath $manifestPath
Set-Content -LiteralPath $manifestPath -NoNewline -Value `
    $canonicalManifest.Replace('path = "vendor/valentine"', 'path = "..\outside\valentine"')
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*cargo metadata*' 'protocol provenance accepted a drifted Valentine path declaration'
Set-Content -LiteralPath $manifestPath -NoNewline -Value $canonicalManifest

$vendorRoot = Join-Path $fixtureRoot 'crates\protocol\vendor'
Copy-Item -LiteralPath (Join-Path $vendorRoot 'valentine') `
    -Destination (Join-Path $vendorRoot 'valentine-decoy') -Recurse
Copy-Item -LiteralPath (Join-Path $vendorRoot 'jolyne') `
    -Destination (Join-Path $vendorRoot 'jolyne-decoy') -Recurse
$jolyneDecoyManifest = Join-Path $vendorRoot 'jolyne-decoy\Cargo.toml'
$jolyneDecoy = (Get-Content -Raw -LiteralPath $jolyneDecoyManifest).Replace(
    'path = "../valentine"',
    'path = "../valentine-decoy"'
)
Set-Content -LiteralPath $jolyneDecoyManifest -NoNewline -Value $jolyneDecoy
$canonicalStringDecoys = @'
[dependencies]
valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }
jolyne = { path = "vendor/jolyne", default-features = false, features = ["client"] }
'@
$quotedWrongPaths = $canonicalManifest.Replace(
    'publish = false',
    "publish = false`ndescription = `"`"`"`n$canonicalStringDecoys`n`"`"`""
).Replace(
    'valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }',
    '"valentine" = { path = "vendor/valentine-decoy", default-features = false, features = ["bedrock_1_26_30"] }'
).Replace(
    'jolyne = { path = "vendor/jolyne", default-features = false, features = ["client"] }',
    '"jolyne" = { path = "vendor/jolyne-decoy", default-features = false, features = ["client"] }'
)
Set-Content -LiteralPath $manifestPath -NoNewline -Value $quotedWrongPaths
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*vendored path*' 'protocol provenance accepted canonical declarations inside a multiline string while quoted real keys resolved wrong paths'
Set-Content -LiteralPath $manifestPath -NoNewline -Value $canonicalManifest

Set-Content -LiteralPath $manifestPath -NoNewline -Value ($canonicalManifest + @'

[target.'cfg(unix)'.dependencies]
valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }
'@)
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*valentine*exactly once*' 'protocol provenance accepted an additional target-table Valentine declaration'

$inactiveDecoy = $canonicalManifest.Replace(
    'valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }',
    '# active Valentine declaration removed'
) + @'

[target.'cfg(unix)'.dependencies]
valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }
'@
Set-Content -LiteralPath $manifestPath -NoNewline -Value $inactiveDecoy
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*valentine*normal non-target*' 'protocol provenance accepted an inactive target-table Valentine decoy'
Set-Content -LiteralPath $manifestPath -NoNewline -Value $canonicalManifest

$upstreamPath = Join-Path $fixtureRoot 'crates\protocol\vendor\UPSTREAM.md'
$canonicalUpstream = Get-Content -Raw -LiteralPath $upstreamPath
Set-Content -LiteralPath $upstreamPath -NoNewline -Value `
    $canonicalUpstream.Replace($expectedForkRevision, ('0' * 40))
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*fork revision*' 'protocol provenance accepted drifted vendored fork metadata'
Set-Content -LiteralPath $upstreamPath -NoNewline -Value `
    $canonicalUpstream.Replace($expectedUpstreamRevision, ('1' * 40))
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*upstream revision*' 'protocol provenance accepted drifted upstream merge metadata'
Set-Content -LiteralPath $upstreamPath -NoNewline -Value $canonicalUpstream

$licensePath = Join-Path $fixtureRoot 'crates\protocol\vendor\LICENSE'
$canonicalLicense = Get-Content -Raw -LiteralPath $licensePath
Set-Content -LiteralPath $licensePath -NoNewline -Value ($canonicalLicense + 'drift')
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*license*SHA-256*' 'protocol provenance accepted a drifted retained license'
Set-Content -LiteralPath $licensePath -NoNewline -Value $canonicalLicense

$lockPath = Join-Path $fixtureRoot 'Cargo.lock'
$canonicalLock = Get-Content -Raw -LiteralPath $lockPath
$driftedLock = $canonicalLock.Replace(
    "name = `"valentine`"`r`nversion = `"0.1.0`"",
    "name = `"valentine`"`r`nversion = `"0.1.0`"`r`n   source   = `"git+https://github.com/HashimTheArab/axolotl-stack.git?rev=$expectedForkRevision#$expectedForkRevision`""
)
if ($driftedLock -ceq $canonicalLock) {
    $driftedLock = $canonicalLock.Replace(
        "name = `"valentine`"`nversion = `"0.1.0`"",
        "name = `"valentine`"`nversion = `"0.1.0`"`n   source   = `"git+https://github.com/HashimTheArab/axolotl-stack.git?rev=$expectedForkRevision#$expectedForkRevision`""
    )
}
Assert-True ($driftedLock -cne $canonicalLock) 'lock drift fixture did not mutate Valentine resolution'
Set-Content -LiteralPath $lockPath -NoNewline -Value $driftedLock
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*Cargo.lock*local package*source*' 'protocol provenance accepted a Git source for a local package'
Set-Content -LiteralPath $lockPath -NoNewline -Value $canonicalLock

$driftedLock = $canonicalLock.Replace(
    "name = `"jolyne`"`r`nversion = `"0.1.0`"",
    "name = `"jolyne`"`r`nversion = `"0.1.0`"`r`n`tchecksum = `"$('2' * 64)`""
)
if ($driftedLock -ceq $canonicalLock) {
    $driftedLock = $canonicalLock.Replace(
        "name = `"jolyne`"`nversion = `"0.1.0`"",
        "name = `"jolyne`"`nversion = `"0.1.0`"`n`tchecksum = `"$('2' * 64)`""
    )
}
Assert-True ($driftedLock -cne $canonicalLock) 'checksum drift fixture did not mutate Jolyne resolution'
Set-Content -LiteralPath $lockPath -NoNewline -Value $driftedLock
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*Cargo.lock*local package*checksum*' 'protocol provenance accepted a checksum for a local package'
