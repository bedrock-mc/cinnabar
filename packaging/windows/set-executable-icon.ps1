# Embed the generated original Cinnabar ICO before signing; no external resource editor.
param([Parameter(Mandatory)][string]$Executable, [Parameter(Mandatory)][string]$Icon)
$ErrorActionPreference = "Stop"
if (-not ("CinnabarPackaging.Resources" -as [type])) {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
namespace CinnabarPackaging {
    public static class Resources {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern IntPtr BeginUpdateResource(string path, bool deleteExisting);
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool UpdateResource(IntPtr handle, IntPtr type, IntPtr name,
                                                ushort language, byte[] bytes, uint size);
        [DllImport("kernel32.dll", SetLastError = true)]
        public static extern bool EndUpdateResource(IntPtr handle, bool discard);
    }
}
'@
}
$exe = (Resolve-Path -LiteralPath $Executable).Path
$bytes = [IO.File]::ReadAllBytes((Resolve-Path -LiteralPath $Icon).Path)
if ($bytes.Length -lt 6 -or [BitConverter]::ToUInt16($bytes, 0) -ne 0 -or
    [BitConverter]::ToUInt16($bytes, 2) -ne 1) { throw "Invalid ICO header" }
$count = [BitConverter]::ToUInt16($bytes, 4)
if ($count -eq 0 -or $bytes.Length -lt 6 + 16 * $count) { throw "Truncated ICO directory" }
$group = [byte[]]::new(6 + 14 * $count)
[Array]::Copy($bytes, 0, $group, 0, 6)
$images = @()
for ($index = 0; $index -lt $count; $index++) {
    $entry = 6 + 16 * $index
    $length = [BitConverter]::ToUInt32($bytes, $entry + 8)
    $offset = [BitConverter]::ToUInt32($bytes, $entry + 12)
    if ($length -eq 0 -or [uint64]$offset + $length -gt $bytes.Length) { throw "Truncated ICO image" }
    $image = [byte[]]::new($length)
    [Array]::Copy($bytes, $offset, $image, 0, $length)
    $images += ,$image
    [Array]::Copy($bytes, $entry, $group, 6 + 14 * $index, 12)
    [Array]::Copy([BitConverter]::GetBytes([uint16]($index + 1)), 0, $group, 18 + 14 * $index, 2)
}
$handle = [CinnabarPackaging.Resources]::BeginUpdateResource($exe, $false)
if ($handle -eq [IntPtr]::Zero) { throw "Cannot open executable resources: $([Runtime.InteropServices.Marshal]::GetLastWin32Error())" }
$discard = $true
try {
    for ($index = 0; $index -lt $count; $index++) {
        if (-not [CinnabarPackaging.Resources]::UpdateResource($handle, [IntPtr]3,
            [IntPtr]($index + 1), 0, $images[$index], $images[$index].Length)) { throw "Cannot embed icon image" }
    }
    if (-not [CinnabarPackaging.Resources]::UpdateResource($handle, [IntPtr]14, [IntPtr]1,
        0, $group, $group.Length)) { throw "Cannot embed icon directory" }
    $discard = $false
} finally {
    if (-not [CinnabarPackaging.Resources]::EndUpdateResource($handle, $discard)) { throw "Cannot publish executable resources" }
}
