<#
.SYNOPSIS
    Bounded synthetic organic-movement input driver for live acceptance sessions.

.DESCRIPTION
    Test tooling only. Drives the focused target window with plain, user-equivalent
    foreground keyboard and mouse events (SendInput-class scan-code injection) so a
    live CandidatePhysics session observes real flat walking, diagonal walking, a
    tap jump, small look drift, sneak pulses, and the client's perspective cycle
    instead of a stationary player. The driver is deliberately game-unaware: it
    reads no server state, inspects no packets, holds no client memory, and applies
    no evasion logic; every event is the same operating-system input stream entry a
    person pressing the physical key produces.

    Safety contract: a live run refuses to start unless a target process name or
    window title is supplied and a matching window becomes the foreground window
    within the grace period (otherwise it aborts cleanly having injected nothing);
    -DryRun prints the planned timeline and never loads any native injection entry
    point; total duration is hard-bounded (default 45 s, maximum 120 s, enforced by
    parameter validation plus an internal constant); every key down is paired with
    a key up by construction and the executor releases still-held keys in a finally
    block including abnormal exits. A forced kill of the host process remains
    outside any software guarantee.

    Unattended runs may pass -Activate: while waiting on the same foreground gate
    the driver then repeatedly attempts to raise a matching target window itself
    (ShowWindow restore, SetForegroundWindow with standard unlocks, WScript.Shell
    AppActivate) using only ordinary local automation. Every attempt is logged as
    an "[activate]" line with its honest outcome; the GetForegroundWindow check
    stays authoritative, activation success is only a hint, and a failed
    activation refuses exactly as the manual-focus path does. Default OFF
    preserves today's semantics; -DryRun ignores the switch entirely.

    Validator mapping (scripts/acceptance Phase3Aggregate CandidatePhysics):
      FlatWalk     -> grounded flat-walk witness (one axis >= 0.25, non-jump)
      DiagonalWalk -> grounded diagonal-walk witness (both axes >= 0.25)
      TapJump      -> non-repeated single-jump witness (grounded takeoff)
      F5Cycle      -> exact FirstPerson -> ThirdBack -> ThirdFront -> FirstPerson
                      perspective sequence. This client cycles three states
                      (app/src/camera.rs next_perspective; proven by
                      perspective_cycle_matches_bedrock_settings_order), so three
                      taps starting at FirstPerson end back at FirstPerson.
      SneakPulse, LookDrift, Idle -> organic variety; no aggregate assertion.

    Honest limitation, stated on every run: GamePad input frames are NOT covered
    by this driver and remain an open blocker for full CandidatePhysics aggregate
    validation (the scenario requires both KeyboardMouse and GamePad witnesses).

.EXAMPLE
    .\scripts\organic-movement-input-driver.ps1 -DryRun
    Prints the default plan without touching any window or injecting anything.

.EXAMPLE
    .\scripts\organic-movement-input-driver.ps1 -ProcessName bedrock-client
    Waits up to 10 s for a bedrock-client window to be foreground, then runs the
    default organic-movement scenario into it.

.EXAMPLE
    .\scripts\organic-movement-input-driver.ps1 -ProcessName bedrock-client `
        -DurationSeconds 90 -Composition 'FlatWalk,TapJump,SneakPulse,LookDrift'

.EXAMPLE
    .\scripts\organic-movement-input-driver.ps1 -ProcessName bedrock-client -Activate
    Same gate and scenario, but the driver also attempts to raise the target
    window itself for unattended acceptance runs; refusal on failure is unchanged.
#>
[CmdletBinding()]
param(
    # Target selection for live runs. Provide exactly one. Dry runs ignore both.
    [string]$ProcessName = '',
    [string]$WindowTitle = '',
    # Print the planned timeline only; never injects and needs no target.
    [switch]$DryRun,
    # Total synthetic-input budget. Hard-capped at $script:MaxTotalDurationSeconds.
    [ValidateRange(5, 120)]
    [int]$DurationSeconds = 45,
    # Seconds to wait for the target window to become the foreground window.
    [ValidateRange(1, 30)]
    [int]$GraceSeconds = 10,
    # Opt-in programmatic activation while waiting on the foreground gate:
    # repeatedly attempt to raise a matching target window (ShowWindow restore,
    # SetForegroundWindow with AttachThreadInput/Alt-tap unlocks, WScript.Shell
    # AppActivate). Default OFF preserves manual-focus-only semantics; every
    # attempt is logged as an "[activate]" line and the gate stays authoritative.
    # Ignored by -DryRun.
    [switch]$Activate,
    # Comma-separated primitives from $script:KnownPrimitives, executed in order.
    [string]$Composition = 'FlatWalk,DiagonalWalk,TapJump,F5Cycle',
    # Disable the periodic FlatWalk/TapJump tail that fills remaining budget.
    [switch]$OneShot,
    [ValidateRange(200, 10000)]
    [int]$WalkMilliseconds = 3000,
    [ValidateRange(20, 1000)]
    [int]$TapJumpMilliseconds = 80,
    [ValidateRange(100, 5000)]
    [int]$SneakMilliseconds = 1000,
    [ValidateRange(1, 64)]
    [int]$LookDriftPixels = 8,
    # Taps in F5Cycle. This client's verified cycle has three states, so the
    # default 3 ends back at FirstPerson when the run starts there.
    [ValidateRange(1, 6)]
    [int]$F5TapCount = 3,
    [ValidateRange(200, 2000)]
    [int]$F5GapMilliseconds = 700,
    # Optional machine-readable plan artifact (JSON) written during dry runs.
    [string]$OutputPlanPath = ''
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Foreground-target resolution and the optional -Activate activation lane live
# in a sibling module so this driver stays under the repository's 800-line
# PowerShell cap. Dot-sourcing keeps every helper in this script's scope.
. (Join-Path $PSScriptRoot 'organic-movement-input-focus.ps1')

# Constants ----------------------------------------------------------------

# Belt-and-braces duplicate of the DurationSeconds parameter ceiling. Nothing may
# extend the total duration beyond this bound regardless of future edits.
$script:MaxTotalDurationSeconds = 120
$script:PlanSchema = 'rust-mcbe-organic-input-plan-v1'

# The complete primitive vocabulary. Unknown names fail closed before any
# window interaction or injection.
$script:KnownPrimitives = @(
    'FlatWalk', 'DiagonalWalk', 'TapJump', 'SneakPulse', 'LookDrift', 'F5Cycle', 'Idle'
)

# Set-1 PC/AT make codes, exactly what a physical keyboard sends. Windows maps
# them to virtual keys and the client's input stack sees ordinary hardware
# input. Names mirror crates/input binding usages: W/A/S/D movement, Space jump,
# LeftShift sneak, F5 perspective cycle.
$script:ScanCodes = @{
    'W'         = 0x11
    'A'         = 0x1E
    'S'         = 0x1F
    'D'         = 0x20
    'Space'     = 0x39
    'ShiftLeft' = 0x2A
    'F5'        = 0x3F
}
$script:ScanNames = @{}
foreach ($scanName in $script:ScanCodes.Keys) {
    $script:ScanNames[[int]$script:ScanCodes[$scanName]] = $scanName
}

$script:SettleMilliseconds = 250
$script:F5HoldMilliseconds = 60
$script:LookStepMilliseconds = 120
$script:IdleMilliseconds = 1500
$script:MaxPlanEvents = 4096

$script:GamePadGapNotice = (
    'GAMEPAD COVERAGE GAP: this driver synthesizes keyboard and mouse input only. ' +
    'GamePad-frame coverage is NOT provided and remains an open blocker for full ' +
    'CandidatePhysics aggregate validation.'
)

# Composition parsing ------------------------------------------------------

function Get-OrganicInputCompositionList {
    param([Parameter(Mandatory = $true)][string]$Raw)

    $names = [Collections.Generic.List[string]]::new()
    foreach ($token in $Raw.Split(',')) {
        $name = $token.Trim()
        if ([string]::IsNullOrEmpty($name)) { continue }
        if ($script:KnownPrimitives -cnotcontains $name) {
            throw ("unknown primitive '{0}'; known primitives: {1}" -f
                $name, ($script:KnownPrimitives -join ','))
        }
        $names.Add($name)
    }
    if ($names.Count -eq 0) {
        throw 'composition is empty; name at least one primitive'
    }
    return ,@($names.ToArray())
}

# Plan builder (pure; no clock reads, no window access) --------------------

function Add-OrganicInputActionStep {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][Collections.Generic.List[object]]$Steps,
        [Parameter(Mandatory = $true)][ref]$TimeMs,
        [Parameter(Mandatory = $true)][ValidateSet('KeyDown', 'KeyUp', 'MouseMove')][string]$Action,
        [string]$Key = '',
        [int]$Dx = 0,
        [int]$Dy = 0,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $step = [pscustomobject][ordered]@{
        offset_ms    = $TimeMs.Value
        action       = $Action
        key          = $Key
        scan_code    = $null
        dx           = $null
        dy           = $null
        milliseconds = $null
        label        = $Label
    }
    if ($Action -eq 'KeyDown' -or $Action -eq 'KeyUp') {
        if (-not $script:ScanCodes.ContainsKey($Key)) {
            throw ("internal error: no scan code bound for key '{0}'" -f $Key)
        }
        $step.scan_code = [int]$script:ScanCodes[$Key]
    }
    else {
        $step.dx = $Dx
        $step.dy = $Dy
    }
    $Steps.Add($step)
}

function Add-OrganicInputWaitStep {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][Collections.Generic.List[object]]$Steps,
        [Parameter(Mandatory = $true)][ref]$TimeMs,
        [Parameter(Mandatory = $true)][ValidateRange(1, [int]::MaxValue)][int]$Milliseconds,
        [Parameter(Mandatory = $true)][string]$Label
    )

    $Steps.Add([pscustomobject][ordered]@{
        offset_ms    = $TimeMs.Value
        action       = 'Wait'
        key          = ''
        scan_code    = $null
        dx           = $null
        dy           = $null
        milliseconds = $Milliseconds
        label        = $Label
    })
    $TimeMs.Value += $Milliseconds
}

function Add-OrganicInputPrimitive {
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][Collections.Generic.List[object]]$Steps,
        [Parameter(Mandatory = $true)][ref]$TimeMs,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][hashtable]$Timing,
        [Parameter(Mandatory = $true)][string]$Label
    )

    switch ($Name) {
        'FlatWalk' {
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyDown -Key 'W' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Walk -Label $Label
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyUp -Key 'W' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Settle -Label $Label
        }
        'DiagonalWalk' {
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyDown -Key 'W' -Label $Label
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyDown -Key 'A' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Walk -Label $Label
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyUp -Key 'A' -Label $Label
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyUp -Key 'W' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Settle -Label $Label
        }
        'TapJump' {
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyDown -Key 'Space' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.TapJump -Label $Label
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyUp -Key 'Space' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Settle -Label $Label
        }
        'SneakPulse' {
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyDown -Key 'ShiftLeft' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Sneak -Label $Label
            Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyUp -Key 'ShiftLeft' -Label $Label
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Settle -Label $Label
        }
        'LookDrift' {
            # Small clamped relative moves out and back; net displacement zero.
            foreach ($direction in @(@(+1, +1), @(-1, -1))) {
                foreach ($sign in $direction) {
                    $delta = $sign * $Timing.DriftPixels
                    Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action MouseMove -Dx $delta -Label $Label
                    Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.LookStep -Label $Label
                }
            }
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Settle -Label $Label
        }
        'F5Cycle' {
            # Tap starts are spaced one gap apart so taps are evenly cadenced.
            for ($index = 0; $index -lt $Timing.F5Taps; $index++) {
                Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyDown -Key 'F5' -Label $Label
                Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.F5Hold -Label $Label
                Add-OrganicInputActionStep -Steps $Steps -TimeMs $TimeMs -Action KeyUp -Key 'F5' -Label $Label
                Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds ($Timing.F5Gap - $Timing.F5Hold) -Label $Label
            }
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Settle -Label $Label
        }
        'Idle' {
            Add-OrganicInputWaitStep -Steps $Steps -TimeMs $TimeMs -Milliseconds $Timing.Idle -Label $Label
        }
        default {
            throw ("internal error: unhandled primitive '{0}'" -f $Name)
        }
    }
}

function New-OrganicInputPlan {
    param(
        [Parameter(Mandatory = $true)][string]$CompositionRaw,
        [Parameter(Mandatory = $true)][ValidateRange(1, 120)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)][hashtable]$Timing,
        [switch]$OneShot
    )

    $requested = Get-OrganicInputCompositionList -Raw $CompositionRaw
    $budgetMs = $DurationSeconds * 1000
    $steps = [Collections.Generic.List[object]]::new()
    $effective = [Collections.Generic.List[string]]::new()
    $cursor = 0

    # The periodic tail must never run after the perspective reset: when the
    # composition ends with F5Cycle the repeats are inserted ahead of it so every
    # such run finishes back at FirstPerson with the exact validator sequence.
    $trailingIndex = $requested.Count - 1
    $trailingIsF5 = $requested[$trailingIndex] -ceq 'F5Cycle'
    $coreEnd = if ($trailingIsF5) { $trailingIndex } else { $trailingIndex + 1 }

    for ($index = 0; $index -lt $coreEnd; $index++) {
        Add-OrganicInputPrimitive -Steps $steps -TimeMs ([ref]$cursor) -Name $requested[$index] -Timing $Timing -Label $requested[$index]
        $effective.Add($requested[$index])
    }

    $pairCost = $Timing.Walk + $Timing.Settle + $Timing.TapJump + $Timing.Settle
    # Reserve room for the trailing perspective reset so the tail can never
    # push the closing F5Cycle past the budget boundary.
    $reservedCost = if ($trailingIsF5) { $Timing.F5Taps * $Timing.F5Gap + $Timing.Settle } else { 0 }
    if (-not $OneShot) {
        while (($cursor + $pairCost + $reservedCost) -le $budgetMs) {
            Add-OrganicInputPrimitive -Steps $steps -TimeMs ([ref]$cursor) -Name 'FlatWalk' `
                -Timing $Timing -Label 'FlatWalkRepeat'
            $effective.Add('FlatWalkRepeat')
            Add-OrganicInputPrimitive -Steps $steps -TimeMs ([ref]$cursor) -Name 'TapJump' `
                -Timing $Timing -Label 'TapJumpRepeat'
            $effective.Add('TapJumpRepeat')
        }
    }

    if ($trailingIsF5) {
        Add-OrganicInputPrimitive -Steps $steps -TimeMs ([ref]$cursor) -Name 'F5Cycle' `
            -Timing $Timing -Label 'F5Cycle'
        $effective.Add('F5Cycle')
    }

    if ($cursor -gt $budgetMs) {
        throw (("composition requires {0} ms which exceeds the {1} s budget; " +
                'shorten the composition or raise -DurationSeconds') -f $cursor, $DurationSeconds)
    }
    if ($steps.Count -gt $script:MaxPlanEvents) {
        throw ("plan produced {0} steps which exceeds the {1}-event safety bound" -f
            $steps.Count, $script:MaxPlanEvents)
    }

    return [pscustomobject][ordered]@{
        schema                 = $script:PlanSchema
        mode                   = 'keyboard-mouse-sendinput-plan'
        duration_seconds       = $DurationSeconds
        planned_duration_ms    = $cursor
        one_shot               = [bool]$OneShot
        f5_tap_count           = $Timing.F5Taps
        composition_requested  = $CompositionRaw.Trim()
        composition_effective  = @($effective.ToArray())
        events                 = @($steps.ToArray())
    }
}

# Timeline formatting and artifacts ----------------------------------------

function Format-OrganicInputOffset {
    param([Parameter(Mandatory = $true)][int]$Milliseconds)
    return ('+' + $Milliseconds.ToString('00000000', [Globalization.CultureInfo]::InvariantCulture))
}

function Get-OrganicInputTotals {
    param([Parameter(Mandatory = $true)][object[]]$Events)

    $totals = @{ Events = $Events.Count; KeyDowns = 0; KeyUps = 0; MouseMoves = 0; Waits = 0 }
    foreach ($step in $Events) {
        switch ($step.action) {
            'KeyDown'   { $totals.KeyDowns++ }
            'KeyUp'     { $totals.KeyUps++ }
            'MouseMove' { $totals.MouseMoves++ }
            'Wait'      { $totals.Waits++ }
        }
    }
    return $totals
}

function Write-OrganicInputTimeline {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][bool]$AsDryRun
    )

    $tag = if ($AsDryRun) { 'dryrun' } else { 'plan' }
    $injection = if ($AsDryRun) { 'disabled' } else { 'enabled' }
    Write-Output ('{0} schema={1} mode={2} injection={3}' -f $tag, $Plan.schema, $Plan.mode, $injection)
    Write-Output ('{0} duration_seconds={1} planned_duration_ms={2} one_shot={3} f5_tap_count={4}' -f
        $tag, $Plan.duration_seconds, $Plan.planned_duration_ms, $Plan.one_shot, $Plan.f5_tap_count)
    Write-Output ('{0} composition_effective={1}' -f $tag, ($Plan.composition_effective -join ','))
    foreach ($step in $Plan.events) {
        $offset = Format-OrganicInputOffset -Milliseconds $step.offset_ms
        switch ($step.action) {
            'KeyDown' {
                Write-Output ('[{0} {1}] KeyDown {2} sc=0x{3:x2} label={4}' -f
                    $tag, $offset, $step.key, $step.scan_code, $step.label)
            }
            'KeyUp' {
                Write-Output ('[{0} {1}] KeyUp {2} sc=0x{3:x2} label={4}' -f
                    $tag, $offset, $step.key, $step.scan_code, $step.label)
            }
            'MouseMove' {
                Write-Output ('[{0} {1}] MouseMove dx={2} dy={3} label={4}' -f
                    $tag, $offset, $step.dx, $step.dy, $step.label)
            }
            'Wait' {
                Write-Output ('[{0} {1}] Wait ms={2} label={3}' -f
                    $tag, $offset, $step.milliseconds, $step.label)
            }
        }
    }
    $totals = Get-OrganicInputTotals -Events @($Plan.events)
    Write-Output ('{0} totals events={1} key_downs={2} key_ups={3} mouse_moves={4} waits={5}' -f
        $tag, $totals.Events, $totals.KeyDowns, $totals.KeyUps, $totals.MouseMoves, $totals.Waits)
}

function Write-OrganicInputPlanJson {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$Path
    )

    $parent = Split-Path -Parent $Path
    if (-not [string]::IsNullOrEmpty($parent) -and -not (Test-Path -LiteralPath $parent -PathType Container)) {
        throw ("output plan directory does not exist: {0}" -f $parent)
    }
    $json = ConvertTo-Json -InputObject $Plan -Depth 5
    [IO.File]::WriteAllText($Path, $json + [Environment]::NewLine, [Text.UTF8Encoding]::new($false))
    Write-Output ("plan-json written: {0}" -f $Path)
}

# Native injection (live path only; never reached during dry runs) ---------

function Initialize-OrganicInputNativeMethods {
    if ('RustMcbe.OrganicInput.NativeMethods' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace RustMcbe.OrganicInput
{
    [StructLayout(LayoutKind.Sequential)]
    public struct MouseInput
    {
        public int dx; public int dy; public uint mouseData; public uint dwFlags;
        public uint time; public IntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct KeyboardInput
    {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time;
        public IntPtr dwExtraInfo;
    }

    // Explicit layout keeps both union views at offset zero; MOUSEINPUT is the
    // largest member so INPUT marshals at the size SendInput expects.
    [StructLayout(LayoutKind.Explicit)]
    public struct InputUnion
    {
        [FieldOffset(0)] public MouseInput mi;
        [FieldOffset(0)] public KeyboardInput ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct Input
    {
        public uint type; public InputUnion u;
    }

    public static class NativeMethods
    {
        public const uint InputMouse = 0;
        public const uint InputKeyboard = 1;
        public const uint KeyEventFScanCode = 0x0008;
        public const uint KeyEventFKeyUp = 0x0002;
        public const uint MouseEventFMove = 0x0001;

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint nInputs, Input[] inputs, int cbSize);

        [DllImport("user32.dll")]
        public static extern IntPtr GetForegroundWindow();

        // Struct assembly stays on the C# side so PowerShell never fights
        // value-type copy semantics on nested union fields.
        public static Input MakeKey(ushort scanCode, bool keyUp)
        {
            Input input = new Input();
            input.type = InputKeyboard;
            input.u.ki.wScan = scanCode;
            input.u.ki.dwFlags = KeyEventFScanCode | (keyUp ? KeyEventFKeyUp : 0u);
            return input;
        }

        public static Input MakeMouse(int dx, int dy)
        {
            Input input = new Input();
            input.type = InputMouse;
            input.u.mi.dx = dx;
            input.u.mi.dy = dy;
            input.u.mi.dwFlags = MouseEventFMove;
            return input;
        }

        public static uint SendOne(Input input)
        {
            Input[] buffer = new Input[1];
            buffer[0] = input;
            return SendInput(1, buffer, Marshal.SizeOf(typeof(Input)));
        }
    }
}
'@
}

function Send-OrganicInputKey {
    param(
        [Parameter(Mandatory = $true)][ValidateRange(1, 255)][int]$ScanCode,
        [Parameter(Mandatory = $true)][bool]$KeyUp
    )

    $input = [RustMcbe.OrganicInput.NativeMethods]::MakeKey([uint16]$ScanCode, $KeyUp)
    $sent = [RustMcbe.OrganicInput.NativeMethods]::SendOne($input)
    if ($sent -ne 1) {
        $error = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw ("SendInput keyboard event failed (sent={0}, win32Error={1})" -f $sent, $error)
    }
}

function Send-OrganicInputMouseMove {
    param(
        [Parameter(Mandatory = $true)][int]$Dx,
        [Parameter(Mandatory = $true)][int]$Dy
    )

    if ([Math]::Abs($Dx) -gt 4096 -or [Math]::Abs($Dy) -gt 4096) {
        throw ("refusing oversized relative mouse move ({0},{1})" -f $Dx, $Dy)
    }
    $input = [RustMcbe.OrganicInput.NativeMethods]::MakeMouse($Dx, $Dy)
    $sent = [RustMcbe.OrganicInput.NativeMethods]::SendOne($input)
    if ($sent -ne 1) {
        $error = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        throw ("SendInput mouse event failed (sent={0}, win32Error={1})" -f $sent, $error)
    }
}

# Target resolution and foreground gating live in the dot-sourced focus
# helper (organic-movement-input-focus.ps1): candidate enumeration, the
# authoritative GetForegroundWindow membership check, and the opt-in -Activate
# activation lane. The wait gate returns a structured @{Matched; Log} result and
# this driver prints its log lines verbatim, refusing with exit 2 when unmatched.

# Live executor ------------------------------------------------------------

function Wait-OrganicInputBoundedMilliseconds {
    param(
        [Parameter(Mandatory = $true)][int]$Milliseconds,
        [AllowNull()][object]$Stopwatch,
        [double]$BudgetMs = [double]::MaxValue
    )

    $remaining = [double]$Milliseconds
    while ($remaining -gt 0.0) {
        if ($null -ne $Stopwatch) {
            $elapsed = $Stopwatch.Elapsed.TotalMilliseconds
            if ($elapsed -ge $BudgetMs) { return $false }
            $remaining = [Math]::Min($remaining, $BudgetMs - $elapsed)
        }
        $chunk = [Math]::Min(200.0, $remaining)
        Start-Sleep -Milliseconds ([Math]::Max(1, [int][Math]::Ceiling($chunk)))
        $remaining -= $chunk
    }
    return $true
}

function Write-OrganicInputLiveLog {
    param(
        [Parameter(Mandatory = $true)][string]$Message,
        [AllowNull()][object]$Stopwatch
    )

    if ($null -ne $Stopwatch) {
        $stamp = ([int64]$Stopwatch.Elapsed.TotalMilliseconds).ToString(
            '00000000', [Globalization.CultureInfo]::InvariantCulture)
        Write-Output ("[live +{0}ms] {1}" -f $stamp, $Message)
    }
    else {
        Write-Output ("[live --------] {0}" -f $Message)
    }
}

function Invoke-OrganicInputPlan {
    param([Parameter(Mandatory = $true)]$Plan)

    Initialize-OrganicInputNativeMethods
    $budgetMs = [double]($Plan.duration_seconds * 1000)
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $held = [Collections.Generic.Stack[int]]::new()
    $injected = 0
    $completed = $false
    try {
        Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message 'synthetic input sequence started'
        # Budget exhaustion is flagged rather than broken out of the switch
        # directly: in PowerShell a bare break inside switch binds to the switch,
        # not the foreach, so the loop re-checks this flag on the next step.
        $budgetExhausted = $false
        foreach ($step in $Plan.events) {
            if (-not $budgetExhausted -and $stopwatch.Elapsed.TotalMilliseconds -ge $budgetMs) {
                $budgetExhausted = $true
            }
            if ($budgetExhausted) {
                Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message `
                    'duration budget exhausted; skipping remaining steps'
                break
            }
            switch ($step.action) {
                'Wait' {
                    $finished = Wait-OrganicInputBoundedMilliseconds -Milliseconds $step.milliseconds `
                        -Stopwatch $stopwatch -BudgetMs $budgetMs
                    if (-not $finished) { $budgetExhausted = $true }
                }
                'KeyDown' {
                    Send-OrganicInputKey -ScanCode $step.scan_code -KeyUp $false
                    $held.Push([int]$step.scan_code)
                    $injected++
                    Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message `
                        ("KeyDown {0} sc=0x{1:x2} label={2}" -f $step.key, $step.scan_code, $step.label)
                }
                'KeyUp' {
                    Send-OrganicInputKey -ScanCode $step.scan_code -KeyUp $true
                    if ($held.Count -gt 0 -and $held.Peek() -eq [int]$step.scan_code) { $held.Pop() }
                    $injected++
                    Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message `
                        ("KeyUp {0} sc=0x{1:x2} label={2}" -f $step.key, $step.scan_code, $step.label)
                }
                'MouseMove' {
                    Send-OrganicInputMouseMove -Dx $step.dx -Dy $step.dy
                    $injected++
                    Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message `
                        ("MouseMove dx={0} dy={1} label={2}" -f $step.dx, $step.dy, $step.label)
                }
            }
        }
        $completed = $true
    }
    finally {
        # Guaranteed release of every still-held key, including abnormal exits.
        $released = 0
        while ($held.Count -gt 0) {
            $scan = $held.Pop()
            try {
                Send-OrganicInputKey -ScanCode $scan -KeyUp $true
                $name = if ($script:ScanNames.ContainsKey($scan)) { $script:ScanNames[$scan] } else { '?' }
                Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message `
                    ("KeyUp {0} sc=0x{1:x2} (release guard)" -f $name, $scan)
                $released++
            }
            catch {
                Write-Warning ("release guard could not send KeyUp for sc=0x{0:x2}: {1}" -f
                    $scan, $_.Exception.Message)
            }
        }
        $state = if ($completed) { 'complete' } else { 'aborted' }
        Write-OrganicInputLiveLog -Stopwatch $stopwatch -Message `
            ("sequence {0}: injected={1} release_guard_released={2}" -f $state, $injected, $released)
    }
}

# Main ---------------------------------------------------------------------

if (-not [string]::IsNullOrWhiteSpace($ProcessName) -and -not [string]::IsNullOrWhiteSpace($WindowTitle)) {
    Write-Output 'usage error: provide either -ProcessName or -WindowTitle, not both.'
    exit 2
}
if ($DurationSeconds -gt $script:MaxTotalDurationSeconds) {
    Write-Output ("usage error: -DurationSeconds {0} exceeds the hard {1}s bound." -f
        $DurationSeconds, $script:MaxTotalDurationSeconds)
    exit 2
}

Write-Output $script:GamePadGapNotice
Write-Output ("organic-movement synthetic input driver; plain user-equivalent foreground input; " +
    'no server-state reads, no packet inspection, no evasion logic.')

try {
    $timing = @{
        Walk      = $WalkMilliseconds
        TapJump   = $TapJumpMilliseconds
        Sneak     = $SneakMilliseconds
        DriftPixels = $LookDriftPixels
        LookStep  = $script:LookStepMilliseconds
        F5Taps    = $F5TapCount
        F5Gap     = $F5GapMilliseconds
        F5Hold    = $script:F5HoldMilliseconds
        Idle      = $script:IdleMilliseconds
        Settle    = $script:SettleMilliseconds
    }
    $plan = New-OrganicInputPlan -CompositionRaw $Composition -DurationSeconds $DurationSeconds `
        -Timing $timing -OneShot:$OneShot
}
catch {
    Write-Output ("plan error: {0}" -f $_.Exception.Message)
    exit 2
}

if ($DryRun) {
    Write-OrganicInputTimeline -Plan $plan -AsDryRun $true
    if (-not [string]::IsNullOrWhiteSpace($OutputPlanPath)) {
        Write-OrganicInputPlanJson -Plan $plan -Path $OutputPlanPath
    }
    exit 0
}

if ([string]::IsNullOrWhiteSpace($ProcessName) -and [string]::IsNullOrWhiteSpace($WindowTitle)) {
    Write-Output 'usage error: a live run requires -ProcessName or -WindowTitle; use -DryRun for plan-only output.'
    exit 2
}
if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    Write-Output 'refusing to inject: this driver requires Windows (SendInput).'
    exit 2
}

$matched = Wait-OrganicInputForegroundTarget -ProcessName $ProcessName -WindowTitle $WindowTitle `
    -GraceSeconds $GraceSeconds -AttemptActivation:$Activate
foreach ($line in @($matched.Log)) { Write-Output $line }
if (-not $matched.Matched) {
    Write-Output 'refusing to inject: no matching foreground window within the grace period.'
    exit 2
}

Write-OrganicInputTimeline -Plan $plan -AsDryRun $false
Write-Output '[live] starting in 1.0s (Ctrl+C cancels; held keys are released automatically).'
Start-Sleep -Milliseconds 1000
Invoke-OrganicInputPlan -Plan $plan
exit 0
