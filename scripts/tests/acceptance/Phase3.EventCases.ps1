    It 'rejects an event session mismatch as the only changed condition' {
        $script:Events[0].session_generation = 8
        (Invoke-Validator (Write-MarkerLog 'event-session.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event FIFO mismatch as the only changed condition' {
        $script:Events[0].fifo_sequence = 99
        (Invoke-Validator (Write-MarkerLog 'event-fifo.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event physics-tick mismatch as the only changed condition' {
        $script:Events[0].physics_tick = 99
        (Invoke-Validator (Write-MarkerLog 'event-tick.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an event dimension mismatch as the only changed condition' {
        $script:Events[0].dimension = 1
        (Invoke-Validator (Write-MarkerLog 'event-dimension.log')).ExitCode | Should Not Be 0
    }

    It 'rejects an out-of-range movement vector as the only changed condition' {
        $script:Frames[0].movement = @(2.0, 0.0)
        (Invoke-Validator (Write-MarkerLog 'bound-movement.log')).ExitCode | Should Not Be 0
    }