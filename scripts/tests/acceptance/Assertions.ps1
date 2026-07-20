$ErrorActionPreference = 'Stop'

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Equal {
    param($Expected, $Actual, [string]$Message)
    if ($Expected -cne $Actual) {
        throw "$Message`nexpected: $Expected`nactual:   $Actual"
    }
}

function Assert-Throws {
    param([scriptblock]$Action, [string]$Message)
    $threw = $false
    try {
        & $Action
    }
    catch {
        $threw = $true
    }
    Assert-True $threw $Message
}

function Assert-ThrowsLike {
    param(
        [scriptblock]$Action,
        [string]$Pattern,
        [string]$Message
    )
    $observed = $null
    try {
        & $Action
    }
    catch {
        $observed = $_.Exception.Message
    }
    Assert-True ($null -ne $observed) $Message
    Assert-True ($observed -like $Pattern) "$Message`nexpected error like: $Pattern`nactual error:        $observed"
}

