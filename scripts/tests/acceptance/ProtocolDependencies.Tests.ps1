$expectedAxolotlStackRevision = 'c4540512dc47833bb40363da7ad1161110d64b67'
$expectedProtocolgenRevision = [string](Get-BedrockTargetManifest -ProjectRoot $ProjectRoot).protocolgen_commit
$expectedLicenseSha256 = '62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184'

# Both acceptance implementations must follow the same target-owned generator identity.
$pinFixture = Join-Path $TempRoot 'generator target policy'
$pinAssets = Join-Path $pinFixture 'assets'
New-Item -ItemType Directory -Force -Path $pinAssets | Out-Null
$pinPath = Join-Path $pinAssets 'bedrock-target.json'
$pinSource = Get-Content -Raw -LiteralPath (Join-Path $ProjectRoot 'assets\bedrock-target.json')
$pinSource | Set-Content -LiteralPath $pinPath -Encoding UTF8
Assert-Equal $expectedProtocolgenRevision (Get-PinnedProtocolgenCommit -ProjectRoot $pinFixture) `
    'acceptance generator identity differs from target policy'
$pinPolicy = $pinSource | ConvertFrom-Json
$alternateRevision = 'a' * 40
$pinPolicy.protocolgen_commit = $alternateRevision
$pinPolicy | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $pinPath -Encoding UTF8
Assert-Equal $alternateRevision (Get-PinnedProtocolgenCommit -ProjectRoot $pinFixture) `
    'acceptance cached a generator identity instead of reading target policy'
foreach ($invalidRevision in @('', 'abc', ('g' * 40), ('A' * 40), 123)) {
    $pinPolicy.protocolgen_commit = $invalidRevision
    $pinPolicy | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $pinPath -Encoding UTF8
    Assert-ThrowsLike { Get-PinnedProtocolgenCommit -ProjectRoot $pinFixture } `
        '*protocolgen_commit*' 'acceptance admitted an invalid target generator identity'
}
$pinPolicy.PSObject.Properties.Remove('protocolgen_commit')
$pinPolicy | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath $pinPath -Encoding UTF8
Assert-ThrowsLike { Get-PinnedProtocolgenCommit -ProjectRoot $pinFixture } `
    '*protocolgen_commit*' 'acceptance admitted missing target generator identity'

$validationSource = Get-Content -Raw -LiteralPath `
    (Join-Path $ProjectRoot 'scripts\acceptance\Orchestration\Validate.ps1')
Assert-True $validationSource.Contains('-ExpectedAxolotlStackRevision $PinnedAxolotlStackCommit') `
    'acceptance validation does not pass its owned Axolotl Stack pin to protocol provenance'
Assert-True $validationSource.Contains('-ExpectedProtocolgenRevision $PinnedProtocolgenCommit') `
    'acceptance validation does not pass its owned protocolgen pin to protocol provenance'
Assert-True (-not $validationSource.Contains('$PinnedValentineForkCommit')) `
    'acceptance validation references the removed Valentine fork pin'
Assert-True (-not $validationSource.Contains('$PinnedValentineUpstreamCommit')) `
    'acceptance validation references the removed Valentine upstream pin'
Assert-True (-not $validationSource.Contains('$PinnedValentineLicenseSha256')) `
    'acceptance validation references the renamed Valentine license pin'

. (Join-Path $ProjectRoot 'scripts\acceptance\Gophertunnel.ps1')
. (Join-Path $ProjectRoot 'scripts\acceptance\Markers.ps1')

function Assert-GophertunnelPinFixtures {
    $fixtureRoot = Join-Path $TempRoot 'gophertunnel pin with spaces'
    $modulePath = 'github.com/sandertv/gophertunnel'
    $forkPath = 'github.com/hashimthearab/gophertunnel'
    $firstCommit = 'a' * 40
    $firstVersion = 'v0.0.0-20200102030405-{0}' -f $firstCommit.Substring(0, 12)
    $savedExitCode = Get-Variable LASTEXITCODE -Scope Global -ErrorAction SilentlyContinue
    $savedExitCodeValue = $null
    if ($null -ne $savedExitCode) { $savedExitCodeValue = $savedExitCode.Value }

    function Reset-GophertunnelPinFixture {
        param([string]$Version = $firstVersion, [string]$Commit = $firstCommit)

        $script:pinFixtureSource = [ordered]@{
            Replace = @([ordered]@{
                Old = [ordered]@{ Path = $modulePath }
                New = [ordered]@{ Path = $forkPath; Version = $Version }
            })
        }
        $script:pinFixtureModule = [ordered]@{
            Path = $modulePath
            Replace = [ordered]@{ Path = $forkPath; Version = $Version }
        }
        $script:pinFixtureDownload = [ordered]@{
            Path = $forkPath
            Version = $Version
            Origin = [ordered]@{ VCS = 'git'; URL = "https://$forkPath"; Hash = $Commit }
        }
        $script:pinFixtureModuleJson = $null
        $script:pinFixtureSourceJson = $null
    }

    function go {
        Assert-Equal '-C' $args[0] 'gophertunnel provenance omitted its explicit Go working directory'
        $global:LASTEXITCODE = 0
        switch ($args[2..($args.Count - 1)] -join ' ') {
            'mod edit -json' {
                Assert-Equal (Join-Path $fixtureRoot 'core') $args[1] 'pin was not read from canonical core/go.mod'
                if ($null -ne $script:pinFixtureSourceJson) { return $script:pinFixtureSourceJson }
                return ($script:pinFixtureSource | ConvertTo-Json -Depth 6)
            }
            "list -m -json $modulePath" {
                Assert-Equal $fixtureRoot $args[1] 'module was not resolved against the workspace graph'
                if ($null -ne $script:pinFixtureModuleJson) { return $script:pinFixtureModuleJson }
                return ($script:pinFixtureModule | ConvertTo-Json -Depth 6)
            }
            default {
                Assert-Equal $fixtureRoot $args[1] 'origin was not verified from the workspace root'
                Assert-Equal "mod download -json $forkPath@$($script:pinFixtureSource.Replace[0].New.Version)" `
                    ($args[2..($args.Count - 1)] -join ' ') 'download query did not follow the canonical replacement'
                return ($script:pinFixtureDownload | ConvertTo-Json -Depth 6)
            }
        }
    }

    try {
        Reset-GophertunnelPinFixture
        Assert-Equal $firstCommit (Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot) `
            'gophertunnel commit was not derived from the verified module origin'
        $changedCommit = 'b' * 40
        $changedVersion = 'v1.2.3-0.20210102030405-{0}' -f $changedCommit.Substring(0, 12)
        Reset-GophertunnelPinFixture -Version $changedVersion -Commit $changedCommit
        Assert-Equal $changedCommit (Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot) `
            'gophertunnel provenance did not follow an updated canonical pin'
        $prereleaseVersion = 'v1.2.3-beta.0.20210102030405-{0}' -f $changedCommit.Substring(0, 12)
        Reset-GophertunnelPinFixture -Version $prereleaseVersion -Commit $changedCommit
        Assert-Equal $changedCommit (Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot) `
            'gophertunnel provenance rejected a valid prerelease pseudo-version'

        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace[0].New.Path = 'github.com/other/gophertunnel'
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*core/go.mod*canonical*' 'gophertunnel provenance accepted a different canonical fork'
        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace[0].Old.Version = 'v1.0.0'
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*core/go.mod*unversioned*' 'gophertunnel provenance accepted a version-scoped replacement'
        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace[0].Old.Version = $null
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*core/go.mod*unversioned*' 'gophertunnel provenance accepted a malformed null replacement scope'
        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace += $script:pinFixtureSource.Replace[0]
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*exactly one*' 'gophertunnel provenance accepted duplicate canonical replacements'
        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace[0].New.Version = 'v1.2.3'
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*core/go.mod*pseudo-version*' 'gophertunnel provenance accepted an unpinned release version'
        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace[0].New.Version = $firstVersion + "`n"
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*core/go.mod*pseudo-version*' 'gophertunnel provenance accepted a pseudo-version with trailing LF'
        Reset-GophertunnelPinFixture
        $script:pinFixtureSource.Replace[0].New.Version = $firstVersion.Replace('20200102', '20201302')
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*valid pinned pseudo-version*' 'gophertunnel provenance accepted an invalid pseudo-version timestamp'
        Reset-GophertunnelPinFixture
        $script:pinFixtureModule.Replace.Version = $changedVersion
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*different*gophertunnel*replacement*' 'gophertunnel provenance accepted a workspace pin override'
        Reset-GophertunnelPinFixture
        $script:pinFixtureModule.Replace.Version = @($firstVersion)
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*different*gophertunnel*replacement*' 'gophertunnel provenance accepted a non-string resolved version'
        foreach ($case in @(
            @{ Field = 'Hash'; Value = ('c' * 40) },
            @{ Field = 'Hash'; Value = ('a' * 39) },
            @{ Field = 'Hash'; Value = ('A' * 40) },
            @{ Field = 'Hash'; Value = ($firstCommit + "`n") },
            @{ Field = 'URL'; Value = 'https://github.com/other/gophertunnel' },
            @{ Field = 'VCS'; Value = 'hg' }
        )) {
            Reset-GophertunnelPinFixture
            $script:pinFixtureDownload.Origin[$case.Field] = $case.Value
            Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
                '*origin*expected exact commit*' "gophertunnel provenance accepted a wrong origin $($case.Field)"
        }
        Reset-GophertunnelPinFixture
        $script:pinFixtureSourceJson = '{'
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*go mod edit*malformed*' 'gophertunnel provenance accepted malformed canonical module metadata'
        foreach ($malformed in @('{', '[]')) {
            Reset-GophertunnelPinFixture
            $script:pinFixtureModuleJson = $malformed
            Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
                '*go list -m*malformed*' 'gophertunnel provenance accepted malformed resolved module metadata'
        }
        Reset-GophertunnelPinFixture
        $script:pinFixtureModuleJson = '{"Path":"wrong","Path":"github.com/sandertv/gophertunnel"}'
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*go list -m*duplicate field*' 'gophertunnel provenance accepted duplicate resolved module fields'
        Reset-GophertunnelPinFixture
        $script:pinFixtureModuleJson = ($script:pinFixtureModule | ConvertTo-Json -Depth 6).Replace('"Path":', '"path":')
        Assert-ThrowsLike { Get-PinnedGophertunnelCommit -ProjectRoot $fixtureRoot } `
            '*different*gophertunnel*replacement*' 'gophertunnel provenance accepted wrong-case JSON schema keys'
    }
    finally {
        if ($null -eq $savedExitCode) { Remove-Variable LASTEXITCODE -Scope Global -ErrorAction SilentlyContinue }
        else { $global:LASTEXITCODE = $savedExitCodeValue }
        Remove-Variable pinFixtureSource, pinFixtureModule, pinFixtureDownload, pinFixtureModuleJson, pinFixtureSourceJson `
            -Scope Script -ErrorAction SilentlyContinue
    }
}

Assert-GophertunnelPinFixtures

$PinnedAxolotlStackCommit = $expectedAxolotlStackRevision
$PinnedProtocolgenCommit = $expectedProtocolgenRevision
$PinnedJolyneLicenseSha256 = $expectedLicenseSha256
$protocolMetadata = Get-ProtocolDependencyProvenanceMetadata
Assert-Equal 4 $protocolMetadata.Count 'protocol provenance metadata added or omitted a field'
Assert-Equal 'vendored-path' $protocolMetadata.protocol_dependency_resolution 'protocol dependency resolution metadata drifted'
Assert-Equal $expectedAxolotlStackRevision $protocolMetadata.pinned_axolotl_stack_commit 'Axolotl Stack metadata drifted'
Assert-Equal $expectedProtocolgenRevision $protocolMetadata.pinned_protocolgen_commit 'protocolgen metadata drifted'
Assert-Equal $expectedLicenseSha256 $protocolMetadata.pinned_jolyne_license_sha256 'retained license metadata drifted'

# Copies every workspace member tree resolved by Cargo, including new parent directories.
function Copy-ProtocolDependencyProvenanceFixture {
    param(
        [Parameter(Mandatory = $true)][string]$SourceRoot,
        [Parameter(Mandatory = $true)][string]$DestinationRoot
    )

    & python (Join-Path $SourceRoot 'scripts/tests/copy_workspace.py') $SourceRoot $DestinationRoot
    if ($LASTEXITCODE -ne 0) {
        throw 'could not copy the workspace for protocol dependency provenance'
    }
}

function Assert-TestProtocolDependencyProvenance {
    param([Parameter(Mandatory = $true)][string]$Root)

    Assert-ProtocolDependencyProvenance `
        -ProjectRoot $Root `
        -ExpectedAxolotlStackRevision $expectedAxolotlStackRevision `
        -ExpectedProtocolgenRevision $expectedProtocolgenRevision `
        -ExpectedLicenseSha256 $expectedLicenseSha256
}

$null = Assert-TestProtocolDependencyProvenance -Root $ProjectRoot

$markersSource = Get-Content -Raw -LiteralPath (Join-Path $ProjectRoot 'scripts\acceptance\Markers.ps1')
$treeKillStart = $markersSource.IndexOf('function Stop-ProtocolMetadataProcessTree {', [StringComparison]::Ordinal)
$treeKillEnd = $markersSource.IndexOf('function Wait-ProtocolMetadataCopyTasks {', $treeKillStart, [StringComparison]::Ordinal)
Assert-True ($treeKillStart -ge 0 -and $treeKillEnd -gt $treeKillStart) 'protocol provenance has no bounded process-tree termination helper'
$treeKillSource = $markersSource.Substring($treeKillStart, $treeKillEnd - $treeKillStart)
Assert-True ($treeKillSource.Contains("'taskkill.exe'")) 'Windows protocol timeout does not use the process-tree terminator'
Assert-True ($treeKillSource.Contains('"/PID $([int]$Process.Id) /T /F"')) 'Windows protocol timeout does not target the exact Cargo PID tree'
Assert-True ($treeKillSource.Contains('$Process.Kill()')) 'protocol timeout has no direct-process termination fallback'
Assert-True ($treeKillSource.Contains('$Process.WaitForExit(10000)')) 'protocol timeout termination wait is not bounded'
Assert-True ($markersSource.Contains('-Tasks @($metadataStdoutCopy, $metadataStderrCopy) -TimeoutMilliseconds 10000')) 'protocol timeout output drain is not bounded'

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
    $canonicalManifest.Replace('path = "vendor/bedrock-protocol-1-26-51"', 'path = "..\outside\bedrock-protocol-1-26-51"')
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*cargo metadata*' 'protocol provenance accepted a drifted codec path declaration'
Set-Content -LiteralPath $manifestPath -NoNewline -Value $canonicalManifest

$vendorRoot = Join-Path $fixtureRoot 'crates\protocol\vendor'
Copy-Item -LiteralPath (Join-Path $vendorRoot 'bedrock-protocol-1-26-51') `
    -Destination (Join-Path $vendorRoot 'bedrock-protocol-1-26-51-decoy') -Recurse
$canonicalStringDecoys = @'
[dependencies]
bedrock-protocol-1-26-51 = { path = "vendor/bedrock-protocol-1-26-51" }
'@
$quotedWrongPaths = $canonicalManifest.Replace(
    'publish = false',
    "publish = false`ndescription = `"`"`"`n$canonicalStringDecoys`n`"`"`""
).Replace(
    'bedrock-protocol-1-26-51 = { path = "vendor/bedrock-protocol-1-26-51" }',
    '"bedrock-protocol-1-26-51" = { path = "vendor/bedrock-protocol-1-26-51-decoy" }'
)
Set-Content -LiteralPath $manifestPath -NoNewline -Value $quotedWrongPaths
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*vendored path*' 'protocol provenance accepted canonical declarations inside a multiline string while quoted real keys resolved wrong paths'
Set-Content -LiteralPath $manifestPath -NoNewline -Value $canonicalManifest

Set-Content -LiteralPath $manifestPath -NoNewline -Value ($canonicalManifest + @'

[target.'cfg(unix)'.dependencies]
bedrock-protocol-1-26-51 = { path = "vendor/bedrock-protocol-1-26-51" }
'@)
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*bedrock-protocol-1-26-51*exactly once*' 'protocol provenance accepted an additional target-table codec declaration'

$inactiveDecoy = $canonicalManifest.Replace(
    'bedrock-protocol-1-26-51 = { path = "vendor/bedrock-protocol-1-26-51" }',
    '# active codec declaration removed'
) + @'

[target.'cfg(unix)'.dependencies]
bedrock-protocol-1-26-51 = { path = "vendor/bedrock-protocol-1-26-51" }
'@
Set-Content -LiteralPath $manifestPath -NoNewline -Value $inactiveDecoy
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*bedrock-protocol-1-26-51*normal non-target*' 'protocol provenance accepted an inactive target-table codec decoy'
Set-Content -LiteralPath $manifestPath -NoNewline -Value $canonicalManifest

$upstreamPath = Join-Path $fixtureRoot 'crates\protocol\vendor\UPSTREAM.md'
$canonicalUpstream = Get-Content -Raw -LiteralPath $upstreamPath
Set-Content -LiteralPath $upstreamPath -NoNewline -Value `
    $canonicalUpstream.Replace($expectedAxolotlStackRevision, ('0' * 40))
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*Axolotl Stack revision*' 'protocol provenance accepted drifted Axolotl Stack metadata'
Set-Content -LiteralPath $upstreamPath -NoNewline -Value `
    $canonicalUpstream.Replace($expectedProtocolgenRevision, ('1' * 40))
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*protocolgen revision*' 'protocol provenance accepted drifted protocolgen metadata'
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
    "name = `"bedrock-protocol-1-26-51`"`r`nversion = `"0.1.0`"",
    "name = `"bedrock-protocol-1-26-51`"`r`nversion = `"0.1.0`"`r`n   source   = `"git+https://github.com/HashimTheArab/axolotl-stack.git?rev=$expectedAxolotlStackRevision#$expectedAxolotlStackRevision`""
)
if ($driftedLock -ceq $canonicalLock) {
    $driftedLock = $canonicalLock.Replace(
        "name = `"bedrock-protocol-1-26-51`"`nversion = `"0.1.0`"",
        "name = `"bedrock-protocol-1-26-51`"`nversion = `"0.1.0`"`n   source   = `"git+https://github.com/HashimTheArab/axolotl-stack.git?rev=$expectedAxolotlStackRevision#$expectedAxolotlStackRevision`""
    )
}
Assert-True ($driftedLock -cne $canonicalLock) 'lock drift fixture did not mutate codec resolution'
Set-Content -LiteralPath $lockPath -NoNewline -Value $driftedLock
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*Cargo.lock*local package*source*' 'protocol provenance accepted a Git source for a local package'
Set-Content -LiteralPath $lockPath -NoNewline -Value $canonicalLock

$driftedLock = $canonicalLock.Replace(
    "name = `"bedrock-protocol-1-26-51`"`r`nversion = `"0.1.0`"",
    "name = `"bedrock-protocol-1-26-51`"`r`nversion = `"0.1.0`"`r`n`tchecksum = `"$('2' * 64)`""
)
if ($driftedLock -ceq $canonicalLock) {
    $driftedLock = $canonicalLock.Replace(
        "name = `"bedrock-protocol-1-26-51`"`nversion = `"0.1.0`"",
        "name = `"bedrock-protocol-1-26-51`"`nversion = `"0.1.0`"`n`tchecksum = `"$('2' * 64)`""
    )
}
Assert-True ($driftedLock -cne $canonicalLock) 'checksum drift fixture did not mutate codec resolution'
Set-Content -LiteralPath $lockPath -NoNewline -Value $driftedLock
Assert-ThrowsLike {
    Assert-TestProtocolDependencyProvenance -Root $fixtureRoot
} '*Cargo.lock*local package*checksum*' 'protocol provenance accepted a checksum for a local package'
