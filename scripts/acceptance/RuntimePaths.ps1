function Assert-SafeRuntimeChild {
    param(
        [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
        [Parameter(Mandatory = $true)][string]$Candidate
    )

    $runtimeFull = [IO.Path]::GetFullPath($RuntimeDirectory).TrimEnd('\', '/')
    $candidateFull = [IO.Path]::GetFullPath($Candidate)
    $prefix = $runtimeFull + [IO.Path]::DirectorySeparatorChar
    if (-not $candidateFull.StartsWith($prefix, [StringComparison]::OrdinalIgnoreCase)) {
        throw "refusing to reset path outside stable runtime: $candidateFull"
    }
}

function Test-IsWindows {
    return [Runtime.InteropServices.RuntimeInformation]::IsOSPlatform(
        [Runtime.InteropServices.OSPlatform]::Windows
    )
}

function Remove-TrailingRuntimeSeparators {
    param([Parameter(Mandatory = $true)][string]$Path)

    $separators = [char[]]@([IO.Path]::DirectorySeparatorChar, [IO.Path]::AltDirectorySeparatorChar)
    $trimmed = $Path.TrimEnd($separators)
    $root = [IO.Path]::GetPathRoot($Path)
    if (-not [string]::IsNullOrEmpty($root)) {
        $rootWithoutSeparators = $root.TrimEnd($separators)
        $comparison = if (Test-IsWindows) {
            [StringComparison]::OrdinalIgnoreCase
        }
        else {
            [StringComparison]::Ordinal
        }
        if ($trimmed.Equals($rootWithoutSeparators, $comparison)) {
            if ($root.EndsWith([IO.Path]::DirectorySeparatorChar.ToString(), [StringComparison]::Ordinal) -or
                $root.EndsWith([IO.Path]::AltDirectorySeparatorChar.ToString(), [StringComparison]::Ordinal)) {
                return $root
            }
            return $root + [IO.Path]::DirectorySeparatorChar
        }
    }
    return $trimmed
}

function ConvertFrom-ExtendedWindowsPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ($Path.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)) {
        return '\\' + $Path.Substring(8)
    }
    if ($Path.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) {
        return $Path.Substring(4)
    }
    return $Path
}

function ConvertTo-ExtendedWindowsPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    if ($Path.StartsWith('\\?\', [StringComparison]::OrdinalIgnoreCase)) {
        return $Path
    }
    if ($Path.StartsWith('\\', [StringComparison]::Ordinal)) {
        return '\\?\UNC\' + $Path.Substring(2)
    }
    if ($Path -match '^[A-Za-z]:\\') {
        return '\\?\' + $Path
    }
    throw "cannot convert non-absolute Windows path to extended form: $Path"
}

function ConvertTo-NormalizedRuntimePath {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-IsWindows)) {
        return Remove-TrailingRuntimeSeparators -Path ([IO.Path]::GetFullPath($Path))
    }

    $usesExtendedDrive = $Path -match '^\\\\\?\\[A-Za-z]:\\'
    $usesExtendedUnc = $Path.StartsWith('\\?\UNC\', [StringComparison]::OrdinalIgnoreCase)
    if ($usesExtendedDrive -or $usesExtendedUnc) {
        $normal = ConvertFrom-ExtendedWindowsPath -Path $Path
        $normal = Remove-TrailingRuntimeSeparators -Path ([IO.Path]::GetFullPath($normal))
        return ConvertTo-ExtendedWindowsPath -Path $normal
    }

    $full = [IO.Path]::GetFullPath($Path).Replace('/', '\')
    return Remove-TrailingRuntimeSeparators -Path $full
}

function Initialize-RuntimePathNativeMethods {
    if (-not (Test-IsWindows) -or ('RustMcbe.AcceptanceRuntimePathNativeMethods' -as [type])) {
        return
    }

    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace RustMcbe {
    public static class AcceptanceRuntimePathNativeMethods {
        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern SafeFileHandle CreateFileW(
            string fileName,
            uint desiredAccess,
            uint shareMode,
            IntPtr securityAttributes,
            uint creationDisposition,
            uint flagsAndAttributes,
            IntPtr templateFile
        );

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern uint GetFinalPathNameByHandleW(
            SafeFileHandle file,
            StringBuilder path,
            uint pathLength,
            uint flags
        );

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool ReplaceFileW(
            string replacedFileName,
            string replacementFileName,
            string backupFileName,
            uint replaceFlags,
            IntPtr exclude,
            IntPtr reserved
        );
    }
}
'@
}

function Get-CanonicalExistingDirectoryPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $full = ConvertTo-NormalizedRuntimePath -Path $Path
    $item = Get-Item -LiteralPath $full -Force -ErrorAction Stop
    if (-not $item.PSIsContainer) {
        throw "path is not a directory: $full"
    }
    if (-not (Test-IsWindows)) {
        return ConvertTo-NormalizedRuntimePath -Path $item.FullName
    }

    Initialize-RuntimePathNativeMethods
    $shareAll = [uint32]0x00000007
    $openExisting = [uint32]3
    $backupSemantics = [uint32]0x02000000
    $handle = [RustMcbe.AcceptanceRuntimePathNativeMethods]::CreateFileW(
        $full,
        0,
        $shareAll,
        [IntPtr]::Zero,
        $openExisting,
        $backupSemantics,
        [IntPtr]::Zero
    )
    if ($handle.IsInvalid) {
        $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
        $handle.Dispose()
        throw [ComponentModel.Win32Exception]::new($errorCode, "open directory handle for $full")
    }
    try {
        $capacity = 512
        while ($true) {
            $buffer = [Text.StringBuilder]::new($capacity)
            $length = [RustMcbe.AcceptanceRuntimePathNativeMethods]::GetFinalPathNameByHandleW(
                $handle,
                $buffer,
                [uint32]$buffer.Capacity,
                0
            )
            if ($length -eq 0) {
                $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
                throw [ComponentModel.Win32Exception]::new($errorCode, "resolve final directory path for $full")
            }
            if ($length -lt [uint32]$buffer.Capacity) {
                return ConvertTo-NormalizedRuntimePath -Path $buffer.ToString()
            }
            if ($length -ge [int]::MaxValue) {
                throw "resolved directory path is too long: $full"
            }
            $capacity = [int]$length + 1
        }
    }
    finally {
        $handle.Dispose()
    }
}

function Get-CanonicalPathThroughExistingParent {
    param([Parameter(Mandatory = $true)][string]$Path)

    $current = ConvertTo-NormalizedRuntimePath -Path $Path
    $missing = [Collections.Generic.List[string]]::new()
    while ($true) {
        try {
            $resolved = Get-CanonicalExistingDirectoryPath -Path $current
            for ($index = $missing.Count - 1; $index -ge 0; $index--) {
                $resolved = [IO.Path]::Combine($resolved, $missing[$index])
            }
            return ConvertTo-NormalizedRuntimePath -Path $resolved
        }
        catch [Management.Automation.ItemNotFoundException], [IO.DirectoryNotFoundException], [IO.FileNotFoundException] {
            $root = Remove-TrailingRuntimeSeparators -Path ([IO.Path]::GetPathRoot($current))
            $comparison = if (Test-IsWindows) {
                [StringComparison]::OrdinalIgnoreCase
            }
            else {
                [StringComparison]::Ordinal
            }
            if ($current.Equals($root, $comparison)) {
                throw "no existing parent for stable runtime: $Path"
            }
            $leaf = [IO.Path]::GetFileName($current)
            if ([string]::IsNullOrEmpty($leaf)) {
                throw "cannot resolve stable runtime component: $current"
            }
            $missing.Add($leaf)
            $parent = [IO.Path]::GetDirectoryName($current)
            if ([string]::IsNullOrEmpty($parent)) {
                $parent = $root
            }
            $current = ConvertTo-NormalizedRuntimePath -Path $parent
        }
    }
}

function Test-RuntimePathContains {
    param(
        [Parameter(Mandatory = $true)][string]$Parent,
        [Parameter(Mandatory = $true)][string]$Candidate
    )

    $parentPath = ConvertTo-NormalizedRuntimePath -Path $Parent
    $candidatePath = ConvertTo-NormalizedRuntimePath -Path $Candidate
    # Retain the acceptance harness's pre-existing case-insensitive separation
    # rule on every platform while making the comparison segment-aware.
    $comparison = [StringComparison]::OrdinalIgnoreCase
    if ($parentPath.Equals($candidatePath, $comparison)) {
        return $true
    }
    $prefix = if ($parentPath.EndsWith([IO.Path]::DirectorySeparatorChar.ToString(), [StringComparison]::Ordinal)) {
        $parentPath
    }
    else {
        $parentPath + [IO.Path]::DirectorySeparatorChar
    }
    return $candidatePath.StartsWith($prefix, $comparison)
}

function Get-CanonicalFileTargetPath {
    param([Parameter(Mandatory = $true)][string]$Path)

    $full = ConvertTo-NormalizedRuntimePath -Path $Path
    $parent = [IO.Path]::GetDirectoryName($full)
    $leaf = [IO.Path]::GetFileName($full)
    if ([string]::IsNullOrWhiteSpace($parent) -or [string]::IsNullOrWhiteSpace($leaf)) {
        throw "invalid file target path: $Path"
    }
    $canonicalParent = Get-CanonicalPathThroughExistingParent -Path $parent
    return ConvertTo-NormalizedRuntimePath -Path ([IO.Path]::Combine($canonicalParent, $leaf))
}

function Assert-PrebuiltClientPathSafe {
    param(
        [Parameter(Mandatory = $true)][string]$ClientExecutable,
        [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [Parameter(Mandatory = $true)][string]$CoreExecutable,
        [Parameter(Mandatory = $true)][string]$MetricsOut
    )

    $clientItem = Get-Item -LiteralPath $ClientExecutable -Force -ErrorAction Stop
    if ($clientItem.PSIsContainer -or
        (($clientItem.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
        throw "prebuilt client must be a regular file: $ClientExecutable"
    }
    $client = Get-CanonicalFileTargetPath -Path $clientItem.FullName
    $runtime = Get-CanonicalPathThroughExistingParent -Path $RuntimeDirectory
    $run = Get-CanonicalPathThroughExistingParent -Path $RunDirectory
    $core = Get-CanonicalFileTargetPath -Path $CoreExecutable
    $metrics = Get-CanonicalFileTargetPath -Path $MetricsOut
    $runtimeLock = Get-CanonicalFileTargetPath -Path ($RuntimeDirectory + '.lock')
    $comparison = [StringComparison]::OrdinalIgnoreCase

    if (Test-RuntimePathContains -Parent $runtime -Candidate $client) {
        throw "prebuilt client overlaps stable BDS runtime: client=$client runtime=$runtime"
    }
    if ($client.Equals($runtimeLock, $comparison)) {
        throw "prebuilt client aliases stable BDS runtime lock: $client"
    }
    if (Test-RuntimePathContains -Parent $run -Candidate $client) {
        throw "prebuilt client overlaps acceptance run output: client=$client run=$run"
    }
    if ($client.Equals($core, $comparison)) {
        throw "prebuilt client aliases generated core executable: $client"
    }
    if ($client.Equals($metrics, $comparison)) {
        throw "prebuilt client aliases requested metrics output: $client"
    }
}

function Assert-FileHashUnchanged {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$ExpectedSha256,
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Label
    )

    if ($ExpectedSha256 -notmatch '^[0-9a-fA-F]{64}$') {
        throw "$Label expected SHA-256 was invalid: $ExpectedSha256"
    }
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "$Label disappeared: $Path"
    }
    $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $Path).Hash.ToLowerInvariant()
    if ($actual -cne $ExpectedSha256.ToLowerInvariant()) {
        throw "$Label was modified: expected=$($ExpectedSha256.ToLowerInvariant()) actual=$actual path=$Path"
    }
}

function ConvertTo-RuntimeSourceIdentity {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-IsWindows)) {
        return ConvertTo-NormalizedRuntimePath -Path $Path
    }
    $canonical = Get-CanonicalExistingDirectoryPath -Path $Path
    return $canonical.ToLowerInvariant()
}

function Get-RuntimeOwnershipMarker {
    param(
        [Parameter(Mandatory = $true)][string]$SourcePath,
        [switch]$Legacy
    )

    $identity = ConvertTo-RuntimeSourceIdentity -Path $SourcePath
    if ((Test-IsWindows) -and $Legacy) {
        $identity = ConvertFrom-ExtendedWindowsPath -Path $identity
    }
    return "rust-mcbe-bds-runtime-v1`nsource=$identity`n"
}

function Replace-AtomicRuntimeFile {
    param(
        [Parameter(Mandatory = $true)][string]$ReplacementPath,
        [Parameter(Mandatory = $true)][string]$DestinationPath
    )

    if (Test-IsWindows) {
        Initialize-RuntimePathNativeMethods
        $writeThrough = [uint32]0x00000001
        $replaced = [RustMcbe.AcceptanceRuntimePathNativeMethods]::ReplaceFileW(
            $DestinationPath,
            $ReplacementPath,
            [NullString]::Value,
            $writeThrough,
            [IntPtr]::Zero,
            [IntPtr]::Zero
        )
        if (-not $replaced) {
            $errorCode = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
            throw [ComponentModel.Win32Exception]::new(
                $errorCode,
                "replace runtime file $DestinationPath (Win32 error $errorCode)"
            )
        }
        return
    }

    $backup = $DestinationPath + (".{0}.backup" -f [guid]::NewGuid().ToString('N'))
    try {
        [IO.File]::Replace($ReplacementPath, $DestinationPath, $backup)
    }
    finally {
        if ([IO.File]::Exists($backup)) {
            [IO.File]::Delete($backup)
        }
    }
}

function Set-AtomicRuntimeOwnerMarker {
    param(
        [Parameter(Mandatory = $true)][string]$MarkerPath,
        [Parameter(Mandatory = $true)][string]$Owner
    )

    $directory = [IO.Path]::GetDirectoryName($MarkerPath)
    $temporary = [IO.Path]::Combine(
        $directory,
        (".rust-mcbe-runtime-owner-{0}.tmp" -f [guid]::NewGuid().ToString('N'))
    )
    [IO.File]::WriteAllText($temporary, $Owner, [Text.UTF8Encoding]::new($false))
    try {
        Replace-AtomicRuntimeFile -ReplacementPath $temporary -DestinationPath $MarkerPath
    }
    finally {
        if ([IO.File]::Exists($temporary)) {
            [IO.File]::Delete($temporary)
        }
    }
}

function Set-StableRuntime {
    param(
        [Parameter(Mandatory = $true)][string]$SourceDirectory,
        [Parameter(Mandatory = $true)][string]$RuntimeDirectory,
        [Parameter(Mandatory = $true)][string]$ExecutableName
    )

    $sourceFull = Get-CanonicalExistingDirectoryPath -Path $SourceDirectory
    $runtimeFull = Get-CanonicalPathThroughExistingParent -Path $RuntimeDirectory
    if ((Test-RuntimePathContains -Parent $sourceFull -Candidate $runtimeFull) -or
        (Test-RuntimePathContains -Parent $runtimeFull -Candidate $sourceFull)) {
        throw "BDS source and stable runtime overlap: source=$sourceFull runtime=$runtimeFull"
    }

    New-Item -ItemType Directory -Path $runtimeFull -Force | Out-Null
    $runtimeInfo = Get-Item -LiteralPath $runtimeFull -Force
    if (($runtimeInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
        throw "stable runtime must not be a reparse point: $runtimeFull"
    }

    $markerPath = [IO.Path]::Combine($runtimeFull, '.rust-mcbe-runtime-owner')
    $owner = Get-RuntimeOwnershipMarker -SourcePath $sourceFull
    $legacyOwner = Get-RuntimeOwnershipMarker -SourcePath $sourceFull -Legacy
    $entries = @(Get-ChildItem -LiteralPath $runtimeFull -Force)
    if (Test-Path -LiteralPath $markerPath) {
        $markerInfo = Get-Item -LiteralPath $markerPath -Force
        if ($markerInfo.PSIsContainer -or
            (($markerInfo.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0)) {
            throw "invalid stable runtime owner marker: $markerPath"
        }
        $currentOwner = [IO.File]::ReadAllText($markerPath)
        if ($currentOwner -ceq $legacyOwner -and $legacyOwner -cne $owner) {
            Set-AtomicRuntimeOwnerMarker -MarkerPath $markerPath -Owner $owner
        }
        elseif ($currentOwner -cne $owner) {
            throw "stable runtime belongs to a different BDS source: $markerPath"
        }
    }
    elseif ($entries.Count -ne 0) {
        throw "refusing unmarked non-empty stable runtime: $runtimeFull"
    }
    else {
        [IO.File]::WriteAllText($markerPath, $owner, [Text.UTF8Encoding]::new($false))
    }

    $sourceExecutable = [IO.Path]::Combine($sourceFull, $ExecutableName)
    $runtimeExecutable = [IO.Path]::Combine($runtimeFull, $ExecutableName)
    $sourceHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $sourceExecutable).Hash
    $copyExecutable = $true
    if (Test-Path -LiteralPath $runtimeExecutable) {
        $runtimeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $runtimeExecutable).Hash
        $copyExecutable = $runtimeHash -ne $sourceHash
    }
    if ($copyExecutable) {
        $temporaryExecutable = [IO.Path]::Combine($runtimeFull, ("bedrock-server-exe-{0}.tmp" -f [guid]::NewGuid().ToString('N')))
        Copy-Item -LiteralPath $sourceExecutable -Destination $temporaryExecutable
        try {
            if (Test-Path -LiteralPath $runtimeExecutable) {
                Replace-AtomicRuntimeFile `
                    -ReplacementPath $temporaryExecutable `
                    -DestinationPath $runtimeExecutable
            }
            else {
                [IO.File]::Move($temporaryExecutable, $runtimeExecutable)
            }
        }
        finally {
            if (Test-Path -LiteralPath $temporaryExecutable) {
                Remove-Item -LiteralPath $temporaryExecutable -Force
            }
        }
    }

    foreach ($entry in @(Get-ChildItem -LiteralPath $runtimeFull -Force)) {
        if ($entry.Name -eq $ExecutableName -or $entry.Name -eq '.rust-mcbe-runtime-owner') {
            continue
        }
        Assert-SafeRuntimeChild -RuntimeDirectory $runtimeFull -Candidate $entry.FullName
        Remove-Item -LiteralPath $entry.FullName -Recurse -Force
    }
    foreach ($entry in @(Get-ChildItem -LiteralPath $sourceFull -Force)) {
        if ($entry.Name -eq $ExecutableName) {
            continue
        }
        Copy-Item -LiteralPath $entry.FullName -Destination ([IO.Path]::Combine($runtimeFull, $entry.Name)) -Recurse
    }
    return $runtimeExecutable
}
