# Run both standalone and as part of the hermetic acquisition suite.
. (Join-Path $PSScriptRoot '../vanilla-assets-helpers.ps1')

$escape = [char]27
foreach ($rendered in @(
    'compression ratio exceeds the per-entry maximum 500',
    "compression ratio exceeds the per-entry maxi`nmum 500",
    "     | compression ratio exceeds the`n     | per-entry maximum 500",
    "${escape}[31;1m     | compression ratio exceeds the${escape}[0m`n${escape}[31;1m     | per-entry maximum 500${escape}[0m"
)) {
    if (-not (Test-OutputContains -Output $rendered -Needle 'exceeds the per-entry maximum 500')) {
        throw 'diagnostic matching lost a plain, wrapped, guttered, or ANSI-styled error'
    }
}
foreach ($different in @(
    'exceeds the per-entry maximum 100',
    'exceeds the aggregate maximum 500',
    'exceeds the | per-entry maximum 500'
)) {
    if (Test-OutputContains -Output $different -Needle 'exceeds the per-entry maximum 500') {
        throw 'diagnostic matching accepted a different error'
    }
}
