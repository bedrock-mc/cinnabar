# Shared Phase 3 acceptance contract-test helpers: marker-log writing and
# validator invocation. Dot-sourced from Phase3.Tests.ps1 so the functions
# keep their original Describe-block definition scope.
    function Write-MarkerLog {
        param([string]$Name)
        $path = Join-Path $script:TempRoot $Name
        $lines = [Collections.Generic.List[string]]::new()
        $lines.Add('ordinary client log line')
        $lines.Add('RUST_MCBE_PHASE3_IDENTITY=' + ($script:Identity | ConvertTo-Json -Depth 6 -Compress))
        foreach ($frame in $script:Frames) {
            $lines.Add('RUST_MCBE_PHASE3_FRAME=' + ($frame | ConvertTo-Json -Depth 6 -Compress))
        }
        foreach ($event in $script:Events) {
            $lines.Add('RUST_MCBE_PHASE3_EVENT=' + ($event | ConvertTo-Json -Depth 6 -Compress))
        }
        foreach ($violation in $script:Violations) {
            $lines.Add('RUST_MCBE_PHASE3_VIOLATION=' + ($violation | ConvertTo-Json -Depth 6 -Compress))
        }
        foreach ($terminal in $script:Terminals) {
            $lines.Add('RUST_MCBE_PHASE3_TERMINAL=' + ($terminal | ConvertTo-Json -Depth 6 -Compress))
        }
        Set-Content -LiteralPath $path -Value $lines -Encoding utf8
        return $path
    }

    function Invoke-Validator {
        param([string]$Path)
        $runMetadataPath = $Path + '.run.json'
        $metricsPath = $Path + '.metrics.json'
        $outputPath = $Path + '.final.json'
        $scenarioManifestPath = $Path + '.scenario.json'
        $script:RunMetadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $runMetadataPath -Encoding utf8
        $script:Metrics | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $metricsPath -Encoding utf8
        $script:ScenarioManifest | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $scenarioManifestPath -Encoding utf8
        $savedErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $script:Validator `
                -LogPath $Path -ExpectedTarget Bds -ExpectedBuildCommit $script:BuildCommit `
                -ExpectedPregSha256 $script:PregSha256 -ExpectedBregSha256 $script:BregSha256 `
                -ExpectedProtocol 2168 `
                -ExpectedRunId $script:RunId -ExpectedEndpoint $script:Endpoint `
                -ExpectedBridgeEndpoint $script:BridgeEndpoint `
                -ExpectedCoreSha256 $script:CoreSha256 -ExpectedCoreProcessId 41 `
                -ExpectedAppProcessId 42 -RunMetadataPath $runMetadataPath `
                -MetricsPath $metricsPath -OutputPath $outputPath `
                -ScenarioManifestPath $scenarioManifestPath `
                2>&1 | Out-String
            $exitCode = $LASTEXITCODE
        }
        finally {
            $ErrorActionPreference = $savedErrorActionPreference
        }
        return [pscustomobject]@{ ExitCode = $exitCode; Output = $output; Aggregate = $outputPath }
    }
