It 'rejects an over-capacity outbox depth as the only changed condition' {
    $script:Frames[1].outbox_depth = 33
    (Invoke-Validator (Write-MarkerLog 'bound-outbox.log')).ExitCode | Should Not Be 0
}

It 'rejects an unsupported input mode as the only changed condition' {
    $script:Frames[2].input_mode = 'RememberedTouch'
    (Invoke-Validator (Write-MarkerLog 'enum-input-mode.log')).ExitCode | Should Not Be 0
}

It 'rejects an over-capacity event record array as the only changed condition' {
    $script:Events = @(0..256 | ForEach-Object {
        [ordered]@{
            schema = 'rust-mcbe-phase3-event-v1'; kind = 'correction'; session_generation = 7
            event_sequence = $_; fifo_sequence = 40; physics_tick = 41; dimension = 0
            correction_outcome = 'snapped'; corrected_tick = 41; replayed_ticks = 0
            correction_magnitude = 1.0
        }
    })
    (Invoke-Validator (Write-MarkerLog 'bound-events.log')).ExitCode | Should Not Be 0
}

It 'rejects hand-authored JSON without registered production marker prefixes' {
    $path = Join-Path $script:TempRoot 'plain.json'
    $script:Frames[0] | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath $path -Encoding utf8
    $result = Invoke-Validator $path
    $result.ExitCode | Should Not Be 0
}

It 'accepts a candidate verdict without a touch witness' {
    $script:Frames[2].input_mode = 'KeyboardMouse'
    $script:Frames[3].input_mode = 'GamePad'
    $result = Invoke-Validator (Write-MarkerLog 'deferred-touch-valid.log')
    $result.ExitCode | Should Be 0
    $aggregate = Get-Content -Raw -LiteralPath $result.Aggregate | ConvertFrom-Json
    $touch = @($aggregate.movement.input_witnesses | Where-Object input_mode -CEQ 'Touch')
    $touch.Count | Should Be 1
    $touch[0].acceptance_disposition | Should Be 'Deferred'
    $touch[0].observed | Should Be $false
    $touch[0].deferral_reason | Should Match 'Owner decision'
}

It 'keeps every non-Drained candidate terminal forbidden' {
    foreach ($terminalState in @(
        'SocketPending', 'BudgetDeferred', 'TransportRestored', 'FullRestored'
    )) {
        $script:Terminals[0].outbox_reconciliation = $terminalState
        (Invoke-Validator (Write-MarkerLog "terminal-$terminalState.log")).ExitCode |
            Should Not Be 0
    }
}

It 'rejects an indeterminate Physics send violation marker' {
    $script:Events += [ordered]@{
        schema = 'rust-mcbe-phase3-event-v1'; kind = 'authority_fault'
        session_generation = 7; next_tick = 41; pending_count = 1
        fault = 'indeterminate_physics_send'; detail = [ordered]@{ tick = 41 }
    }
    $script:Violations = @([ordered]@{
        schema = 'rust-mcbe-phase3-violation-v1'; reason = 'authority_fault'
    })
    (Invoke-Validator (Write-MarkerLog 'indeterminate-physics-send.log')).ExitCode |
        Should Not Be 0
}

It 'defines Zeno as an authenticated five-minute production-physics target' {
    $endpoint = Get-Phase3TargetEndpoint -Target Zeno
    $endpoint | Should Be 'zenomc.org:19197'
    $plan = New-Phase3LaunchPlan -Target Zeno -Endpoint $endpoint `
        -RunId $script:RunId -SocketDirectory socket -MetricsPath metrics.json `
        -DurationSeconds 300 -Scenario CandidatePhysics -AuthCache token.json
    ($plan.CoreArguments -ccontains '-auth-cache') | Should Be $true
    ($plan.AppArguments -ccontains '--phase3-candidate-physics') | Should Be $false
}
