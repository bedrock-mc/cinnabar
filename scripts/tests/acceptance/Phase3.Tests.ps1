Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Describe 'Phase 3 deterministic evidence validation' {
    BeforeAll {
        $script:RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\..'))
        $script:Validator = Join-Path $script:RepoRoot 'scripts\acceptance\Phase3.ps1'
        $script:TempRoot = Join-Path ([IO.Path]::GetTempPath()) "rust-mcbe-phase3-$PID-$([guid]::NewGuid().ToString('N'))"
        New-Item -ItemType Directory -Path $script:TempRoot -Force | Out-Null
        $script:BuildCommit = (& git -C $script:RepoRoot rev-parse HEAD).Trim()
        $script:PregSha = (Get-Content -Raw -LiteralPath (Join-Path $script:RepoRoot 'crates\assets\data\block-physics-v1001.sha256')).Trim()
        $script:BregSha = (Get-FileHash -Algorithm SHA256 -LiteralPath (Join-Path $script:RepoRoot 'crates\assets\data\block-registry-v1001.bin')).Hash.ToLowerInvariant()
    }

    AfterAll {
        if (Test-Path -LiteralPath $script:TempRoot) {
            Remove-Item -LiteralPath $script:TempRoot -Recurse -Force
        }
    }

    BeforeEach {
        $script:Evidence = [ordered]@{
            schema_version = 1
            build_commit = $script:BuildCommit
            target = 'Bds'
            endpoint = '127.0.0.1:19132'
            session_generation = 7
            preg_sha256 = $script:PregSha
            breg_sha256 = $script:BregSha
            input_mode = 'KeyboardMouse'
            tick_range = [ordered]@{ first = 41; last = 43; count = 3 }
            corrections = [ordered]@{ replay_count = 1; snap_count = 0; maximum_magnitude = 0.125 }
            outbox = [ordered]@{ high_water = 2; drops = 0 }
            free_camera_packet_count = 0
            held_jump = [ordered]@{ landings = 2; rejumps = 2; passed = $true }
            perspective_cycle = @('FirstPerson', 'ThirdPersonBack', 'ThirdPersonFront', 'FirstPerson')
            camera = [ordered]@{ blocked_count = 1; fallback_count = 0 }
            local_avatar = [ordered]@{ first_person_visible = 0; third_person_back_visible = 1; third_person_front_visible = 1 }
            frame_rate = [ordered]@{ samples = @(59.5, 60.0, 60.5) }
            process = [ordered]@{ exit_code = 0; timed_out = $false; peak_private_bytes = 268435456 }
            resources = [ordered]@{ bounded = $true; queue_drops = 0 }
            events = @([ordered]@{ kind = 'authority'; tick = 41 })
        }
    }

    function Write-Evidence {
        param([string]$Name)
        $path = Join-Path $script:TempRoot $Name
        $script:Evidence | ConvertTo-Json -Depth 12 | Set-Content -LiteralPath $path -Encoding utf8
        return $path
    }

    function Invoke-Validator {
        param([string]$Path)
        $savedErrorActionPreference = $ErrorActionPreference
        try {
            $ErrorActionPreference = 'Continue'
            $output = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $script:Validator `
                -EvidencePath $Path -ExpectedTarget Bds 2>&1 | Out-String
        }
        finally {
            $ErrorActionPreference = $savedErrorActionPreference
        }
        return [pscustomobject]@{ ExitCode = $LASTEXITCODE; Output = $output }
    }

    It 'accepts one exact bounded evidence record' {
        $result = Invoke-Validator (Write-Evidence 'valid.json')
        $result.ExitCode | Should Be 0
        $result.Output | Should Match 'PHASE3_EVIDENCE_VALID'
    }

    It 'rejects free-camera packets and queue drops' {
        $script:Evidence.free_camera_packet_count = 1
        $script:Evidence.outbox.drops = 1
        $result = Invoke-Validator (Write-Evidence 'packets.json')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects a stale target or registry identity' {
        $script:Evidence.target = 'Zeqa'
        $script:Evidence.preg_sha256 = '0' * 64
        $result = Invoke-Validator (Write-Evidence 'identity.json')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects tick gaps non-finite metrics and unbounded records' {
        $script:Evidence.tick_range.count = 2
        $script:Evidence.corrections.maximum_magnitude = 'NaN'
        $script:Evidence.events = @(0..256 | ForEach-Object { [ordered]@{ kind = 'tick'; tick = $_ } })
        $result = Invoke-Validator (Write-Evidence 'bounds.json')
        $result.ExitCode | Should Not Be 0
    }

    It 'rejects missing required fields and failed process outcomes' {
        $script:Evidence.Remove('held_jump')
        $script:Evidence.process.exit_code = 1
        $result = Invoke-Validator (Write-Evidence 'missing.json')
        $result.ExitCode | Should Not Be 0
    }
}
