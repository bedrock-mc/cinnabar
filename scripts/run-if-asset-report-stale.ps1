param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Report,
    [Parameter(Mandatory = $true, Position = 1)]
    [string]$Carrier,
    [Parameter(Mandatory = $true, Position = 2, ValueFromRemainingArguments = $true)]
    [string[]]$Command
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$reportItem = Get-Item -LiteralPath $Report -ErrorAction SilentlyContinue
$carrierItem = Get-Item -LiteralPath $Carrier
if ($null -ne $reportItem -and $reportItem.LastWriteTimeUtc -ge $carrierItem.LastWriteTimeUtc) {
    exit 0
}

if ($Command.Count -eq 0) {
    throw "asset report recovery command is empty"
}

$executable = $Command[0]
$arguments = if ($Command.Count -gt 1) { $Command[1..($Command.Count - 1)] } else { @() }
& $executable @arguments
if ($LASTEXITCODE -ne 0) {
    exit $LASTEXITCODE
}
