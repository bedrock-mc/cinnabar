# Pester contracts for scripts/organic-movement-input-driver.ps1.
#
# These tests are strictly offline: every invocation either uses -DryRun (which
# must never touch any native injection entry point) or exercises a live-path
# refusal against a guaranteed-nonexistent process, which aborts cleanly before
# any window interaction or injection. No game is launched and no input is ever
# injected by this file.

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..'))
$DriverPath = Join-Path $RepoRoot 'scripts\organic-movement-input-driver.ps1'

function Assert-True([bool]$Condition, [string]$Message) {
    if (-not $Condition) { throw $Message }
}

function Assert-Equal($Expected, $Actual, [string]$Message) {
    if ($Expected -cne $Actual) {
        throw ('{0} (expected: {1}; actual: {2})' -f $Message, $Expected, $Actual)
    }
}

function Invoke-DriverChild([string[]]$Arguments) {
    # Child stderr (for example parameter-binding failures) arrives through
    # 2>&1; keep Continue active during the redirection so nonempty stderr does
    # not become a terminating NativeCommandError in this parent session.
    $previousPreference = $ErrorActionPreference
    $ErrorActionPreference = 'Continue'
    try {
        $lines = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $DriverPath @Arguments 2>&1
    }
    finally {
        $ErrorActionPreference = $previousPreference
    }
    return [pscustomobject]@{
        ExitCode = $LASTEXITCODE
        Output   = @($lines | ForEach-Object { $_.ToString() })
    }
}

function Get-DriverParseResult {
    $tokens = $null
    $errors = $null
    $ast = [System.Management.Automation.Language.Parser]::ParseFile(
        $DriverPath, [ref]$tokens, [ref]$errors)
    return @{ Ast = $ast; Errors = @($errors) }
}

function Find-DriverParameter($Ast, [string]$Name) {
    foreach ($parameter in @($Ast.ParamBlock.Parameters)) {
        if ([string]$parameter.Name.VariablePath.UserPath -ieq $Name) { return $parameter }
    }
    throw ("driver parameter '{0}' was not found" -f $Name)
}

function Get-DriverValidateRange($Ast, [string]$Name) {
    $parameter = Find-DriverParameter $Ast $Name
    foreach ($attribute in @($parameter.Attributes)) {
        if ([string]$attribute.TypeName.FullName -ieq 'ValidateRange') {
            $lower = $attribute.PositionalArguments[0]
            $upper = $attribute.PositionalArguments[1]
            if ($lower -isnot [System.Management.Automation.Language.ConstantExpressionAst] -or
                $upper -isnot [System.Management.Automation.Language.ConstantExpressionAst]) {
                throw ("driver parameter '{0}' must use literal ValidateRange bounds" -f $Name)
            }
            return @([int]$lower.Value, [int]$upper.Value)
        }
    }
    throw ("driver parameter '{0}' lacks a ValidateRange attribute" -f $Name)
}

function Assert-DriverHasKeyBalance([object[]]$Events) {
    $held = @{}
    foreach ($step in $Events) {
        $scan = [int]$step.scan_code
        if ([string]$step.action -ceq 'KeyDown') {
            $state = 0
            if ($held.ContainsKey($scan)) { $state = $held[$scan] }
            if ($state -ne 0) {
                throw ('nested KeyDown without KeyUp for scan 0x{0:x2}' -f $scan)
            }
            $held[$scan] = 1
        }
        elseif ([string]$step.action -ceq 'KeyUp') {
            if (-not $held.ContainsKey($scan) -or [int]$held[$scan] -ne 1) {
                throw ('KeyUp without a held KeyDown for scan 0x{0:x2}' -f $scan)
            }
            $held[$scan] = 0
        }
    }
    foreach ($scan in $held.Keys) {
        if ([int]$held[$scan] -ne 0) {
            throw ('event stream ends with an unreleased key scan 0x{0:x2}' -f $scan)
        }
    }
}

Describe 'organic movement synthetic input driver' {

    BeforeAll {
        $script:Scratch = Join-Path (Join-Path ([IO.Path]::GetTempPath()) (
            'rust-mcbe-organic-input-' + [guid]::NewGuid().ToString('N'))) 'scratch'
        New-Item -ItemType Directory -Path $script:Scratch -Force | Out-Null

        # One shared deterministic default dry run feeds the mapping and
        # determinism contracts below.
        $script:DefaultPlanPath = Join-Path $script:Scratch 'default-plan.json'
        $script:DefaultRun = Invoke-DriverChild @(
            '-DryRun', '-DurationSeconds', '45', '-OutputPlanPath', $script:DefaultPlanPath)
        $script:DefaultPlan = Get-Content -Raw -LiteralPath $script:DefaultPlanPath | ConvertFrom-Json
        $script:DefaultEvents = @($script:DefaultPlan.events)
    }

    AfterAll {
        $root = Split-Path -Parent $script:Scratch
        if (Test-Path -LiteralPath $root) {
            Remove-Item -LiteralPath $root -Recurse -Force
        }
    }

    Context 'static script schema' {

        It 'parses cleanly with zero syntax errors' {
            $parsed = Get-DriverParseResult
            Assert-Equal 0 $parsed.Errors.Count 'driver must parse without syntax errors'
        }

        It 'exposes the required parameter schema' {
            $parsed = Get-DriverParseResult
            foreach ($name in @(
                'ProcessName', 'WindowTitle', 'DryRun', 'DurationSeconds', 'GraceSeconds',
                'Activate', 'Composition', 'OneShot', 'WalkMilliseconds', 'TapJumpMilliseconds',
                'SneakMilliseconds', 'LookDriftPixels', 'F5TapCount', 'F5GapMilliseconds',
                'OutputPlanPath'
            )) {
                Find-DriverParameter $parsed.Ast $name | Out-Null
            }
        }

        It 'bounds every duration and magnitude parameter' {
            $parsed = Get-DriverParseResult
            $expectedRanges = @{
                'DurationSeconds'      = @(5, 120)
                'GraceSeconds'         = @(1, 30)
                'WalkMilliseconds'     = @(200, 10000)
                'TapJumpMilliseconds'  = @(20, 1000)
                'SneakMilliseconds'    = @(100, 5000)
                'LookDriftPixels'      = @(1, 64)
                'F5TapCount'           = @(1, 6)
                'F5GapMilliseconds'    = @(200, 2000)
            }
            foreach ($name in $expectedRanges.Keys) {
                $range = Get-DriverValidateRange $parsed.Ast $name
                Assert-Equal $expectedRanges[$name][0] $range[0] "$name lower bound"
                Assert-Equal $expectedRanges[$name][1] $range[1] "$name upper bound"
            }
        }

        It 'types DryRun as a switch and Composition as validated free text' {
            $parsed = Get-DriverParseResult
            $dryRun = Find-DriverParameter $parsed.Ast 'DryRun'
            $isSwitch = $false
            foreach ($attribute in @($dryRun.Attributes)) {
                if ([string]$attribute.TypeName.FullName -ieq 'switch') { $isSwitch = $true }
            }
            Assert-True $isSwitch 'DryRun must be declared [switch]'
            $composition = Find-DriverParameter $parsed.Ast 'Composition'
            $isString = $false
            foreach ($attribute in @($composition.Attributes)) {
                if ([string]$attribute.TypeName.FullName -ieq 'string') { $isString = $true }
            }
            Assert-True $isString 'Composition must be a CSV string (array values cannot cross -File)'
        }

        It 'declares the full known-primitive vocabulary and the internal hard cap' {
            $text = Get-Content -Raw -LiteralPath $DriverPath
            foreach ($primitive in @(
                'FlatWalk', 'DiagonalWalk', 'TapJump', 'SneakPulse', 'LookDrift', 'F5Cycle', 'Idle'
            )) {
                Assert-True ($text -cmatch [regex]::Escape("'$primitive'")) `
                    "known primitive '$primitive' must be declared"
            }
            Assert-True ($text -cmatch [regex]::Escape('$script:MaxTotalDurationSeconds = 120')) `
                'the internal hard duration cap must remain 120 seconds'
        }
    }

    Context 'default dry-run plan structure' {

        It 'runs dry without a target and reports injection disabled' {
            Assert-Equal 0 $script:DefaultRun.ExitCode 'dry run must exit successfully'
            $joined = $script:DefaultRun.Output -join "`n"
            Assert-True ($joined -cmatch 'GAMEPAD COVERAGE GAP') 'GamePad gap notice must print'
            $gapCount = @($script:DefaultRun.Output | Where-Object {
                $_ -cmatch 'GAMEPAD COVERAGE GAP' }).Count
            Assert-Equal 1 $gapCount 'GamePad gap notice prints exactly once per run'
            Assert-True ($joined -cmatch 'injection=disabled') 'dry run must state injection=disabled'
            Assert-True ($joined -cnotmatch 'SendInput keyboard event failed') `
                'dry run must not attempt injection'
        }

        It 'covers flat walk, diagonal walk, tap jump, and ends back at FirstPerson via F5' {
            $plan = $script:DefaultPlan
            Assert-Equal 'rust-mcbe-organic-input-plan-v1' ([string]$plan.schema) 'plan schema'
            # composition_requested echoes the CSV text; split before membership.
            $requested = @(([string]$plan.composition_requested) -split ',')
            foreach ($required in @('FlatWalk', 'DiagonalWalk', 'TapJump', 'F5Cycle')) {
                Assert-True ($requested -contains $required) `
                    "requested composition must contain $required"
            }
            $effective = @($plan.composition_effective)
            Assert-True ($effective.Count -ge 5) 'default plan must include repeat-tail entries'
            Assert-True ($effective -ccontains 'FlatWalkRepeat') 'periodic walk repeats must be scheduled'
            Assert-True ($effective -ccontains 'TapJumpRepeat') 'periodic jump repeats must be scheduled'
            Assert-Equal 'F5Cycle' ([string]$effective[$effective.Count - 1]) `
                'the perspective reset must terminate the composition'

            $f5Downs = @($script:DefaultEvents | Where-Object {
                [string]$_.action -ceq 'KeyDown' -and [string]$_.key -ceq 'F5' })
            $f5Ups = @($script:DefaultEvents | Where-Object {
                [string]$_.action -ceq 'KeyUp' -and [string]$_.key -ceq 'F5' })
            Assert-Equal 3 $f5Downs.Count `
                'three taps traverse FirstPerson->ThirdBack->ThirdFront->FirstPerson'
            Assert-Equal 3 $f5Ups.Count 'every F5 tap must be released'

            $keyEvents = @($script:DefaultEvents | Where-Object {
                [string]$_.action -cin @('KeyDown', 'KeyUp') })
            Assert-True ($keyEvents.Count -gt 0) 'plan must contain key events'
            $lastKey = $keyEvents[$keyEvents.Count - 1]
            Assert-Equal 'KeyUp' ([string]$lastKey.action) 'final key event must be a release'
            Assert-Equal 'F5' ([string]$lastKey.key) 'final key event must release F5'
            Assert-Equal 'F5Cycle' ([string]$lastKey.label) 'final release must belong to F5Cycle'
        }

        It 'pairs every key down with a release and respects the duration bound' {
            Assert-DriverHasKeyBalance $script:DefaultEvents
            $mouseMoves = @($script:DefaultEvents | Where-Object {
                [string]$_.action -ceq 'MouseMove' })
            Assert-Equal 0 $mouseMoves.Count 'default composition must not move the mouse'
            Assert-True ([int]$script:DefaultPlan.planned_duration_ms -le 45000) `
                'planned duration must stay within the requested budget'
            Assert-True ([int]$script:DefaultPlan.planned_duration_ms -gt 0) `
                'planned duration must be positive'
        }
    }

    Context 'deterministic dry-run output' {

        It 'reproduces byte-identical timelines and plan artifacts across runs' {
            $secondPath = Join-Path $script:Scratch 'second-plan.json'
            $second = Invoke-DriverChild @(
                '-DryRun', '-DurationSeconds', '45', '-OutputPlanPath', $secondPath)
            Assert-Equal 0 $second.ExitCode 'second dry run must exit successfully'

            # The artifact path line differs by design; everything else must match.
            $firstJoined = (@($script:DefaultRun.Output | Where-Object {
                $_ -cnotmatch '^plan-json written:' }) -join "`n")
            $secondJoined = (@($second.Output | Where-Object {
                $_ -cnotmatch '^plan-json written:' }) -join "`n")
            Assert-True ($firstJoined -ceq $secondJoined) 'dry-run stdout must be deterministic'

            $firstHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $script:DefaultPlanPath).Hash
            $secondHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $secondPath).Hash
            Assert-Equal $firstHash $secondHash 'plan artifact bytes must be deterministic'
        }
    }

    Context 'composition handling and budget enforcement' {

        It 'accepts every known primitive individually' {
            foreach ($primitive in @(
                'FlatWalk', 'DiagonalWalk', 'TapJump', 'SneakPulse', 'LookDrift', 'F5Cycle', 'Idle'
            )) {
                $run = Invoke-DriverChild @(
                    '-DryRun', '-OneShot', '-DurationSeconds', '30',
                    '-Composition', $primitive)
                Assert-Equal 0 $run.ExitCode "primitive $primitive must be accepted"
            }
        }

        It 'honors a custom composition with bounded clamped look drift' {
            $planPath = Join-Path $script:Scratch 'custom-plan.json'
            $run = Invoke-DriverChild @(
                '-DryRun', '-OneShot', '-DurationSeconds', '30',
                '-Composition', 'SneakPulse,LookDrift,Idle', '-OutputPlanPath', $planPath)
            Assert-Equal 0 $run.ExitCode 'custom composition must exit successfully'

            $plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
            $effective = @($plan.composition_effective)
            Assert-Equal 3 $effective.Count 'custom composition length'
            Assert-Equal 'SneakPulse' ([string]$effective[0]) 'custom order 1'
            Assert-Equal 'LookDrift' ([string]$effective[1]) 'custom order 2'
            Assert-Equal 'Idle' ([string]$effective[2]) 'custom order 3'

            $events = @($plan.events)
            Assert-DriverHasKeyBalance $events
            $keys = @($events | Where-Object { [string]$_.action -cin @('KeyDown', 'KeyUp') })
            foreach ($step in $keys) {
                Assert-Equal 'ShiftLeft' ([string]$step.key) `
                    'custom composition may only press ShiftLeft'
            }
            $moves = @($events | Where-Object { [string]$_.action -ceq 'MouseMove' })
            Assert-Equal 4 $moves.Count 'look drift emits four relative moves'
            $netDx = 0
            foreach ($move in $moves) {
                Assert-Equal 8 ([Math]::Abs([int]$move.dx)) 'each drift step must equal the clamp'
                Assert-Equal 0 ([int]$move.dy) 'drift must stay horizontal'
                $netDx += [int]$move.dx
            }
            Assert-Equal 0 $netDx 'look drift must return to its starting yaw offset'
            Assert-True ([int]$plan.planned_duration_ms -le 30000) 'custom plan must respect budget'
        }

        It 'fails closed before any interaction when the budget cannot hold the composition' {
            $run = Invoke-DriverChild @('-OneShot', '-DurationSeconds', '5')
            Assert-True ($run.ExitCode -ne 0) 'impossible budget must exit nonzero'
            $joined = $run.Output -join "`n"
            Assert-True ($joined -cmatch 'plan error:') 'failure must surface as a plan error'
            Assert-True ($joined -cmatch 'exceeds') 'failure must explain the budget overrun'
            Assert-True ($joined -cnotmatch 'KeyDown W') 'no timeline may print for a failed plan'
        }

        It 'truncates the periodic tail to stay within a maximal budget' {
            $planPath = Join-Path $script:Scratch 'max-plan.json'
            $run = Invoke-DriverChild @(
                '-DryRun', '-DurationSeconds', '120', '-OutputPlanPath', $planPath)
            Assert-Equal 0 $run.ExitCode 'maximal-budget dry run must succeed'
            $plan = Get-Content -Raw -LiteralPath $planPath | ConvertFrom-Json
            Assert-True ([int]$plan.planned_duration_ms -le 120000) `
                'even maximal plans must stay within the hard 120 s bound'
        }

        It 'rejects unknown primitives before any window interaction' {
            $run = Invoke-DriverChild @('-DryRun', '-Composition', 'BunnyHop')
            Assert-True ($run.ExitCode -ne 0) 'unknown primitive must exit nonzero'
            $joined = $run.Output -join "`n"
            Assert-True ($joined -cmatch 'unknown primitive') 'rejection must name the cause'
        }
    }

    Context 'live-path safety refusals' {

        It 'rejects mutually exclusive target selectors' {
            $run = Invoke-DriverChild @(
                '-DryRun', '-ProcessName', 'anything', '-WindowTitle', 'anything')
            Assert-True ($run.ExitCode -ne 0) 'both targets together must exit nonzero'
            $joined = $run.Output -join "`n"
            Assert-True ($joined -cmatch 'not both') 'usage error must explain exclusivity'
        }

        It 'refuses a live run with no target selector at all' {
            $run = Invoke-DriverChild @()
            Assert-True ($run.ExitCode -ne 0) 'target-less live run must exit nonzero'
            $joined = $run.Output -join "`n"
            Assert-True ($joined -cmatch 'requires -ProcessName') `
                'refusal must explain the missing target'
            Assert-True ($joined -cnotmatch 'KeyDown') 'nothing may be injected'
        }

        It 'aborts cleanly when no matching foreground window appears within grace' {
            # The default composition fits a 45 s budget, so the plan builds and
            # the run reaches the foreground gate before refusing.
            $run = Invoke-DriverChild @(
                '-ProcessName', 'rust-mcbe-no-such-process-xyz',
                '-GraceSeconds', '1', '-DurationSeconds', '45')
            Assert-Equal 2 $run.ExitCode 'clean safety abort must exit with code 2'
            $joined = $run.Output -join "`n"
            Assert-True ($joined -cmatch 'refusing to inject') 'abort must be explicit'
            Assert-True ($joined -cnotmatch 'KeyDown') 'abort path must never inject keys'
        }
    }

    Context 'window activation (-Activate)' {

        It 'declares Activate as a default-off switch' {
            $parsed = Get-DriverParseResult
            $activate = Find-DriverParameter $parsed.Ast 'Activate'
            $isSwitch = $false
            foreach ($attribute in @($activate.Attributes)) {
                if ([string]$attribute.TypeName.FullName -ieq 'switch') { $isSwitch = $true }
            }
            Assert-True $isSwitch 'Activate must be declared [switch]'
            Assert-True ($null -eq $activate.DefaultValue) `
                'Activate must default to off (no initializer expression)'
        }

        It 'keeps dry-run stdout and plan artifacts byte-identical when -Activate is passed' {
            $activatePath = Join-Path $script:Scratch 'activate-dry-plan.json'
            $run = Invoke-DriverChild @(
                '-DryRun', '-Activate', '-DurationSeconds', '45',
                '-OutputPlanPath', $activatePath)
            Assert-Equal 0 $run.ExitCode 'dry run with -Activate must exit successfully'

            # The artifact path line differs by design; everything else must match.
            $baselineJoined = (@($script:DefaultRun.Output | Where-Object {
                $_ -cnotmatch '^plan-json written:' }) -join "`n")
            $activateJoined = (@($run.Output | Where-Object {
                $_ -cnotmatch '^plan-json written:' }) -join "`n")
            Assert-True ($baselineJoined -ceq $activateJoined) `
                '-Activate must not change any dry-run output line'
            $baselineHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $script:DefaultPlanPath).Hash
            $activateHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $activatePath).Hash
            Assert-Equal $baselineHash $activateHash `
                '-Activate must not change the plan artifact bytes (dry runs never activate)'
        }

        It 'parses the focus helper cleanly and binds its structural surface' {
            $helperPath = Join-Path $RepoRoot 'scripts\organic-movement-input-focus.ps1'
            Assert-True (Test-Path -LiteralPath $helperPath) 'focus helper module must exist'
            $tokens = $null
            $errors = $null
            $helperAst = [System.Management.Automation.Language.Parser]::ParseFile(
                $helperPath, [ref]$tokens, [ref]$errors)
            Assert-Equal 0 @($errors).Count 'focus helper must parse without syntax errors'
            $functions = @($helperAst.FindAll(
                { param($node) $node -is [System.Management.Automation.Language.FunctionDefinitionAst] },
                $true))
            foreach ($functionName in @(
                'Get-OrganicInputTargetCandidates',
                'Test-OrganicInputForegroundCandidate',
                'Invoke-OrganicInputActivationAttempt',
                'Wait-OrganicInputForegroundTarget'
            )) {
                Assert-True ($null -ne ($functions | Where-Object {
                    ([string]$_.Name) -ceq $functionName })) `
                    "focus helper must define $functionName"
            }
        }

        It 'uses only legitimate activation mechanisms and keeps the gate authoritative' {
            $text = Get-Content -Raw -LiteralPath (Join-Path $RepoRoot 'scripts\organic-movement-input-focus.ps1')
            foreach ($token in @(
                'AppActivate', 'WScript.Shell', 'SetForegroundWindow',
                'AttachThreadInput', 'SW_RESTORE', 'TapAltUnlockKey',
                '[activate]', 'refused'
            )) {
                Assert-True ($text -cmatch [regex]::Escape($token)) `
                    "activation lane must contain '$token'"
            }
            $driverText = Get-Content -Raw -LiteralPath $DriverPath
            Assert-True ($driverText -cmatch [regex]::Escape(
                ". (Join-Path `$PSScriptRoot 'organic-movement-input-focus.ps1')")) `
                'driver must dot-source the focus helper module'
            Assert-True ($driverText -cmatch [regex]::Escape('-AttemptActivation:$Activate')) `
                'driver must forward the Activate switch to the wait gate'
        }

        It 'rechecks the originally matched foreground handle before every injected event' {
            $safetyPath = Join-Path $RepoRoot 'scripts\organic-movement-input-safety.ps1'
            Assert-True (Test-Path -LiteralPath $safetyPath -PathType Leaf) `
                'event focus safety helper must exist'
            . $safetyPath

            $observed = [Collections.Generic.Queue[long]]::new()
            $observed.Enqueue(0x1234)
            $observed.Enqueue(0x5678)
            $probe = { return $observed.Dequeue() }
            { Assert-OrganicInputForegroundHandle -ExpectedHandle 0x1234 `
                    -ForegroundHandleProvider $probe } | Should Not Throw
            { Assert-OrganicInputForegroundHandle -ExpectedHandle 0x1234 `
                    -ForegroundHandleProvider $probe } | Should Throw

            $driverText = Get-Content -Raw -LiteralPath $DriverPath
            foreach ($action in @('KeyDown', 'KeyUp', 'MouseMove')) {
                $pattern = "'$action'\s*\{\s*Assert-OrganicInputForegroundHandle"
                Assert-True ($driverText -cmatch $pattern) `
                    "$action must recheck focus before its SendInput event"
            }
            Assert-True ($driverText -cmatch 'finally\s*\{[\s\S]*release guard') `
                'focus-loss abort must retain the held-key release guard'
        }

        It 'reports honest activation failure and still refuses when no target ever appears' {
            $run = Invoke-DriverChild @(
                '-ProcessName', 'rust-mcbe-no-such-process-xyz', '-Activate',
                '-GraceSeconds', '1', '-DurationSeconds', '45')
            Assert-Equal 2 $run.ExitCode 'failed activation must refuse with exit code 2'
            $joined = $run.Output -join "`n"
            Assert-True ($joined -cnotmatch '\[activate\].*(succeeded|took the foreground|matched:)') `
                'no activation success may be claimed for a nonexistent process'
            Assert-True ($joined -cmatch '\[activate\] programmatic activation enabled') `
                'the activation lane must announce itself on [activate] lines'
            Assert-True ($joined -cmatch 'without the target owning the foreground') `
                'exhausted activation attempts must be reported honestly'
            Assert-True ($joined -cmatch 'refusing to inject') 'refusal must stay explicit'
            Assert-True ($joined -cnotmatch 'KeyDown') 'abort path must never inject keys'
        }
    }
}
