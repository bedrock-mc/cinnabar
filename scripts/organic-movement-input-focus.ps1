<#
.SYNOPSIS
    Foreground-target resolution and optional window activation helpers for
    scripts/organic-movement-input-driver.ps1.

.DESCRIPTION
    Test tooling only; dot-sourced by the organic-movement input driver so the
    driver stays under the repository's 800-line PowerShell cap. This module owns
    two concerns and nothing else:

    1. Target resolution: enumerate candidate windows for the operator-supplied
       process name or window title. Candidate selection is never widened beyond
       the operator's own selector, so activation can never target an arbitrary
       process.

    2. Optional programmatic activation (-Activate): repeatedly attempt to bring
       one of those candidate windows to the foreground using ordinary local
       automation only - ShowWindow(SW_RESTORE) for minimized windows,
       SetForegroundWindow with the standard AttachThreadInput unlock, a bare
       Alt-keypress SendInput unlock (types nothing), and WScript.Shell
       AppActivate by process id. Every sub-attempt is recorded as an "[activate]"
       timeline line with its honest outcome, including refusals from the Windows
       foreground-lock model. The driver's GetForegroundWindow membership check
       remains the sole authority for deciding that the target is focused;
       activation results are only a hint, never a bypass.
#>

# Focus-native entry points -------------------------------------------------

function Initialize-OrganicInputFocusNatives {
    if ('RustMcbe.OrganicInput.FocusNative' -as [type]) { return }
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;

namespace RustMcbe.OrganicInput
{
    // Padding shaped like MOUSEINPUT so the union below marshals at the size
    // SendInput requires even though this helper only sends keyboard events.
    [StructLayout(LayoutKind.Sequential)]
    public struct FocusMousePadding
    {
        public int dx; public int dy; public uint mouseData; public uint dwFlags;
        public uint time; public IntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct FocusKeyboardInput
    {
        public ushort wVk; public ushort wScan; public uint dwFlags; public uint time;
        public IntPtr dwExtraInfo;
    }

    [StructLayout(LayoutKind.Explicit)]
    public struct FocusInputUnion
    {
        [FieldOffset(0)] public FocusMousePadding mi;
        [FieldOffset(0)] public FocusKeyboardInput ki;
    }

    [StructLayout(LayoutKind.Sequential)]
    public struct FocusInput
    {
        public uint type; public FocusInputUnion u;
    }

    // Window-focus natives for the optional -Activate lane: the same legitimate
    // local automation an interactive user exercises by clicking a taskbar
    // button. None of these reads game memory, inspects packets, or bypasses
    // the driver's own GetForegroundWindow gate.
    public static class FocusNative
    {
        public const int SW_RESTORE = 9;
        public const ushort VK_MENU = 0x12;
        public const uint InputKeyboardFlag = 1;
        public const uint KeyEventFKeyUp = 0x0002;

        [DllImport("user32.dll")]
        public static extern IntPtr GetForegroundWindow();

        [DllImport("user32.dll")]
        public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint processId);

        [DllImport("user32.dll")]
        public static extern bool ShowWindow(IntPtr hWnd, int command);

        [DllImport("user32.dll")]
        public static extern bool IsIconic(IntPtr hWnd);

        [DllImport("user32.dll", SetLastError = true)]
        public static extern bool SetForegroundWindow(IntPtr hWnd);

        // GetCurrentThreadId is a kernel32 export despite living among the
        // thread-input helpers here; importing it from user32 fails with
        // EntryPointNotFound.
        [DllImport("kernel32.dll")]
        public static extern uint GetCurrentThreadId();

        [DllImport("user32.dll")]
        public static extern bool AttachThreadInput(uint attachId, uint attachToId, bool attach);

        [DllImport("user32.dll", SetLastError = true)]
        private static extern uint SendInput(uint count, FocusInput[] inputs, int size);

        // Bare Alt tap (virtual-key down then up): the documented unlock for
        // SetForegroundWindow refusals under the Windows foreground-lock model.
        // A lone Alt press types nothing and targets no window.
        public static void TapAltUnlockKey()
        {
            FocusInput[] inputs = new FocusInput[2];
            inputs[0].type = InputKeyboardFlag;
            inputs[0].u.ki.wVk = VK_MENU;
            inputs[0].u.ki.dwFlags = 0u;
            inputs[1].type = InputKeyboardFlag;
            inputs[1].u.ki.wVk = VK_MENU;
            inputs[1].u.ki.dwFlags = KeyEventFKeyUp;
            if (SendInput(2, inputs, Marshal.SizeOf(typeof(FocusInput))) != 2u)
            {
                throw new InvalidOperationException(
                    "SendInput refused the Alt unlock tap (win32Error=" +
                    Marshal.GetLastWin32Error() + ")");
            }
        }
    }
}
'@
}

# Target resolution ----------------------------------------------------------

function Get-OrganicInputTargetCandidates {
    param(
        # Exactly-one-target presence is enforced by the caller; empty siblings
        # are legal here and must not fail parameter binding.
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$ProcessName,
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$WindowTitle
    )

    $candidates = [Collections.Generic.List[object]]::new()
    if (-not [string]::IsNullOrWhiteSpace($ProcessName)) {
        foreach ($process in @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue)) {
            if ($process.MainWindowHandle -ne 0) {
                $candidates.Add([pscustomobject]@{
                    handle = [int64]$process.MainWindowHandle
                    pid    = [int]$process.Id
                    title  = [string]$process.MainWindowTitle
                })
            }
        }
        return @{
            Candidates  = $candidates
            Description = ("processes named '{0}'" -f $ProcessName)
        }
    }
    $pattern = '*' + $WindowTitle + '*'
    foreach ($process in @(Get-Process -ErrorAction SilentlyContinue)) {
        if ($process.MainWindowHandle -eq 0) { continue }
        if ($process.MainWindowTitle -like $pattern) {
            $candidates.Add([pscustomobject]@{
                handle = [int64]$process.MainWindowHandle
                pid    = [int]$process.Id
                title  = [string]$process.MainWindowTitle
            })
        }
    }
    return @{
        Candidates  = $candidates
        Description = ("windows titled like '{0}'" -f $WindowTitle)
    }
}

function Test-OrganicInputForegroundCandidate {
    # Returns the candidate that currently owns the foreground, or $null. This
    # check - not any activation result - is the driver's authoritative gate.
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Candidates,
        [Parameter(Mandatory = $true)][long]$ForegroundHandle
    )

    foreach ($candidate in $Candidates) {
        if ([int64]$candidate.handle -eq $ForegroundHandle) { return $candidate }
    }
    return $null
}

# Activation attempt ---------------------------------------------------------

function Invoke-OrganicInputActivationAttempt {
    # One bounded activation sweep across every candidate window of the
    # operator-selected target. Returns $true when the sweep observed the target
    # take the foreground; the caller still re-verifies through its own
    # GetForegroundWindow membership check before injecting anything.
    param(
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][object[]]$Candidates,
        [Parameter(Mandatory = $true)][Collections.Generic.List[string]]$Log
    )

    Initialize-OrganicInputFocusNatives
    foreach ($candidate in $Candidates) {
        $handle = [IntPtr][int64]$candidate.handle

        try {
            if ([RustMcbe.OrganicInput.FocusNative]::IsIconic($handle)) {
                $shown = [RustMcbe.OrganicInput.FocusNative]::ShowWindow(
                    $handle, [RustMcbe.OrganicInput.FocusNative]::SW_RESTORE)
                $Log.Add(("[activate] restored minimized window pid={0} handle=0x{1:x} shown={2}" -f
                    $candidate.pid, $candidate.handle, $shown))
            }
        }
        catch {
            $Log.Add(("[activate] restore check failed pid={0}: {1}" -f
                $candidate.pid, $_.Exception.Message))
        }

        # 1. Plain request. Refused whenever this process lacks foreground
        #    rights (the usual unattended-session outcome). Every mechanism is
        #    individually fault-guarded: activation is only a hint, so an
        #    unexpected native error degrades to an honest log line instead of
        #    aborting the run outside the gate's own refusal path.
        try {
            $granted = [RustMcbe.OrganicInput.FocusNative]::SetForegroundWindow($handle)
            $setError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if ($granted) {
                if ($null -ne (Test-OrganicInputForegroundCandidate -Candidates $Candidates `
                        -ForegroundHandle ([RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow().ToInt64()))) {
                    $Log.Add(("[activate] target took the foreground pid={0} handle=0x{1:x} via=set-foreground-window" -f
                        $candidate.pid, $candidate.handle))
                    return $true
                }
                $Log.Add(("[activate] SetForegroundWindow granted but the foreground check disagrees pid={0}" -f
                    $candidate.pid))
            }
            else {
                $Log.Add(("[activate] SetForegroundWindow refused pid={0} win32Error={1}; trying unlocks" -f
                    $candidate.pid, $setError))
            }
        }
        catch {
            $Log.Add(("[activate] SetForegroundWindow attempt failed pid={0}: {1}" -f
                $candidate.pid, $_.Exception.Message))
        }

        # 2. AttachThreadInput unlock: briefly share input queue state with the
        #    current foreground thread and the target thread, then retry.
        try {
            $fgHandle = [RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow()
            $attachTried = $false
            if ($fgHandle -ne [IntPtr]::Zero) {
                $fgPid = [uint32]0
                $targetPid = [uint32]0
                $fgThread = [RustMcbe.OrganicInput.FocusNative]::GetWindowThreadProcessId($fgHandle, [ref]$fgPid)
                $targetThread = [RustMcbe.OrganicInput.FocusNative]::GetWindowThreadProcessId($handle, [ref]$targetPid)
                $currentThread = [RustMcbe.OrganicInput.FocusNative]::GetCurrentThreadId()
                if ($fgThread -ne 0 -and $targetThread -ne 0 -and $currentThread -ne 0) {
                    $attachTried = $true
                    $attachedFg = [RustMcbe.OrganicInput.FocusNative]::AttachThreadInput($currentThread, $fgThread, $true)
                    $attachedTarget = [RustMcbe.OrganicInput.FocusNative]::AttachThreadInput($currentThread, $targetThread, $true)
                    $attachGranted = [RustMcbe.OrganicInput.FocusNative]::SetForegroundWindow($handle)
                    $attachError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
                    if ($attachedTarget) {
                        [void][RustMcbe.OrganicInput.FocusNative]::AttachThreadInput($currentThread, $targetThread, $false)
                    }
                    if ($attachedFg) {
                        [void][RustMcbe.OrganicInput.FocusNative]::AttachThreadInput($currentThread, $fgThread, $false)
                    }
                    if ($attachGranted -and
                        ($null -ne (Test-OrganicInputForegroundCandidate -Candidates $Candidates `
                            -ForegroundHandle ([RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow().ToInt64())))) {
                        $Log.Add(("[activate] target took the foreground pid={0} handle=0x{1:x} via=attach-thread-input" -f
                            $candidate.pid, $candidate.handle))
                        return $true
                    }
                    $Log.Add(("[activate] AttachThreadInput unlock did not take focus pid={0} granted={1} win32Error={2}" -f
                        $candidate.pid, $attachGranted, $attachError))
                }
            }
            if (-not $attachTried) {
                $Log.Add(("[activate] no foreground thread available for AttachThreadInput pid={0}" -f
                    $candidate.pid))
            }
        }
        catch {
            $Log.Add(("[activate] AttachThreadInput attempt failed pid={0}: {1}" -f
                $candidate.pid, $_.Exception.Message))
        }

        # 3. Bare Alt-keypress unlock: makes this process the most recent input
        #    recipient for the foreground-lock check. Types nothing.
        try {
            [RustMcbe.OrganicInput.FocusNative]::TapAltUnlockKey()
            $altGranted = [RustMcbe.OrganicInput.FocusNative]::SetForegroundWindow($handle)
            $altError = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            if ($altGranted -and
                ($null -ne (Test-OrganicInputForegroundCandidate -Candidates $Candidates `
                    -ForegroundHandle ([RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow().ToInt64())))) {
                $Log.Add(("[activate] target took the foreground pid={0} handle=0x{1:x} via=alt-unlock" -f
                    $candidate.pid, $candidate.handle))
                return $true
            }
            $Log.Add(("[activate] Alt-unlock did not take focus pid={0} granted={1} win32Error={2}" -f
                $candidate.pid, $altGranted, $altError))
        }
        catch {
            $Log.Add(("[activate] Alt-unlock tap failed pid={0}: {1}" -f
                $candidate.pid, $_.Exception.Message))
        }

        # 4. Last resort: shell automation's AppActivate by process id.
        try {
            $shell = New-Object -ComObject WScript.Shell
            $appActivated = [bool]$shell.AppActivate([int]$candidate.pid)
            if ($null -ne (Test-OrganicInputForegroundCandidate -Candidates $Candidates `
                    -ForegroundHandle ([RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow().ToInt64()))) {
                $Log.Add(("[activate] target took the foreground pid={0} handle=0x{1:x} via=appactivate reported={2}" -f
                    $candidate.pid, $candidate.handle, $appActivated))
                return $true
            }
            $Log.Add(("[activate] AppActivate reported {0} but the foreground check disagrees pid={1}" -f
                $appActivated, $candidate.pid))
        }
        catch {
            $Log.Add(("[activate] AppActivate unavailable pid={0}: {1}" -f
                $candidate.pid, $_.Exception.Message))
        }
    }
    return $false
}

# Foreground wait gate -------------------------------------------------------

function Wait-OrganicInputForegroundTarget {
    # Returns a structured result rather than mixing progress lines into the
    # pipeline: any Write-Output here would be captured by "$matched = ..." and
    # a non-empty message array would evaluate truthy regardless of the
    # boolean, silently bypassing the safety gate.
    param(
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$ProcessName,
        [Parameter(Mandatory = $true)][AllowEmptyString()][string]$WindowTitle,
        [Parameter(Mandatory = $true)][ValidateRange(1, 120)][int]$GraceSeconds,
        # Opt-in programmatic activation attempts while waiting. The
        # GetForegroundWindow membership re-check below stays authoritative
        # either way; without this switch the loop is byte-for-byte today's
        # manual-focus-only behavior.
        [switch]$AttemptActivation
    )

    Initialize-OrganicInputFocusNatives
    $log = [Collections.Generic.List[string]]::new()
    $target = Get-OrganicInputTargetCandidates -ProcessName $ProcessName -WindowTitle $WindowTitle
    $log.Add(("[live] waiting up to {0}s for a foreground window of {1}; focus the target window now." -f
        $GraceSeconds, $target.Description))
    if ($AttemptActivation) {
        $log.Add('[activate] programmatic activation enabled; attempting to raise a matching window')
    }
    $deadline = (Get-Date).AddSeconds($GraceSeconds)
    while ((Get-Date) -lt $deadline) {
        $target = Get-OrganicInputTargetCandidates -ProcessName $ProcessName -WindowTitle $WindowTitle
        if ($target.Candidates.Count -gt 0) {
            $foreground = [RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow().ToInt64()
            if ($AttemptActivation -and
                ($null -eq (Test-OrganicInputForegroundCandidate -Candidates @($target.Candidates) `
                    -ForegroundHandle $foreground))) {
                $null = Invoke-OrganicInputActivationAttempt -Candidates @($target.Candidates) -Log $log
            }
            # Authoritative re-check after any activation hint.
            $foreground = [RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow().ToInt64()
            $matched = Test-OrganicInputForegroundCandidate -Candidates @($target.Candidates) `
                -ForegroundHandle $foreground
            if ($null -ne $matched) {
                $log.Add(("[live] foreground target matched: pid={0} handle=0x{1:x} title='{2}'" -f
                    $matched.pid, $matched.handle, $matched.title))
                return @{
                    Matched = $true
                    Log = @($log.ToArray())
                    Handle = [int64]$matched.handle
                    ProcessId = [uint32]$matched.pid
                }
            }
        }
        Start-Sleep -Milliseconds 400
    }
    if ($AttemptActivation) {
        $log.Add('[activate] activation attempts ended without the target owning the foreground')
    }
    $log.Add(("[live] grace expired with no matching foreground window for {0}" -f
        $target.Description))
    return @{ Matched = $false; Log = @($log.ToArray()) }
}
