param(
    [Parameter(Mandatory = $true, Position = 0)]
    [string]$Report,
    [Parameter(Mandatory = $true, Position = 1)]
    [string]$Carrier
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

$reportItem = Get-Item -LiteralPath $Report -ErrorAction SilentlyContinue
$carrierItem = Get-Item -LiteralPath $Carrier
if ($null -ne $reportItem -and $reportItem.LastWriteTimeUtc -ge $carrierItem.LastWriteTimeUtc) {
    exit 0
}
exit 1
