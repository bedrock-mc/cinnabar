# Dependency-free tests: a fake WiX exports File-table fixtures, then the real strict
# payload checker verifies that reconstruction retains every installed file.
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot "../.."))
$helper = Join-Path $repo "packaging/windows/extract-msi.ps1"
$testRoot = Join-Path ([IO.Path]::GetTempPath()) ("cinnabar-msi-tests-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $testRoot | Out-Null
try {
    $fakeWix = Join-Path $testRoot "fake-wix.ps1"
    @'
param(
    [Parameter(Position=0)][string]$Command,
    [Parameter(Position=1)][string]$Subcommand,
    [Parameter(Position=2)][string]$InputMsi,
    [Alias("x")][string]$Exports,
    [Alias("o")][string]$Authoring,
    [string]$intermediateFolder
)
$ErrorActionPreference = "Stop"
$fixture = Get-Content -LiteralPath $InputMsi -Raw | ConvertFrom-Json
if ($fixture.Mode -eq "fail") { exit 17 }
if ($Command -ne "msi" -or $Subcommand -ne "decompile") { throw "Wrong WiX command" }
New-Item -ItemType Directory -Path (Join-Path $Exports "File") -Force | Out-Null
New-Item -ItemType Directory -Path (Join-Path $Exports "Icon") -Force | Out-Null
Set-Content -LiteralPath (Join-Path $Exports "Icon/installer-icon") -Value "not an installed File"
$ns = "http://wixtoolset.org/schemas/v4/wxs"
$xml = [Xml.XmlDocument]::new()
$wix = $xml.CreateElement("Wix", $ns)
$xml.AppendChild($wix) | Out-Null
$package = $xml.CreateElement("Package", $ns)
$wix.AppendChild($package) | Out-Null
$standard = $xml.CreateElement("StandardDirectory", $ns)
$standard.SetAttribute("Id", "ProgramFiles64Folder")
$package.AppendChild($standard) | Out-Null
$install = $xml.CreateElement("Directory", $ns)
$install.SetAttribute("Id", "INSTALLFOLDER")
$install.SetAttribute("Name", "Cinnabar")
$standard.AppendChild($install) | Out-Null
if ($fixture.Mode -eq "same-directory") {
    $same = $xml.CreateElement("Directory", $ns)
    $same.SetAttribute("Id", "SAME_DIRECTORY")
    $install.AppendChild($same) | Out-Null
    $install = $same
}
$directories = @{ "" = $install }
foreach ($entry in $fixture.Files) {
    $parts = $entry.Path.Split("/")
    $parent = $install
    $key = ""
    for ($i = 0; $i -lt $parts.Count - 1; $i++) {
        $key += "/" + $parts[$i]
        if (-not $directories.ContainsKey($key)) {
            $directory = $xml.CreateElement("Directory", $ns)
            $directory.SetAttribute("Id", "dir" + $directories.Count)
            $directory.SetAttribute("Name", $parts[$i])
            $parent.AppendChild($directory) | Out-Null
            $directories[$key] = $directory
        }
        $parent = $directories[$key]
    }
    $component = $xml.CreateElement("Component", $ns)
    $component.SetAttribute("Id", "component" + $entry.Id)
    $parent.AppendChild($component) | Out-Null
    $file = $xml.CreateElement("File", $ns)
    $file.SetAttribute("Id", $entry.Id)
    $file.SetAttribute("Name", $parts[-1])
    $file.SetAttribute("Source", "SourceDir\File\" + $entry.Id)
    $component.AppendChild($file) | Out-Null
    if ($fixture.Mode -ne "missing") {
        [IO.File]::WriteAllText((Join-Path $Exports ("File/" + $entry.Id)), $entry.Content)
    }
}
if ($fixture.Mode -eq "unmapped") {
    Set-Content -LiteralPath (Join-Path $Exports "File/not_in_file_table") -Value "unexpected"
}
$xml.Save($Authoring)
exit 0
'@ | Set-Content -LiteralPath $fakeWix

    function Invoke-Fixture([string]$Name, [array]$Files, [string]$Mode = "ok", [string]$ExpectedError = "") {
        $msi = Join-Path $testRoot "$Name.msi"
        $out = Join-Path $testRoot "$Name-image"
        @{ Mode = $Mode; Files = $Files } | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $msi
        $hash = (Get-FileHash -LiteralPath $msi).Hash
        $failure = $null
        try { & $helper -Msi $msi -Out $out -Wix $fakeWix | Out-Host } catch { $failure = $_ }
        if ($ExpectedError) {
            if ($null -eq $failure -or $failure.Exception.Message -notlike "*$ExpectedError*") {
                throw "$Name did not fail with '$ExpectedError': $failure"
            }
            if (Test-Path -LiteralPath $out) { throw "$Name published a failed extraction" }
        } elseif ($null -ne $failure) { throw $failure }
        if ((Get-FileHash -LiteralPath $msi).Hash -ne $hash) { throw "$Name modified the MSI" }
        if (@(Get-ChildItem -LiteralPath $testRoot -Filter ".cinnabar-msi-*").Count -ne 0) {
            throw "$Name leaked extraction work"
        }
        Write-Host "PASS extract-msi $Name"
        return $out
    }
    function ConvertTo-BashPath([string]$Path) {
        if ([Environment]::OSVersion.Platform -eq [PlatformID]::Win32NT) {
            $converted = & bash -c 'cygpath -u "$1"' -- $Path
            if ($LASTEXITCODE -ne 0) { throw "Could not convert Git Bash path: $Path" }
            return $converted
        }
        return $Path
    }
    $manifest = ConvertTo-BashPath (Join-Path $repo "packaging/common/stage-payload.sh")
    $checker = ConvertTo-BashPath (Join-Path $repo "packaging/check-payload.sh")
    $resources = @(& bash -c 'source "$1"; resource_manifest windows' -- $manifest)
    if ($LASTEXITCODE -ne 0) { throw "Could not load actual Windows resource manifest" }
    $paths = @("bedrock-client.exe", "bedrock-core.exe", "bedrock-local-server.exe") + @($resources | ForEach-Object { "resources/$_" })
    $payload = @()
    for ($i = 0; $i -lt $paths.Count; $i++) {
        $payload += @{ Id = "file$i"; Path = $paths[$i]; Content = "payload $i" }
    }
    $valid = Invoke-Fixture "complete-payload" $payload
    & bash $checker windows (ConvertTo-BashPath $valid)
    if ($LASTEXITCODE -ne 0) { throw "Strict checker rejected the complete extracted payload" }
    $unexpected = Invoke-Fixture "unexpected-installed-file" ($payload + @(@{ Id = "extra"; Path = "unreviewed.bin"; Content = "must remain visible" }))
    $checkerOutput = @(& bash $checker windows (ConvertTo-BashPath $unexpected) 2>&1)
    if ($LASTEXITCODE -eq 0 -or ($checkerOutput -join "`n") -notlike "*unexpected file*unreviewed.bin*") {
        throw "Strict checker did not reject retained unexpected installed file: $checkerOutput"
    }
    Write-Host "PASS strict payload rejection after extraction"
    $longNames = @(
        @{ Id = "long1"; Path = "folder with spaces/long file name.txt"; Content = "first" },
        @{ Id = "long2"; Path = "nested/long file name.txt"; Content = "second" }
    )
    $long = Invoke-Fixture "installed-long-names" $longNames
    foreach ($file in $longNames) {
        $target = Join-Path $long ("ProgramFiles64Folder/Cinnabar/" + $file.Path)
        if ([IO.File]::ReadAllText($target) -ne $file.Content) { throw "Installed path/content mismatch: $target" }
    }
    $sameDirectory = Invoke-Fixture "unnamed-same-directory" $longNames "same-directory"
    foreach ($file in $longNames) {
        $target = Join-Path $sameDirectory ("ProgramFiles64Folder/Cinnabar/" + $file.Path)
        if ([IO.File]::ReadAllText($target) -ne $file.Content) { throw "Unnamed directory changed installed path" }
    }
    Invoke-Fixture "traversal-directory" @(@{ Id = "file"; Path = "../escape.txt"; Content = "bad" }) "ok" "Unsafe directory name" | Out-Null
    Invoke-Fixture "unsafe-file-name" @(@{ Id = "file"; Path = "bad:name.txt"; Content = "bad" }) "ok" "Unsafe file name" | Out-Null
    Invoke-Fixture "case-collision" @(
        @{ Id = "first"; Path = "same.txt"; Content = "first" },
        @{ Id = "second"; Path = "SAME.txt"; Content = "second" }
    ) "ok" "Colliding installed MSI path" | Out-Null
    Invoke-Fixture "missing-cabinet-file" $longNames "missing" "Missing cabinet payload" | Out-Null
    Invoke-Fixture "unmapped-cabinet-file" $longNames "unmapped" "Unmapped cabinet payload" | Out-Null
    Invoke-Fixture "decompiler-failure" $longNames "fail" "exit code 17" | Out-Null
    Write-Host "All MSI extraction tests passed"
} finally {
    Remove-Item -LiteralPath $testRoot -Recurse -Force
}

# Expected native failures leave LASTEXITCODE nonzero. GitHub's pwsh wrapper
# propagates it, so report success only after every assertion and cleanup passed.
exit 0
