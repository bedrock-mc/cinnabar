# Phase 3 launch-plan contract cases, extracted verbatim from Phase3.Tests.ps1
# and dot-sourced back at their original registration position so suite
# ordering, names, and totals are unchanged.
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
            $endpoint = Get-Phase3TargetEndpoint -Target $target
            $endpoint | Should Be $targets[$target]
            $authCache = if ($target -ceq 'Bds') { $null } else { '.local/auth/token.json' }
            $duration = if ($target -ceq 'Bds') { 60 } else { 300 }
            $plan = New-Phase3LaunchPlan -Target $target -Endpoint $endpoint `
                -RunId $script:RunId -SocketDirectory 'socket' -MetricsPath 'metrics.json' `
                -DurationSeconds $duration -Scenario CandidatePhysics -AuthCache $authCache
            $plan.CoreArguments -join ' ' | Should Match ([regex]::Escape("-upstream $endpoint"))
            ($plan.AppArguments -ccontains '--phase3-candidate-physics') | Should Be $true
            ($plan.AppArguments -ccontains '--phase3-evidence-target') | Should Be $true
            ($plan.AppArguments -ccontains '--auto-fly') | Should Be $false
            ($plan.CoreArguments -ccontains '-auth-cache') | Should Be ($target -cne 'Bds')
        }
    }

    It 'forbids missing authentication on all external plans' {
        foreach ($target in @('Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')) {
            { New-Phase3LaunchPlan -Target $target -Endpoint (Get-Phase3TargetEndpoint $target) `
                    -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                    -DurationSeconds 300 -Scenario CandidatePhysics } | Should Throw
        }
    }

    It 'forbids sub-five-minute external plans' {
        foreach ($target in @('Lunar', 'Zeqa', 'Lbsg', 'Zeno', 'Venity')) {
            { New-Phase3LaunchPlan -Target $target -Endpoint (Get-Phase3TargetEndpoint $target) `
                    -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                    -DurationSeconds 299 -Scenario CandidatePhysics -AuthCache token.json } | Should Throw
        }
    }

    It 'preserves the offline BDS candidate plan' {
        $bds = New-Phase3LaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 60 -Scenario CandidatePhysics
        ($bds.CoreArguments -ccontains '-auth-cache') | Should Be $false
    }

    It 'keeps the core command byte-identical when no extra core arguments are supplied' {
        $plan = New-Phase3LaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 60 -Scenario CandidatePhysics
        ($plan.CoreArguments -join ' ') | Should Be '-socket-dir socket -upstream 127.0.0.1:19132'
        @($plan.CoreExtraArguments).Count | Should Be 0
        $remote = New-Phase3LaunchPlan -Target Venity -Endpoint 'play.venitymc.com:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 300 -Scenario CandidatePhysics -AuthCache token.json
        ($remote.CoreArguments -join ' ') |
            Should Be '-socket-dir socket -upstream play.venitymc.com:19132 -auth-cache token.json'
        @($remote.CoreExtraArguments).Count | Should Be 0
    }

    It 'passes allowlisted core extra arguments through verbatim after the standard arguments' {
        $plan = New-Phase3LaunchPlan -Target Venity -Endpoint 'play.venitymc.com:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 300 -Scenario CandidatePhysics -AuthCache token.json `
            -CoreExtraArgs @('-upstream-client-cache')
        ($plan.CoreArguments -join ' ') | Should Be (
            '-socket-dir socket -upstream play.venitymc.com:19132 -auth-cache token.json ' +
            '-upstream-client-cache'
        )
        ($plan.CoreExtraArguments -join ' ') | Should Be '-upstream-client-cache'
        $paired = New-Phase3LaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
            -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
            -DurationSeconds 60 -Scenario FreeCameraSilence `
            -CoreExtraArgs @('-quota-mib', '512')
        ($paired.CoreArguments -join ' ') |
            Should Be '-socket-dir socket -upstream 127.0.0.1:19132 -quota-mib 512'
        ($paired.CoreExtraArguments -join ' ') | Should Be '-quota-mib 512'
    }

    It 'rejects disallowed core extra argument tokens before building any plan' {
        foreach ($rejected in @(
            '', 'not-a-flag-or-value?', '-Upstream', '--shorthand', '-dup=1',
            '"inject"', 'two words', '-ok;rm', '-pipe|it', '-back\slash'
        )) {
            { New-Phase3LaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
                    -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                    -DurationSeconds 60 -Scenario CandidatePhysics -CoreExtraArgs @($rejected) } |
                Should Throw
        }
        { New-Phase3LaunchPlan -Target Bds -Endpoint '127.0.0.1:19132' `
                -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
                -DurationSeconds 60 -Scenario CandidatePhysics `
                -CoreExtraArgs @(1..17 | ForEach-Object { "-cap-$_" }) } | Should Throw
    }
