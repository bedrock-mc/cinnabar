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
