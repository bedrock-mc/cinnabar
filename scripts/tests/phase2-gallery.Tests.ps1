$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
$ScriptPath = Join-Path $ProjectRoot 'scripts\phase2-gallery.ps1'

Describe 'Phase 2 gallery runner' {
    It 'creates one bounded manifest with every lighting and atmosphere comparator' {
        $runId = 'pester-gallery-' + [guid]::NewGuid().ToString('N')
        $runDirectory = Join-Path $ProjectRoot ".local\phase2\galleries\$runId"
        try {
            & $ScriptPath -Gallery LightingAtmosphere -RunId $runId -BdsDir 'synthetic-bds' `
                -Assets 'synthetic.mcbea' -NativeRoot '.local/phase2/native/synthetic' `
                -PresentMode Fifo -DurationSeconds 150 -ValidateOnly
            $manifest = Get-Content -Raw -LiteralPath (Join-Path $runDirectory 'manifest.json') | ConvertFrom-Json
            @($manifest.comparisons).Count | Should Be 4
            @($manifest.comparisons.kind) | Should Be @('lighting', 'fog-air', 'fog-water', 'fog-lava')
            $manifest.performance.resource_sample_count | Should Be 120
            { & $ScriptPath -Gallery LightingAtmosphere -RunId $runId -BdsDir 'synthetic-bds' `
                -Assets 'synthetic.mcbea' -NativeRoot '.local/phase2/native/synthetic' `
                -PresentMode Fifo -DurationSeconds 150 -ValidateOnly } | Should Throw
            { & $ScriptPath -Gallery Cloud -RunId ($runId + '-short') -BdsDir 'synthetic-bds' `
                -Assets 'synthetic.mcbea' -NativeRoot '.local/phase2/native/synthetic' `
                -PresentMode Fifo -DurationSeconds 149 -ValidateOnly } | Should Throw
            { & $ScriptPath -Gallery Cloud -RunId ($runId + '-skip') -BdsDir 'synthetic-bds' `
                -Assets 'synthetic.mcbea' -NativeRoot '.local/phase2/native/synthetic' `
                -PresentMode Fifo -DurationSeconds 150 -SkipClientBuild -ValidateOnly } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $runDirectory -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
