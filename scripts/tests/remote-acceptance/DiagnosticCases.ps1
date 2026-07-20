It 'rejects missing or incoherent publication evidence as diagnostic completeness' {
    $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-bad-diagnostic-' + [guid]::NewGuid().ToString('N'))
    try {
        New-Item -ItemType Directory -Path $temporary | Out-Null
        $emptyPath = Join-Path $temporary 'empty.log'
        Set-Content -LiteralPath $emptyPath -Value 'no publication evidence'
        { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
            -ClientLogPath $emptyPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw

        $logPath = Join-Path $temporary 'incoherent.log'
        $first = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 1 `
            -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 1 -SubchunksCommitted 1
        $last = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 2 `
            -RequestsConstructed 2 -RequestsSent 2 -ResponsesAdmitted 2 -SubchunksCommitted 2
        $last.presentation.graphics_identity_sha256 = 'cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc'
        @(
            'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
            'PHASE2_PUBLICATION=' + ($last | ConvertTo-Json -Depth 20 -Compress)
        ) | Set-Content -LiteralPath $logPath
        { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
            -ClientLogPath $logPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw

        $regressionPath = Join-Path $temporary 'regression.log'
        $first.presentation.graphics_identity_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
        $last.presentation.graphics_identity_sha256 = 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa'
        $first.publication.stages.requests_constructed = 10
        $last.publication.stages.requests_constructed = 9
        @(
            'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
            'PHASE2_PUBLICATION=' + ($last | ConvertTo-Json -Depth 20 -Compress)
        ) | Set-Content -LiteralPath $regressionPath
        { Complete-Phase2DiagnosticEvidence -Manifest ([pscustomobject]@{ mode = 'Diagnostic' }) `
            -ClientLogPath $regressionPath -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw
    }
    finally {
        Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
    }
}
