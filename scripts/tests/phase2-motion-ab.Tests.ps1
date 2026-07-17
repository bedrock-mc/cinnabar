$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$ScriptPath = Join-Path $ProjectRoot 'scripts\phase2-motion-ab.ps1'

Describe 'Phase 2 motion A/B runner' {
    It 'freezes identical FIFO and Immediate leg identities with effective-mode proof' {
        $runId = 'pester-motion-' + [guid]::NewGuid().ToString('N')
        $runDirectory = Join-Path $ProjectRoot ".local\phase2\motion\$runId"
        try {
            & $ScriptPath -RunId $runId -BdsDir 'synthetic-bds' -Assets 'synthetic.mcbea' `
                -NativeRoot '.local/phase2/native/motion' -DurationSeconds 150 -ValidateOnly
            $manifest = Get-Content -Raw -LiteralPath (Join-Path $runDirectory 'manifest.json') | ConvertFrom-Json
            @($manifest.legs).Count | Should Be 2
            $manifest.legs[0].requested_present_mode | Should Be 'Fifo'
            $manifest.legs[1].requested_present_mode | Should Be 'Immediate'
            $manifest.legs[0].require_effective_present_mode_proof | Should Be $true
            $manifest.scene_identity_sha256 | Should Be $manifest.legs[0].scene_identity_sha256
            $manifest.scene_identity_sha256 | Should Be $manifest.legs[1].scene_identity_sha256
            $manifest.performance.resource_samples_per_leg | Should Be 120
            { & $ScriptPath -RunId $runId -BdsDir 'synthetic-bds' -Assets 'synthetic.mcbea' `
                -NativeRoot '.local/phase2/native/motion' -DurationSeconds 150 -ValidateOnly } | Should Throw
            { & $ScriptPath -RunId ($runId + '-short') -BdsDir 'synthetic-bds' `
                -Assets 'synthetic.mcbea' -NativeRoot '.local/phase2/native/motion' `
                -DurationSeconds 149 -ValidateOnly } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $runDirectory -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
