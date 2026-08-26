# PowerShell publication regressions run after the Bash injected race so both
# platforms prove direct no-nesting/no-replace directory rename semantics.

if ($bashDeepTools) {
    $renameHelperTest = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
        (Join-Path $repoRoot 'scripts\tests\rename-directory-no-replace_test.sh'),
        (Join-Path $repoRoot 'scripts\rename-directory-no-replace.c')
    )
    if ($renameHelperTest.ExitCode -ne 0) {
        $sandboxFailures += "atomic-publisher-helper(bash): $($renameHelperTest.Output.Trim())"
    }
}

$sandboxFailures += Test-PowerShellDirectoryMoveRace -Root $sandboxRoot
$powerShellFetcherText = Get-Content -Raw -LiteralPath $sandboxPowerShellFetcher
if (-not $powerShellFetcherText.Contains('[System.IO.Directory]::Move(')) {
    $sandboxFailures += 'publish-race(PowerShell): fetcher does not use direct Directory.Move publication'
}
if ($powerShellFetcherText -match 'Move-Item\s+-LiteralPath\s+\$normalizedRoot') {
    $sandboxFailures += 'publish-race(PowerShell): fetcher retained directory-destination Move-Item publication'
}
