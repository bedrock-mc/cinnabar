$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$ScriptPath = Join-Path $ProjectRoot 'scripts\remote-acceptance.ps1'

Describe 'Phase 2 remote acceptance runner' {
    It 'enforces create-new, duration, radius, auth locality, and canonical Lunar endpoint' {
        $runId = 'pester-remote-' + [guid]::NewGuid().ToString('N')
        $runDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId"
        $candidateRunDirectory = Join-Path $ProjectRoot ".local\phase2\remote\$runId-candidate"
        try {
            & $ScriptPath -Server Lunar -Mode Diagnostic -RunId $runId -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly
            $manifest = Get-Content -Raw -LiteralPath (Join-Path $runDirectory 'manifest.json') | ConvertFrom-Json
            $manifest.upstream | Should Be 'pvp.lunarbedrock.com:19134'
            $manifest.initial_radius | Should Be 16
            @($manifest.client_arguments) -join ' ' | Should Not Match 'initial.radius'
            $manifest.performance.warmup_seconds | Should Be 30
            $manifest.performance.resource_sample_count | Should Be 120

            & $ScriptPath -Server Lunar -Mode Candidate -RunId ($runId + '-candidate') -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly
            $candidateManifest = Get-Content -Raw -LiteralPath (Join-Path $candidateRunDirectory 'manifest.json') | ConvertFrom-Json
            $candidateManifest.diagnostic_complete | Should Be $false

            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId $runId -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-short') -DurationSeconds 149 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 16 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-radius') -DurationSeconds 150 `
                -AuthCache '.local/auth/microsoft-token.json' -InitialRadius 15 `
                -PresentMode Fifo -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
            { & $ScriptPath -Server Lunar -Mode Diagnostic -RunId ($runId + '-auth') -DurationSeconds 150 `
                -AuthCache '..\token.json' -InitialRadius 16 -PresentMode Fifo `
                -Assets 'synthetic.mcbea' -ValidateOnly } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $runDirectory -Recurse -Force -ErrorAction SilentlyContinue
            Remove-Item -LiteralPath $candidateRunDirectory -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects incomplete release, presentation, frame, lifecycle, and resource evidence' {
        . (Join-Path $ProjectRoot 'scripts\acceptance\Load.ps1')
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-evidence-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $metricsPath = Join-Path $temporary 'metrics.json'
            $resourcesPath = Join-Path $temporary 'resources.json'
            $logPath = Join-Path $temporary 'client.log'
            @{ p95_frame_ms = 16.0; p99_frame_ms = 16.5; max_frame_ms = 49.0; teleport_settle_ms = $null; forced_full_view_remesh_ms = $null } |
                ConvertTo-Json | Set-Content -LiteralPath $metricsPath
            $samples = @(1..120 | ForEach-Object { @{ elapsed_seconds = $_; combined_rss_bytes = 104857600; cpu_percent = 5.0 } })
            @{ schema = 'rust-mcbe-phase2-resources-v1'; warmup_seconds = 30; duration_seconds = 120; processor_count = 8; samples = $samples; summary = @{ sample_count = 120; max_combined_rss_bytes = 104857600; mean_cpu_percent = 5.0; p95_cpu_percent = 5.0 } } |
                ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $resourcesPath
            'PHASE2_PUBLICATION={"presentation":{"build_profile":"release","requested_present_mode":"fifo","effective_present_mode":"fifo","present_mode_proven":true,"graphics_identity_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","assets_manifest_sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}}' |
                Set-Content -LiteralPath $logPath
            { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Not Throw
            (Get-Content -Raw -LiteralPath $metricsPath).Replace('16.5', '17.0') | Set-Content -LiteralPath $metricsPath
            { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Throw

            foreach ($malformed in @($null, 'NaN', $true, -1.0)) {
                @{ p95_frame_ms = $malformed; p99_frame_ms = 16.5; max_frame_ms = 49.0 } |
                    ConvertTo-Json | Set-Content -LiteralPath $metricsPath
                { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                    -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Throw
            }

            @{ p95_frame_ms = 16.0; p99_frame_ms = 16.5; max_frame_ms = 49.0 } |
                ConvertTo-Json | Set-Content -LiteralPath $metricsPath
            (Get-Content -Raw -LiteralPath $logPath).Replace('"present_mode_proven":true', '"present_mode_proven":false') |
                Set-Content -LiteralPath $logPath
            { Assert-Phase2Evidence -MetricsPath $metricsPath -ResourcesPath $resourcesPath `
                -ClientLogPath $logPath -ExpectedPresentMode Fifo -JoinMilliseconds 1500 } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }


    It 'accepts only a schema-valid completed Lunar Diagnostic manifest as the Zeqa prerequisite' {
        . (Join-Path $ProjectRoot 'scripts\acceptance\Load.ps1')
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-lunar-gate-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $manifestPath = Join-Path $temporary 'manifest.json'
            @{ schema = 'wrong'; server = 'Lunar'; mode = 'Diagnostic'; status = 'passed'; diagnostic_complete = $true } |
                ConvertTo-Json | Set-Content -LiteralPath $manifestPath
            (Find-Phase2CompletedLunarDiagnostic -RemoteRoot $temporary) | Should BeNullOrEmpty

            @{ schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; mode = 'Candidate'; status = 'passed'; diagnostic_complete = $true } |
                ConvertTo-Json | Set-Content -LiteralPath $manifestPath
            (Find-Phase2CompletedLunarDiagnostic -RemoteRoot $temporary) | Should BeNullOrEmpty

            @{ schema = 'rust-mcbe-phase2-remote-v1'; server = 'Lunar'; mode = 'Diagnostic'; status = 'passed'; diagnostic_complete = $true } |
                ConvertTo-Json | Set-Content -LiteralPath $manifestPath
            $result = Find-Phase2CompletedLunarDiagnostic -RemoteRoot $temporary
            $result.Path | Should Be $manifestPath
            $result.Sha256 | Should Match '^[0-9A-F]{64}$'
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
