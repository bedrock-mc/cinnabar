[CmdletBinding()]
param(
    [ValidateSet(
        'All',
        'AliasMarkerEquality',
        'NestedRuntimeRejection',
        'TargetMarkerThroughAlias',
        'LegacyMarkerMigration',
        'ExecutableRefresh',
        'PathSyntax'
    )]
    [string]$Case = 'All'
)

$ErrorActionPreference = 'Stop'

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param([string]$Expected, [string]$Actual, [string]$Message)
    if ($Expected -cne $Actual) {
        throw "$Message`nexpected: $Expected`nactual:   $Actual"
    }
}

function New-TestJunction {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Target
    )

    $output = & cmd.exe /d /c mklink /J "`"$Path`"" "`"$Target`"" 2>&1
    if ($LASTEXITCODE -ne 0) {
        throw "failed to create test junction: $($output -join [Environment]::NewLine)"
    }
}

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$AcceptanceScript = Join-Path $ProjectRoot 'scripts\acceptance.ps1'
$TempRoot = Join-Path ([IO.Path]::GetTempPath()) ("rust-mcbe runtime safety {0}" -f [guid]::NewGuid().ToString('N'))
$SourceTarget = Join-Path $TempRoot 'BDS target'
$SourceAlias = Join-Path $TempRoot 'BDS alias'
$MarkerName = '.rust-mcbe-runtime-owner'

try {
    New-Item -ItemType Directory -Path $SourceTarget -Force | Out-Null
    [IO.File]::WriteAllText(
        (Join-Path $SourceTarget 'bedrock_server.exe'),
        'fixture',
        [Text.UTF8Encoding]::new($false)
    )
    New-TestJunction -Path $SourceAlias -Target $SourceTarget

    $env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY = '1'
    try {
        . $AcceptanceScript `
            -DryRun `
            -DurationSeconds 900 `
            -BdsDir $SourceTarget `
            -MetricsOut (Join-Path $TempRoot 'unused.json')
    }
    finally {
        Remove-Item Env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -ErrorAction SilentlyContinue
    }

    $cases = if ($Case -eq 'All') {
        @(
            'AliasMarkerEquality',
            'NestedRuntimeRejection',
            'TargetMarkerThroughAlias',
            'LegacyMarkerMigration',
            'ExecutableRefresh',
            'PathSyntax'
        )
    }
    else {
        @($Case)
    }

    foreach ($selectedCase in $cases) {
        switch ($selectedCase) {
            'AliasMarkerEquality' {
                $targetMarker = Get-RuntimeOwnershipMarker -SourcePath $SourceTarget
                $aliasMarker = Get-RuntimeOwnershipMarker -SourcePath $SourceAlias
                Assert-Equal $targetMarker $aliasMarker 'junction alias and target produced different ownership markers'
            }
            'NestedRuntimeRejection' {
                $nestedRuntime = Join-Path $SourceAlias 'nested stable runtime'
                $rejected = $false
                try {
                    $null = Set-StableRuntime `
                        -SourceDirectory $SourceTarget `
                        -RuntimeDirectory $nestedRuntime `
                        -ExecutableName 'missing-bedrock-server.exe'
                }
                catch {
                    $rejected = $true
                }
                Assert-True (-not (Test-Path -LiteralPath $nestedRuntime)) 'nested runtime was created before overlap rejection'
                Assert-True (-not (Test-Path -LiteralPath (Join-Path $nestedRuntime $MarkerName))) 'owner marker was created before overlap rejection'
                Assert-True $rejected 'junction-alias descendant runtime was accepted'
            }
            'TargetMarkerThroughAlias' {
                $runtime = Join-Path $TempRoot 'target marker runtime'
                New-Item -ItemType Directory -Path $runtime | Out-Null
                $targetMarker = Get-RuntimeOwnershipMarker -SourcePath $SourceTarget
                [IO.File]::WriteAllText(
                    (Join-Path $runtime $MarkerName),
                    $targetMarker,
                    [Text.UTF8Encoding]::new($false)
                )

                $executable = Set-StableRuntime `
                    -SourceDirectory $SourceAlias `
                    -RuntimeDirectory $runtime `
                    -ExecutableName 'bedrock_server.exe'
                Assert-True (Test-Path -LiteralPath $executable -PathType Leaf) 'target Go marker was rejected through its junction alias'
                Assert-Equal $targetMarker ([IO.File]::ReadAllText((Join-Path $runtime $MarkerName))) 'canonical owner marker changed unexpectedly'
            }
            'LegacyMarkerMigration' {
                $runtime = Join-Path $TempRoot 'legacy marker runtime'
                New-Item -ItemType Directory -Path $runtime | Out-Null
                $legacyMarker = Get-RuntimeOwnershipMarker -SourcePath $SourceTarget -Legacy
                $canonicalMarker = Get-RuntimeOwnershipMarker -SourcePath $SourceTarget
                Assert-True ($legacyMarker -cne $canonicalMarker) 'Windows legacy and canonical marker fixtures unexpectedly match'
                [IO.File]::WriteAllText(
                    (Join-Path $runtime $MarkerName),
                    $legacyMarker,
                    [Text.UTF8Encoding]::new($false)
                )

                $null = Set-StableRuntime `
                    -SourceDirectory $SourceAlias `
                    -RuntimeDirectory $runtime `
                    -ExecutableName 'bedrock_server.exe'
                Assert-Equal $canonicalMarker ([IO.File]::ReadAllText((Join-Path $runtime $MarkerName))) 'accepted legacy marker was not migrated to canonical bytes'
                Assert-True (@(Get-ChildItem -LiteralPath $runtime -Force -Filter '*.tmp').Count -eq 0) 'marker migration left a temporary file behind'

                $differentSource = Join-Path $TempRoot 'different BDS target'
                $differentRuntime = Join-Path $TempRoot 'different legacy marker runtime'
                New-Item -ItemType Directory -Path $differentSource, $differentRuntime | Out-Null
                [IO.File]::WriteAllText(
                    (Join-Path $differentSource 'bedrock_server.exe'),
                    'different fixture',
                    [Text.UTF8Encoding]::new($false)
                )
                $differentLegacyMarker = Get-RuntimeOwnershipMarker -SourcePath $differentSource -Legacy
                [IO.File]::WriteAllText(
                    (Join-Path $differentRuntime $MarkerName),
                    $differentLegacyMarker,
                    [Text.UTF8Encoding]::new($false)
                )
                $differentRejected = $false
                try {
                    $null = Set-StableRuntime `
                        -SourceDirectory $SourceAlias `
                        -RuntimeDirectory $differentRuntime `
                        -ExecutableName 'bedrock_server.exe'
                }
                catch {
                    $differentRejected = $true
                }
                Assert-True $differentRejected 'legacy marker for a different canonical source was accepted'
                Assert-Equal $differentLegacyMarker ([IO.File]::ReadAllText((Join-Path $differentRuntime $MarkerName))) 'rejected legacy marker was mutated'
            }
            'ExecutableRefresh' {
                $runtime = Join-Path $TempRoot 'executable refresh runtime'
                $firstExecutable = Set-StableRuntime `
                    -SourceDirectory $SourceTarget `
                    -RuntimeDirectory $runtime `
                    -ExecutableName 'bedrock_server.exe'
                Assert-Equal 'fixture' ([IO.File]::ReadAllText($firstExecutable)) 'initial executable was not copied'

                [IO.File]::WriteAllText(
                    (Join-Path $SourceTarget 'bedrock_server.exe'),
                    'updated fixture',
                    [Text.UTF8Encoding]::new($false)
                )
                $updatedExecutable = Set-StableRuntime `
                    -SourceDirectory $SourceAlias `
                    -RuntimeDirectory $runtime `
                    -ExecutableName 'bedrock_server.exe'
                Assert-Equal 'updated fixture' ([IO.File]::ReadAllText($updatedExecutable)) 'changed source executable was not atomically refreshed'
            }
            'PathSyntax' {
                Assert-Equal '\\?\C:\' (ConvertTo-NormalizedRuntimePath '\\?\C:\') 'extended drive root lost its root separator'
                Assert-Equal 'C:\' (ConvertFrom-ExtendedWindowsPath '\\?\C:\') 'extended drive root was not converted to legacy form'
                Assert-Equal '\\?\UNC\server\share\' (ConvertTo-NormalizedRuntimePath '\\?\UNC\server\share\') 'extended UNC root lost its root separator'
                Assert-Equal '\\server\share\' (ConvertFrom-ExtendedWindowsPath '\\?\UNC\server\share\') 'extended UNC root was not converted to legacy form'
                Assert-True (Test-RuntimePathContains '\\?\C:\' '\\?\c:\child') 'drive root did not contain its child case-insensitively'
                Assert-True (Test-RuntimePathContains '\\?\UNC\server\share\' '\\?\unc\SERVER\SHARE\child') 'UNC root did not contain its child case-insensitively'
                Assert-True (-not (Test-RuntimePathContains '\\?\C:\source' '\\?\C:\source-sibling')) 'segment-aware overlap treated a sibling prefix as a child'
            }
        }
    }

    Write-Output "acceptance runtime safety tests ($Case): PASS"
}
finally {
    if (Test-Path -LiteralPath $SourceAlias) {
        [IO.Directory]::Delete($SourceAlias)
    }
    if (Test-Path -LiteralPath $TempRoot) {
        Remove-Item -LiteralPath $TempRoot -Recurse -Force
    }
}
