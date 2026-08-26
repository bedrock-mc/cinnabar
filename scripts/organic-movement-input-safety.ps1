<#
.SYNOPSIS
    Per-event foreground safety check for organic synthetic input.

.DESCRIPTION
    The live driver captures the target HWND and process id at its initial
    foreground gate and calls this helper immediately before every new
    SendInput event. Tests provide a deterministic identity provider;
    production uses GetForegroundWindow and GetWindowThreadProcessId directly.
#>

function Assert-OrganicInputForegroundIdentity {
    # Throws before injection when focus no longer belongs to the exact window
    # and process that passed the initial gate. Pairing PID with HWND prevents
    # a recycled handle from silently authorizing a different process. The
    # provider seam keeps this behavior deterministic without native input.
    param(
        [Parameter(Mandatory = $true)][long]$ExpectedHandle,
        [Parameter(Mandatory = $true)][uint32]$ExpectedProcessId,
        [scriptblock]$ForegroundIdentityProvider = {
            $handle = [RustMcbe.OrganicInput.FocusNative]::GetForegroundWindow()
            $processId = [uint32]0
            if ($handle -ne [IntPtr]::Zero) {
                [void][RustMcbe.OrganicInput.FocusNative]::GetWindowThreadProcessId(
                    $handle,
                    [ref]$processId
                )
            }
            return [pscustomobject]@{
                Handle = $handle.ToInt64()
                ProcessId = $processId
            }
        }
    )

    $actual = & $ForegroundIdentityProvider
    $actualHandle = [long]$actual.Handle
    $actualProcessId = [uint32]$actual.ProcessId
    if ($actualHandle -ne $ExpectedHandle -or $actualProcessId -ne $ExpectedProcessId) {
        throw ("foreground identity lost before SendInput event: expected HWND=0x{0:x} PID={1}, actual HWND=0x{2:x} PID={3}" -f
            $ExpectedHandle, $ExpectedProcessId, $actualHandle, $actualProcessId)
    }
}
