# Embeds or checks the executable manifest before signing. Requires the Windows SDK mt.exe.
param([Parameter(Mandatory)][string[]]$Executable, [switch]$Embed, [string]$Mt = "")
$ErrorActionPreference = "Stop"
if (-not $Mt) {
    $tool = Get-Command mt.exe -ErrorAction SilentlyContinue
    if ($tool) { $Mt = $tool.Source } else {
        $sdk = Join-Path ${env:ProgramFiles(x86)} "Windows Kits/10/bin"
        $tool = Get-ChildItem -LiteralPath $sdk -Filter mt.exe -Recurse |
            Where-Object { $_.Directory.Name -eq "x64" } |
            Sort-Object FullName -Descending | Select-Object -First 1
        if (-not $tool) { throw "Windows SDK mt.exe is required" }
        $Mt = $tool.FullName
    }
}
foreach ($file in $Executable) {
    $exe = (Resolve-Path -LiteralPath $file).Path
    $manifest = [IO.Path]::GetTempFileName()
    try {
        if ($Embed) {
            # Keep existing compatibility and DPI declarations; add only the permission contract.
            & $Mt -nologo "-inputresource:$exe;#1" "-out:$manifest" 2>$null | Out-Null
            $existing = $LASTEXITCODE -eq 0
            $arguments = @("-nologo", "-manifest", (Join-Path $PSScriptRoot "as-invoker.manifest"))
            if ($existing) { $arguments += $manifest }
            & $Mt @arguments "-outputresource:$exe;#1" | Out-Null
            if ($LASTEXITCODE -ne 0) { throw "Cannot embed asInvoker manifest: $exe" }
        }
        & $Mt -nologo "-inputresource:$exe;#1" "-out:$manifest" | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "Missing executable manifest: $exe" }
        [xml]$xml = Get-Content -LiteralPath $manifest -Raw
        $namespaces = [Xml.XmlNamespaceManager]::new($xml.NameTable)
        $namespaces.AddNamespace("a", "urn:schemas-microsoft-com:asm.v3")
        $levels = @($xml.SelectNodes("/*/a:trustInfo/a:security/a:requestedPrivileges/a:requestedExecutionLevel", $namespaces))
        if ($levels.Count -ne 1 -or $levels[0].level -ne "asInvoker" -or $levels[0].uiAccess -ne "false") {
            throw "Executable must request asInvoker with uiAccess=false: $exe"
        }
    } finally {
        Remove-Item -LiteralPath $manifest -Force
    }
}
