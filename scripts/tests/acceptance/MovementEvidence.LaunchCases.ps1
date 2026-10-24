    It 'builds exact live target plans with candidate-only physics and no free camera' {
        $targets = [ordered]@{
            Lunar = 'pvp.lunarbedrock.com:19134'
            Zeqa = 'zeqa.net:19132'
            Lbsg = 'play.lbsg.net:19132'
            Zeno = 'zenomc.org:19197'
            Venity = 'play.venitymc.com:19132'
            Bds = '127.0.0.1:19132'
        }
        foreach ($target in $targets.Keys) {
            $endpoint = Get-EvidenceTargetEndpoint -Target $target
            $endpoint | Should Be $targets[$target]
            $duration = if ($target -ceq 'Bds') { 60 } else { 300 }
            $plan = New-MovementLaunchPlan -Target $target -Endpoint $endpoint `
                -RunId $script:RunId -MetricsPath 'metrics.json' `
                -DurationSeconds $duration -Scenario CandidatePhysics
            $plan.AppArguments[0] | Should Be '--address'
            $plan.AppArguments[1] | Should Be $endpoint
            ($plan.AppArguments -ccontains '--movement-candidate-physics') | Should Be $true
            ($plan.AppArguments -ccontains '--movement-evidence-target') | Should Be $true
            ($plan.AppArguments -ccontains '--auto-fly') | Should Be $false
            ($plan.AppArguments -ccontains '--native-networking') | Should Be ($target -ceq 'Bds')
        }
    }

    It 'forbids sub-five-minute external plans' {
        foreach ($target in @('Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')) {
            { New-MovementLaunchPlan -Target $target -Endpoint (Get-EvidenceTargetEndpoint $target) `
                    -RunId $script:RunId -MetricsPath metrics.json `
                    -DurationSeconds 299 -Scenario CandidatePhysics } | Should Throw
        }
    }

    It 'preserves the offline BDS candidate plan' {
        $bds = New-MovementLaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
            -RunId $script:RunId -MetricsPath metrics.json `
            -DurationSeconds 60 -Scenario CandidatePhysics
        ($bds.AppArguments -ccontains '--display-name') | Should Be $true
    }
