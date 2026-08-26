<#
.SYNOPSIS
    Per-event foreground safety check for organic synthetic input.

.DESCRIPTION
    The live driver captures the target HWND after its initial foreground gate
    and calls this helper immediately before every new SendInput event. Tests
    provide a deterministic handle provider; production uses
    GetForegroundWindow directly.
#>

function Assert-OrganicInputForegroundHandle {
    # Throws before injection when focus no longer belongs to the exact window
    # that passed the initial gate. The provider seam keeps focus-loss behavior
    # deterministic without sending native input in tests.
    param(
        [Parameter(Mandatory = $true)][long]$ExpectedHandle,
        [scriptblock]$ForegroundHandleProvider = {
            [RustMcbe.OrganicInput.NativeMethods]::GetForegroundWindow().ToInt64()
        }
    )

    $actualHandle = [long](& $ForegroundHandleProvider)
    if ($actualHandle -ne $ExpectedHandle) {
        throw ("foreground focus lost before SendInput event: expected HWND=0x{0:x}, actual HWND=0x{1:x}" -f
            $ExpectedHandle, $actualHandle)
    }
}
