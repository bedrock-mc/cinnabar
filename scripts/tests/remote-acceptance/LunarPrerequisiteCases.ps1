It 'requires a complete mode-matched Lunar manifest for every Zeqa mode' {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('publication-lunar-gate-' + [guid]::NewGuid().ToString('N'))
    try {
        New-Item -ItemType Directory -Path $temporary | Out-Null
        $skeletalPath = Join-Path $temporary 'skeletal\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $skeletalPath) | Out-Null
        @{ schema = 'rust-mcbe-remote-acceptance-v1'; server = 'Lunar'; mode = 'Diagnostic'; status = 'passed'; diagnostic_complete = $true } |
            ConvertTo-Json | Set-Content -LiteralPath $skeletalPath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty

        $immediatePath = Join-Path $temporary 'immediate\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $immediatePath) | Out-Null
        $immediate = New-SyntheticLunarManifest -Mode Diagnostic
        $immediate.requested_present_mode = 'Immediate'
        $immediate.final_publication.presentation.requested_present_mode = 'immediate'
        $immediate.final_publication.presentation.effective_present_mode = 'immediate'
        $immediate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $immediatePath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty
        { Find-CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic `
            -ExpectedPresentMode Immediate -ExpectedInitialRadius 16 -RequireFullView:$false } | Should Throw

        $diagnosticPath = Join-Path $temporary 'diagnostic\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $diagnosticPath) | Out-Null
        New-SyntheticLunarManifest -Mode Diagnostic | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $diagnosticPath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic).Path | Should Be $diagnosticPath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView) | Should BeNullOrEmpty

        $noFullViewPath = Join-Path $temporary 'candidate-no-full-view\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $noFullViewPath) | Out-Null
        $noFullView = New-SyntheticLunarManifest -Mode Candidate
        $noFullView.full_view_teleport_gate = $false
        $noFullView | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $noFullViewPath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty

        $badCandidatePath = Join-Path $temporary 'candidate-bad\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $badCandidatePath) | Out-Null
        $badCandidate = New-SyntheticLunarManifest -Mode Candidate
        $badCandidate.first_stalled_stage = 'meshing'
        $badCandidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $badCandidatePath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty

        $candidatePath = Join-Path $temporary 'candidate\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $candidatePath) | Out-Null
        New-SyntheticLunarManifest -Mode Candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $candidatePath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView).Path | Should Be $candidatePath
        (Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView) | Should BeNullOrEmpty

        $finalPath = Join-Path $temporary 'final\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $finalPath) | Out-Null
        New-SyntheticLunarManifest -Mode Final | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $finalPath
        $result = Find-SyntheticLunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView
        $result.Path | Should Be $finalPath
        $result.Mode | Should Be 'Final'
        $result.Sha256 | Should Match '^[0-9A-F]{64}$'
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}

It 'requires an exact completed Lunar manifest schema and exact integral numeric fields' {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('publication-lunar-schema-' + [guid]::NewGuid().ToString('N'))
    try {
        New-Item -ItemType Directory -Path $temporary | Out-Null
        $case = 0
        $invalidManifests = [Collections.Generic.List[object]]::new()

        $unknownRoot = New-SyntheticLunarManifest -Mode Candidate
        $unknownRoot['access_token'] = 'must-not-pass'
        $invalidManifests.Add($unknownRoot)
        $unknownMetrics = New-SyntheticLunarManifest -Mode Candidate
        $unknownMetrics.metrics_evidence['access_token'] = 'must-not-pass'
        $invalidManifests.Add($unknownMetrics)
        $missingResource = New-SyntheticLunarManifest -Mode Candidate
        $missingResource.resources_evidence.Remove('reason')
        $invalidManifests.Add($missingResource)
        $unknownPerformance = New-SyntheticLunarManifest -Mode Candidate
        $unknownPerformance.performance['extra'] = 1
        $invalidManifests.Add($unknownPerformance)

        $missingRoot = New-SyntheticLunarManifest -Mode Candidate
        $missingRoot.Remove('duration_seconds')
        $invalidManifests.Add($missingRoot)
        $missingPerformance = New-SyntheticLunarManifest -Mode Candidate
        $missingPerformance.performance.Remove('steady_seconds')
        $invalidManifests.Add($missingPerformance)

        $rootIntegralFields = [ordered]@{
            initial_radius = 16; publication_snapshot_count = 2
            duration_seconds = 150; client_shutdown_grace_seconds = 5
        }
        foreach ($field in $rootIntegralFields.Keys) {
            $expected = $rootIntegralFields[$field]
            foreach ($invalid in @($null, $true, [string]$expected, ([double]$expected + 0.5), -1, [decimal]18446744073709551616)) {
                $manifest = New-SyntheticLunarManifest -Mode Candidate
                $manifest[$field] = $invalid
                $invalidManifests.Add($manifest)
            }
        }
        $performanceIntegralFields = [ordered]@{
            warmup_seconds = 30; steady_seconds = 120; resource_sample_count = 120
            max_client_rss_bytes = 681574400
        }
        foreach ($field in $performanceIntegralFields.Keys) {
            $expected = $performanceIntegralFields[$field]
            foreach ($invalid in @($null, $true, [string]$expected, ([double]$expected + 0.5), -1, [decimal]18446744073709551616)) {
                $manifest = New-SyntheticLunarManifest -Mode Candidate
                $manifest.performance[$field] = $invalid
                $invalidManifests.Add($manifest)
            }
        }

        foreach ($manifest in $invalidManifests) {
            $root = Join-Path $temporary "case-$case"
            New-Item -ItemType Directory -Path $root | Out-Null
            $manifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $root 'manifest.json')
            (Find-SyntheticLunarPrerequisite -RemoteRoot $root -Mode Candidate -RequireFullView) |
                Should BeNullOrEmpty
            $case++
        }
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}
