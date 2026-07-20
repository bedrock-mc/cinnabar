[CmdletBinding()]
param(
    [ValidateRange(600, [int]::MaxValue)][int]$DurationSeconds = 900,
    [string]$AuthCache = '.local\auth\microsoft-token.json',
    [string]$Assets = '.local\assets\compiled\vanilla-v1001.mcbea',
    [string]$OutputDirectory,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$arguments = @{
    Target = 'Lbsg'
    Scenario = 'FastTransferWitness'
    DurationSeconds = $DurationSeconds
    AuthCache = $AuthCache
    Assets = $Assets
    DryRun = $DryRun
}
if (-not [string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $arguments.OutputDirectory = $OutputDirectory
}

& (Join-Path $PSScriptRoot 'Phase3Launcher.ps1') @arguments
