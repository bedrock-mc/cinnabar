# Directory-entry parity regressions run in the caller's hermetic asset
# sandbox. Explicit directory members must be unique, while repeated implicit
# ancestors shared by distinct files remain valid.

Reset-BoundedFixture
New-TestZipArchive -Path $syntheticArchivePath -Entries @(
    [pscustomobject]@{ Name = "resource_pack/"; Content = $null },
    [pscustomobject]@{ Name = "resource_pack/"; Content = $null },
    [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" }
)
Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
Assert-ExtractionRejection -Label "duplicate-explicit-directory" `
    -Needle "duplicate ZIP entry path 'resource_pack'"

Reset-BoundedFixture
New-TestZipArchive -Path $syntheticArchivePath -Entries @(
    [pscustomobject]@{ Name = "resource_pack/blocks.json"; Content = "{}" },
    [pscustomobject]@{ Name = "resource_pack/textures/one.txt"; Content = "one" },
    [pscustomobject]@{ Name = "resource_pack/textures/two.txt"; Content = "two" }
)
Write-BoundedCaseManifest -Sha (Get-TestSha256Hex -Path $syntheticArchivePath)
if ($bashDeepTools) {
    $implicitAncestorResult = Invoke-NativeCapture -FilePath $bash -ArgumentList @(
        $sandboxBashFetcher, "--accept-eula")
    if ($implicitAncestorResult.ExitCode -ne 0) {
        $sandboxFailures += "implicit-ancestor-repeat(bash): valid archive failed: $($implicitAncestorResult.Output.Trim())"
    }
}
