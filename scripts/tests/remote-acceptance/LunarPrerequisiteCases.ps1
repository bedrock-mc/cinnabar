It 'requires a complete mode-matched Lunar manifest for every Zeqa mode' {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-lunar-gate-' + [guid]::NewGuid().ToString('N'))
    try {
        New-Item -ItemType Directory -Path $temporary | Out-Null
        $skeletalPath = Join-Path $temporary 'skeletal\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $skeletalPath) | Out-Null
        @{ schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; mode = 'Diagnostic'; status = 'passed'; diagnostic_complete = $true } |
            ConvertTo-Json | Set-Content -LiteralPath $skeletalPath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty

        $immediatePath = Join-Path $temporary 'immediate\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $immediatePath) | Out-Null
        $immediate = New-SyntheticPhase2LunarManifest -Mode Diagnostic
        $immediate.requested_present_mode = 'Immediate'
        $immediate.final_publication.presentation.requested_present_mode = 'immediate'
        $immediate.final_publication.presentation.effective_present_mode = 'immediate'
        $immediate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $immediatePath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic) | Should BeNullOrEmpty
        { Find-Phase2CompletedLunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic `
            -ExpectedPresentMode Immediate -ExpectedInitialRadius 16 -RequireFullView:$false } | Should Throw

        $diagnosticPath = Join-Path $temporary 'diagnostic\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $diagnosticPath) | Out-Null
        New-SyntheticPhase2LunarManifest -Mode Diagnostic | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $diagnosticPath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Diagnostic).Path | Should Be $diagnosticPath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView) | Should BeNullOrEmpty

        $noFullViewPath = Join-Path $temporary 'candidate-no-full-view\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $noFullViewPath) | Out-Null
        $noFullView = New-SyntheticPhase2LunarManifest -Mode Candidate
        $noFullView.full_view_teleport_gate = $false
        $noFullView | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $noFullViewPath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty

        $badCandidatePath = Join-Path $temporary 'candidate-bad\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $badCandidatePath) | Out-Null
        $badCandidate = New-SyntheticPhase2LunarManifest -Mode Candidate
        $badCandidate.first_stalled_stage = 'meshing'
        $badCandidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $badCandidatePath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView) | Should BeNullOrEmpty

        $candidatePath = Join-Path $temporary 'candidate\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $candidatePath) | Out-Null
        New-SyntheticPhase2LunarManifest -Mode Candidate | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $candidatePath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Candidate -RequireFullView).Path | Should Be $candidatePath
        (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView) | Should BeNullOrEmpty

        $finalPath = Join-Path $temporary 'final\manifest.json'
        New-Item -ItemType Directory -Path (Split-Path -Parent $finalPath) | Out-Null
        New-SyntheticPhase2LunarManifest -Mode Final | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $finalPath
        $result = Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $temporary -Mode Final -RequireFullView
        $result.Path | Should Be $finalPath
        $result.Mode | Should Be 'Final'
        $result.Sha256 | Should Match '^[0-9A-F]{64}$'
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}

It 'requires an exact completed Lunar manifest schema and exact integral numeric fields' {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-lunar-schema-' + [guid]::NewGuid().ToString('N'))
    try {
        New-Item -ItemType Directory -Path $temporary | Out-Null
        $case = 0
        $invalidManifests = [Collections.Generic.List[object]]::new()

        $unknownRoot = New-SyntheticPhase2LunarManifest -Mode Candidate
        $unknownRoot['access_token'] = 'must-not-pass'
        $invalidManifests.Add($unknownRoot)
        $unknownMetrics = New-SyntheticPhase2LunarManifest -Mode Candidate
        $unknownMetrics.metrics_evidence['access_token'] = 'must-not-pass'
        $invalidManifests.Add($unknownMetrics)
        $missingResource = New-SyntheticPhase2LunarManifest -Mode Candidate
        $missingResource.resources_evidence.Remove('reason')
        $invalidManifests.Add($missingResource)
        $unknownPerformance = New-SyntheticPhase2LunarManifest -Mode Candidate
        $unknownPerformance.performance['extra'] = 1
        $invalidManifests.Add($unknownPerformance)

        $missingRoot = New-SyntheticPhase2LunarManifest -Mode Candidate
        $missingRoot.Remove('duration_seconds')
        $invalidManifests.Add($missingRoot)
        $missingPerformance = New-SyntheticPhase2LunarManifest -Mode Candidate
        $missingPerformance.performance.Remove('steady_seconds')
        $invalidManifests.Add($missingPerformance)

        $rootIntegralFields = [ordered]@{
            initial_radius = 16; publication_snapshot_count = 2
            duration_seconds = 150; client_shutdown_grace_seconds = 5
        }
        foreach ($field in $rootIntegralFields.Keys) {
            $expected = $rootIntegralFields[$field]
            foreach ($invalid in @($null, $true, [string]$expected, ([double]$expected + 0.5), -1, [decimal]18446744073709551616)) {
                $manifest = New-SyntheticPhase2LunarManifest -Mode Candidate
                $manifest[$field] = $invalid
                $invalidManifests.Add($manifest)
            }
        }
        $performanceIntegralFields = [ordered]@{
            warmup_seconds = 30; steady_seconds = 120; resource_sample_count = 120
            max_combined_rss_bytes = 681574400
        }
        foreach ($field in $performanceIntegralFields.Keys) {
            $expected = $performanceIntegralFields[$field]
            foreach ($invalid in @($null, $true, [string]$expected, ([double]$expected + 0.5), -1, [decimal]18446744073709551616)) {
                $manifest = New-SyntheticPhase2LunarManifest -Mode Candidate
                $manifest.performance[$field] = $invalid
                $invalidManifests.Add($manifest)
            }
        }

        foreach ($manifest in $invalidManifests) {
            $root = Join-Path $temporary "case-$case"
            New-Item -ItemType Directory -Path $root | Out-Null
            $manifest | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $root 'manifest.json')
            (Find-SyntheticPhase2LunarPrerequisite -RemoteRoot $root -Mode Candidate -RequireFullView) |
                Should BeNullOrEmpty
            $case++
        }
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}
