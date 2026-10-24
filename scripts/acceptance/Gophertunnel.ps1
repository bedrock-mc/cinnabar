function ConvertFrom-GophertunnelProvenanceJson {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Output,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $encoded = $Output -join [Environment]::NewLine
    if ([Text.Encoding]::UTF8.GetByteCount($encoded) -gt 65536) {
        throw "$Label gophertunnel output exceeds the 64 KiB provenance bound"
    }
    try { $document = $encoded | ConvertFrom-Json -ErrorAction Stop }
    catch { throw "$Label returned malformed gophertunnel JSON" }
    if ($null -eq $document -or $document -isnot [Management.Automation.PSCustomObject] -or
        -not $encoded.TrimStart().StartsWith('{')) {
        throw "$Label returned malformed gophertunnel JSON object"
    }
    # ConvertFrom-Json silently overwrites duplicate keys on PowerShell 5.1 and 7.
    # Tokenize strings first so braces inside values cannot alter object scopes.
    $scopes = [Collections.Generic.Stack[object]]::new()
    foreach ($token in [regex]::Matches($encoded, '"(?:[^"\\]|\\.)*"|[{}\[\]]')) {
        if ($token.Value -ceq '{') {
            $scopes.Push([Collections.Generic.HashSet[string]]::new([StringComparer]::Ordinal))
        }
        elseif ($token.Value -ceq '[') { $scopes.Push($null) }
        elseif ($token.Value -ceq '}' -or $token.Value -ceq ']') { $null = $scopes.Pop() }
        elseif ($encoded.Substring($token.Index + $token.Length).TrimStart().StartsWith(':')) {
            $key = $token.Value | ConvertFrom-Json -ErrorAction Stop
            if (-not $scopes.Peek().Add([string]$key)) {
                throw "$Label returned malformed gophertunnel JSON with a duplicate field"
            }
        }
    }
    return $document
}

function Get-GophertunnelProvenanceField {
    param($Object, [string]$Name)

    if ($null -ne $Object -and $Object -is [Management.Automation.PSCustomObject]) {
        $property = @($Object.PSObject.Properties | Where-Object Name -CEQ $Name)
        if ($property.Count -eq 1) { return ,$property[0].Value }
    }
    return $null
}

function Get-PinnedGophertunnelCommit {
    param([Parameter(Mandatory = $true)][string]$ProjectRoot)

    $previousGoWork = $env:GOWORK
    $env:GOWORK = 'off'
    try {
        $sourceOutput = @(& go -C (Join-Path $ProjectRoot 'tools/localserver') mod edit -json 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "go mod edit failed while reading tools/localserver/go.mod gophertunnel pin: $($sourceOutput -join [Environment]::NewLine)"
        }
        $source = ConvertFrom-GophertunnelProvenanceJson -Output $sourceOutput -Label 'go mod edit'
        $replacements = Get-GophertunnelProvenanceField $source 'Replace'
        $matching = @($replacements | Where-Object {
            (Get-GophertunnelProvenanceField (Get-GophertunnelProvenanceField $_ 'Old') 'Path') -ceq 'github.com/sandertv/gophertunnel'
        })
        if ($replacements -isnot [array] -or $matching.Count -ne 1) {
            throw 'tools/localserver/go.mod must declare exactly one unversioned gophertunnel replacement'
        }
        $old = Get-GophertunnelProvenanceField $matching[0] 'Old'
        $oldPath = Get-GophertunnelProvenanceField $old 'Path'
        $oldVersion = Get-GophertunnelProvenanceField $old 'Version'
        $new = Get-GophertunnelProvenanceField $matching[0] 'New'
        $expectedPath = Get-GophertunnelProvenanceField $new 'Path'
        $expectedVersion = Get-GophertunnelProvenanceField $new 'Version'
        if ($oldPath -isnot [string] -or
            ($null -ne $old.PSObject.Properties['Version'] -and ($oldVersion -isnot [string] -or $oldVersion -cne '')) -or
            $expectedPath -isnot [string] -or $expectedPath -cne 'github.com/hashimthearab/gophertunnel' -or
            $expectedVersion -isnot [string] -or
            $expectedVersion -cnotmatch '^v(?:0|[1-9][0-9]*)\.(?<minor>0|[1-9][0-9]*)\.(?<patch>0|[1-9][0-9]*)-(?:(?<prefix>[0-9A-Za-z.-]+)\.)?(?<timestamp>[0-9]{14})-(?<revision>[0-9a-f]{12})(?:\+incompatible)?\z') {
            throw 'tools/localserver/go.mod must pin the canonical gophertunnel fork with an unversioned pseudo-version replacement'
        }
        $revision = [string]$Matches.revision
        $prefix = [string]$Matches['prefix']
        $timestampText = [string]$Matches.timestamp
        $timestamp = [datetime]::MinValue
        $validPrefix = ($prefix -ceq '' -and $Matches.minor -ceq '0' -and $Matches.patch -ceq '0') -or
            ($prefix -ceq '0' -and $Matches.patch -cne '0')
        if ($prefix -cmatch '^(.+)\.0$') {
            $validPrefix = $true
            foreach ($identifier in $Matches[1].Split('.')) {
                if ($identifier -cnotmatch '^[0-9A-Za-z-]+$' -or $identifier -cmatch '^0[0-9]+$') {
                    $validPrefix = $false
                }
            }
        }
        if (-not $validPrefix -or -not [datetime]::TryParseExact($timestampText, 'yyyyMMddHHmmss',
            [Globalization.CultureInfo]::InvariantCulture, [Globalization.DateTimeStyles]::None, [ref]$timestamp)) {
            throw 'tools/localserver/go.mod gophertunnel replacement is not a valid pinned pseudo-version'
        }

        $output = @(& go -C (Join-Path $ProjectRoot 'tools/localserver') list -m -json github.com/sandertv/gophertunnel 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "go list -m failed while resolving gophertunnel: $($output -join [Environment]::NewLine)"
        }
        $module = ConvertFrom-GophertunnelProvenanceJson -Output $output -Label 'go list -m'
        $replacement = Get-GophertunnelProvenanceField $module 'Replace'
        $modulePath = Get-GophertunnelProvenanceField $module 'Path'
        $replacementPath = Get-GophertunnelProvenanceField $replacement 'Path'
        $replacementVersion = Get-GophertunnelProvenanceField $replacement 'Version'
        if ($modulePath -isnot [string] -or $modulePath -cne 'github.com/sandertv/gophertunnel' -or
            $replacementPath -isnot [string] -or $replacementPath -cne $expectedPath -or
            $replacementVersion -isnot [string] -or $replacementVersion -cne $expectedVersion) {
            throw 'go list -m resolved a different gophertunnel module or replacement version than tools/localserver/go.mod'
        }
        $replacementQuery = '{0}@{1}' -f $expectedPath, $expectedVersion
        $downloadOutput = @(& go -C (Join-Path $ProjectRoot 'tools/localserver') mod download -json $replacementQuery 2>&1)
        if ($LASTEXITCODE -ne 0) {
            throw "go mod download failed while verifying gophertunnel origin: $($downloadOutput -join [Environment]::NewLine)"
        }
        $download = ConvertFrom-GophertunnelProvenanceJson -Output $downloadOutput -Label 'go mod download'
        $origin = Get-GophertunnelProvenanceField $download 'Origin'
        $downloadPath = Get-GophertunnelProvenanceField $download 'Path'
        $downloadVersion = Get-GophertunnelProvenanceField $download 'Version'
        $originVcs = Get-GophertunnelProvenanceField $origin 'VCS'
        $originUrl = Get-GophertunnelProvenanceField $origin 'URL'
        $commit = Get-GophertunnelProvenanceField $origin 'Hash'
        if ($downloadPath -isnot [string] -or $downloadPath -cne $expectedPath -or
            $downloadVersion -isnot [string] -or $downloadVersion -cne $expectedVersion -or
            $originVcs -isnot [string] -or $originVcs -cne 'git' -or
            $originUrl -isnot [string] -or $originUrl -cne 'https://github.com/hashimthearab/gophertunnel' -or
            $commit -isnot [string] -or $commit -cnotmatch '^[0-9a-f]{40}\z' -or
            $commit.Substring(0, 12) -cne $revision) {
            throw 'resolved gophertunnel module origin does not match the expected exact commit'
        }
        return $commit
    }
    finally { $env:GOWORK = $previousGoWork }
}
