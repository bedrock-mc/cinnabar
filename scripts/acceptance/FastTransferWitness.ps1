[CmdletBinding()]
param(
    [ValidateRange(600, [int]::MaxValue)][int]$DurationSeconds = 900,
    [string]$Assets,
    [string]$OutputDirectory,
    [switch]$DryRun
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$arguments = @{
    Target = 'Lbsg'
    Scenario = 'FastTransferWitness'
    DurationSeconds = $DurationSeconds
    DryRun = $DryRun
}
if (-not [string]::IsNullOrWhiteSpace($Assets)) {
    $arguments.Assets = $Assets
}
if (-not [string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $arguments.OutputDirectory = $OutputDirectory
}

& (Join-Path $PSScriptRoot 'MovementLauncher.ps1') @arguments
