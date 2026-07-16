function Get-AcceptanceLibraryPaths {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $root = Join-Path (Split-Path -Parent $EntryPath) 'acceptance'
    return @(
        'Common.ps1',
        'RuntimePaths.ps1',
        'Process.ps1',
        'Bds.ps1',
        'Markers.ps1',
        'Galleries\Common.ps1',
        'Galleries\Leaves.ps1',
        'Galleries\CrossCrop.ps1',
        'Galleries\Aquatic.ps1',
        'Galleries\Water.ps1',
        'Galleries\FlowerBed.ps1',
        'Galleries\SlabStair.ps1',
        'Galleries\Vine.ps1',
        'Proofs.ps1',
        'Resources.ps1',
        'Metrics.ps1',
        'Orchestration\Validate.ps1',
        'Orchestration\Execute.ps1',
        'Orchestrator.ps1'
    ) | ForEach-Object { Join-Path $root $_ }
}

function Get-AcceptanceCompositeSource {
    param([Parameter(Mandatory = $true)][string]$EntryPath)

    $parts = @([IO.File]::ReadAllText($EntryPath))
    $parts += @(
        Get-AcceptanceLibraryPaths -EntryPath $EntryPath |
            ForEach-Object { [IO.File]::ReadAllText($_) }
    )
    return $parts -join "`n"
}
