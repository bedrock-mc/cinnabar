# Extract Burn's package container without installing; require exactly the checked MSI.
param([Parameter(Mandatory)][string]$Setup, [Parameter(Mandatory)][string]$Msi,
      [Parameter(Mandatory)][string]$Out, [string]$Wix = "wix")
$ErrorActionPreference = "Stop"
if (Test-Path -LiteralPath $Out) { throw "Setup extraction output already exists: $Out" }
& $Wix burn extract $Setup -o $Out
if ($LASTEXITCODE -ne 0) { throw "Setup extraction failed" }
$files = @(Get-ChildItem -LiteralPath $Out -Recurse -File)
if ($files.Count -ne 1) { throw "Setup must embed exactly one MSI package" }
if ((Get-FileHash -LiteralPath $files[0].FullName -Algorithm SHA256).Hash -ne
    (Get-FileHash -LiteralPath $Msi -Algorithm SHA256).Hash) { throw "Setup contains a different MSI" }
Write-Output "Setup embeds only the verified MSI"
