[CmdletBinding()]
param(
    [switch]$DryRun,
    [Parameter(Mandatory = $true)]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$DurationSeconds,
    [Parameter(Mandatory = $true)]
    [string]$BdsDir,
    [Parameter(Mandatory = $true)]
    [string]$MetricsOut,
    [string]$Assets,
    [ValidateSet('None', 'Front', 'Back')]
    [string]$VisualFixturePose = 'None'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$PinnedGophertunnelCommit = '9948b1729395d2e819fce28e079d4a7bfc67716c'
$PinnedValentineCommit = '6f6806e821a579c183c44d786f76d9b358a2b825'

function ConvertTo-CommandArgument {
    param([Parameter(Mandatory = $true)][string]$Value)

    if ($Value -notmatch '[\s"]') {
        return $Value
    }
    return '"' + $Value.Replace('"', '\"') + '"'
}

function Format-ResolvedCommand {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments
    )

    $parts = @((ConvertTo-CommandArgument $Executable))
    $parts += @($Arguments | ForEach-Object { ConvertTo-CommandArgument $_ })
    return $parts -join ' '
}

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

function Set-ServerProperties {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][int]$Port,
        [Parameter(Mandatory = $true)][int]$PortV6
    )

    $wanted = [ordered]@{
        'server-port' = $Port.ToString([Globalization.CultureInfo]::InvariantCulture)
        'server-portv6' = $PortV6.ToString([Globalization.CultureInfo]::InvariantCulture)
        'online-mode' = 'false'
        'allow-list' = 'false'
        'enable-lan-visibility' = 'false'
    }
    $lines = @([IO.File]::ReadAllLines($Path))
    foreach ($key in $wanted.Keys) {
        $matchingIndices = @()
        for ($index = 0; $index -lt $lines.Count; $index++) {
            if ($lines[$index] -match ('^' + [regex]::Escape($key) + '=')) {
                $matchingIndices += $index
            }
        }
        if ($matchingIndices.Count -ne 1) {
            throw "server.properties must contain exactly one $key entry"
        }
        $lines[$matchingIndices[0]] = "$key=$($wanted[$key])"
    }
    [IO.File]::WriteAllLines($Path, $lines, [Text.UTF8Encoding]::new($false))
}

function New-ReservedUdpPort {
    $client = [Net.Sockets.UdpClient]::new(0)
    [pscustomobject]@{
        Client = $client
        Port = ([Net.IPEndPoint]$client.Client.LocalEndPoint).Port
    }
}

function Invoke-CheckedBuild {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$LogPath,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory
    )

    $previousErrorAction = $ErrorActionPreference
    Push-Location -LiteralPath $WorkingDirectory
    try {
        # Windows PowerShell 5.1 represents a native process's stderr as non-terminating
        # ErrorRecords. Compilers routinely use stderr for progress even when they exit 0.
        $ErrorActionPreference = 'Continue'
        & $Executable @Arguments 2>&1 |
            ForEach-Object { $_.ToString() } |
            Tee-Object -FilePath $LogPath
        $exitCode = $LASTEXITCODE
    }
    finally {
        $ErrorActionPreference = $previousErrorAction
        Pop-Location
    }
    if ($exitCode -ne 0) {
        throw "build failed ($exitCode): $(Format-ResolvedCommand $Executable $Arguments)"
    }
}

function Start-LoggedProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Executable,
        [Parameter(Mandatory = $true)][AllowEmptyCollection()][string[]]$Arguments,
        [Parameter(Mandatory = $true)][string]$WorkingDirectory,
        [Parameter(Mandatory = $true)][string]$StdoutPath,
        [Parameter(Mandatory = $true)][string]$StderrPath
    )

    $fileOptions = [IO.FileOptions]::Asynchronous -bor [IO.FileOptions]::WriteThrough
    $stdoutStream = [IO.FileStream]::new(
        $StdoutPath,
        [IO.FileMode]::Create,
        [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite,
        1,
        $fileOptions
    )
    $stderrStream = [IO.FileStream]::new(
        $StderrPath,
        [IO.FileMode]::Create,
        [IO.FileAccess]::Write,
        [IO.FileShare]::ReadWrite,
        1,
        $fileOptions
    )
    $startInfo = [Diagnostics.ProcessStartInfo]::new()
    $startInfo.FileName = $Executable
    $startInfo.Arguments = (@($Arguments | ForEach-Object { ConvertTo-CommandArgument $_ }) -join ' ')
    $startInfo.WorkingDirectory = $WorkingDirectory
    $startInfo.UseShellExecute = $false
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.CreateNoWindow = $false
    $process = [Diagnostics.Process]::new()
    $process.StartInfo = $startInfo
    try {
        if (-not $process.Start()) {
            throw "failed to start $Executable"
        }
    }
    catch {
        $stdoutStream.Dispose()
        $stderrStream.Dispose()
        throw
    }
    return [pscustomobject]@{
        Process = $process
        StdoutPath = $StdoutPath
        StderrPath = $StderrPath
        StdoutStream = $stdoutStream
        StderrStream = $stderrStream
        StdoutCopy = $process.StandardOutput.BaseStream.CopyToAsync($stdoutStream)
        StderrCopy = $process.StandardError.BaseStream.CopyToAsync($stderrStream)
    }
}

function Wait-ProcessOutputMarker {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][string]$Marker,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds
    )

    $deadline = [DateTime]::UtcNow.AddSeconds($TimeoutSeconds)
    $offset = [long]0
    $partialLine = ''
    $buffer = [byte[]]::new(65536)
    while ([DateTime]::UtcNow -lt $deadline) {
        $reader = [IO.FileStream]::new(
            $Handle.StdoutPath,
            [IO.FileMode]::Open,
            [IO.FileAccess]::Read,
            [IO.FileShare]::ReadWrite
        )
        try {
            $null = $reader.Seek($offset, [IO.SeekOrigin]::Begin)
            while ($reader.Position -lt $reader.Length) {
                $wanted = [Math]::Min($buffer.Length, $reader.Length - $reader.Position)
                $read = $reader.Read($buffer, 0, [int]$wanted)
                if ($read -eq 0) {
                    break
                }
                $offset += $read
                $partialLine += [Text.Encoding]::UTF8.GetString($buffer, 0, $read)
                while (($newline = $partialLine.IndexOf("`n", [StringComparison]::Ordinal)) -ge 0) {
                    $line = $partialLine.Substring(0, $newline).TrimEnd("`r")
                    $partialLine = $partialLine.Substring($newline + 1)
                    if ($line.Contains($Marker)) {
                        return $line
                    }
                }
                if ($partialLine.Length -gt 131072) {
                    $partialLine = $partialLine.Substring($partialLine.Length - 131072)
                }
            }
        }
        finally {
            $reader.Dispose()
        }
        if ($Handle.Process.HasExited -and $Handle.StdoutCopy.IsCompleted) {
            break
        }
        Start-Sleep -Milliseconds 100
    }
    throw "timed out waiting for '$Marker'; process exit=$($Handle.Process.HasExited) log=$($Handle.StdoutPath)"
}

function New-VisualFixturePlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [Parameter(Mandatory = $true)]
        [ValidateSet('Front', 'Back')]
        [string]$Pose
    )

    $mx = [int]$MutationCoordinate[0]
    $my = [int]$MutationCoordinate[1]
    $mz = [int]$MutationCoordinate[2]
    $clearMin = [pscustomobject][ordered]@{ x = $mx - 24; y = $my + 1; z = $mz - 16 }
    $clearMax = [pscustomobject][ordered]@{ x = $mx + 24; y = $my + 12; z = $mz + 16 }
    $clearVolume = ($clearMax.x - $clearMin.x + 1) *
        ($clearMax.y - $clearMin.y + 1) *
        ($clearMax.z - $clearMin.z + 1)
    if ($clearVolume -gt 32768) {
        throw "visual fixture clear volume exceeds BDS fill limit: $clearVolume"
    }

    $galleryCenter = [pscustomobject][ordered]@{ x = $mx; y = $my + 3; z = $mz + 4 }
    $camera = if ($Pose -eq 'Front') {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 8; z = $mz - 14 }
    }
    else {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 4; z = $mz + 14 }
    }

    $galleryCommands = [Collections.Generic.List[string]]::new()
    $galleryCommands.Add(
        "fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air"
    )
    $galleryCommands.Add("fill $($mx - 3) $($my + 1) $($mz - 11) $($mx + 3) $($my + 1) $($mz + 3) minecraft:oak_planks")
    $galleryCommands.Add("setblock $mx $($my + 1) $mz minecraft:air")

    $blockDefinitions = @(
        [pscustomobject][ordered]@{ label = 'stone'; block = 'minecraft:stone'; x_offset = -21 },
        [pscustomobject][ordered]@{ label = 'dirt'; block = 'minecraft:dirt'; x_offset = -16 },
        [pscustomobject][ordered]@{ label = 'grass'; block = 'minecraft:grass_block'; x_offset = -11 },
        [pscustomobject][ordered]@{ label = 'oak_planks'; block = 'minecraft:oak_planks'; x_offset = -6 },
        [pscustomobject][ordered]@{ label = 'coal_ore'; block = 'minecraft:coal_ore'; x_offset = -1 },
        [pscustomobject][ordered]@{ label = 'iron_ore'; block = 'minecraft:iron_ore'; x_offset = 4 },
        [pscustomobject][ordered]@{ label = 'diamond_ore'; block = 'minecraft:diamond_ore'; x_offset = 9 },
        [pscustomobject][ordered]@{ label = 'sand'; block = 'minecraft:sand'; x_offset = 14 },
        [pscustomobject][ordered]@{ label = 'glass'; block = 'minecraft:glass'; x_offset = 19 }
    )
    $manifestBlocks = [Collections.Generic.List[object]]::new()
    foreach ($definition in $blockDefinitions) {
        $minimum = [pscustomobject][ordered]@{
            x = $mx + [int]$definition.x_offset
            y = $my + 1
            z = $mz + 5
        }
        $maximum = [pscustomobject][ordered]@{
            x = $minimum.x + 1
            y = $minimum.y + 1
            z = $minimum.z + 1
        }
        $galleryCommands.Add(
            "fill $($minimum.x) $($minimum.y) $($minimum.z) $($maximum.x) $($maximum.y) $($maximum.z) $($definition.block)"
        )
        $manifestBlocks.Add([pscustomobject][ordered]@{
            label = $definition.label
            block = $definition.block
            min = $minimum
            max = $maximum
            size = @(2, 2, 2)
        })
    }

    foreach ($xOffset in @(-9, -8, -7)) {
        $galleryCommands.Add("setblock $($mx + $xOffset) $($my + 2) $mz minecraft:oak_stairs")
    }
    foreach ($xOffset in @(7, 8, 9)) {
        $galleryCommands.Add("setblock $($mx + $xOffset) $($my + 2) $mz minecraft:glass_pane")
    }
    $galleryCommands.Add("fill $($mx - 9) $($my + 5) $($mz + 1) $($mx - 5) $($my + 5) $($mz + 1) minecraft:oak_log [`"pillar_axis`"=`"x`"]")
    $galleryCommands.Add("fill $mx $($my + 3) $($mz + 1) $mx $($my + 7) $($mz + 1) minecraft:oak_log [`"pillar_axis`"=`"y`"]")
    $galleryCommands.Add("fill $($mx + 5) $($my + 4) $($mz - 2) $($mx + 5) $($my + 4) $($mz + 2) minecraft:oak_log [`"pillar_axis`"=`"z`"]")
    $galleryCommands.Add("fill $($mx - 2) $($my + 7) $($mz - 15) $($mx + 2) $($my + 7) $($mz - 13) minecraft:glass")
    $galleryCommands.Add("fill $($mx - 2) $($my + 3) $($mz + 13) $($mx + 2) $($my + 3) $($mz + 15) minecraft:glass")

    $fenceMarker = "RUST_MCBE_TEXTURE_FIXTURE_READY_$($Pose.ToUpperInvariant())"
    $fenceCommand = "say $fenceMarker"
    $teleportCommand = "tp @a[name=RustMCBE] $($camera.x) $($camera.y) $($camera.z) facing $($galleryCenter.x) $($galleryCenter.y) $($galleryCenter.z)"
    $commands = @($galleryCommands) + @($fenceCommand, $teleportCommand)
    if ($commands.Count -gt 64) {
        throw "visual fixture command list is not bounded: $($commands.Count)"
    }

    $manifest = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-visual-fixture-v1'
        pose = $Pose
        mutation = [pscustomobject][ordered]@{ x = $mx; y = $my; z = $mz }
        clear = [pscustomobject][ordered]@{
            min = $clearMin
            max = $clearMax
            volume = $clearVolume
        }
        gallery_center = $galleryCenter
        camera = [pscustomobject][ordered]@{
            position = $camera
            target = $galleryCenter
        }
        runway = [pscustomobject][ordered]@{
            min = [pscustomobject][ordered]@{ x = $mx - 3; y = $my + 1; z = $mz - 11 }
            max = [pscustomobject][ordered]@{ x = $mx + 3; y = $my + 1; z = $mz + 3 }
            mutation_aperture = [pscustomobject][ordered]@{ x = $mx; y = $my + 1; z = $mz }
        }
        blocks = @($manifestBlocks)
        diagnostics = [pscustomobject][ordered]@{
            non_full_blocks = @('minecraft:oak_stairs', 'minecraft:glass_pane')
            log_axes = @('x', 'y', 'z')
        }
        processing_fence = $fenceMarker
        teleport_command = $teleportCommand
        settle_milliseconds = 3000
    }

    return [pscustomobject][ordered]@{
        Pose = $Pose
        GalleryCommands = @($galleryCommands)
        FenceMarker = $fenceMarker
        FenceCommand = $fenceCommand
        TeleportCommand = $teleportCommand
        Commands = $commands
        Manifest = $manifest
    }
}

function Write-BdsConsoleCommand {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Command,
        [Parameter(Mandatory = $true)][string]$LogPath
    )

    if ($Command.Length -gt 512 -or $Command.Contains("`r") -or $Command.Contains("`n")) {
        throw 'refusing unsafe BDS console command'
    }
    $Handle.Process.StandardInput.WriteLine($Command)
    $Handle.Process.StandardInput.Flush()
    [IO.File]::AppendAllText($LogPath, $Command + [Environment]::NewLine)
}

function Publish-VisualFixture {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [ValidateRange(0, 10000)][int]$SettleMilliseconds = 3000,
        [scriptblock]$WaitForFence
    )

    $consoleLogPath = Join-Path $RunDirectory 'bds.console.log'
    foreach ($command in $Plan.GalleryCommands) {
        Write-BdsConsoleCommand -Handle $Handle -Command $command -LogPath $consoleLogPath
    }
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
    if ($null -eq $WaitForFence) {
        $null = Wait-ProcessOutputMarker -Handle $Handle -Marker $Plan.FenceMarker -TimeoutSeconds 30
    }
    else {
        $null = & $WaitForFence $Handle $Plan.FenceMarker 30
    }
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand -LogPath $consoleLogPath
    if ($SettleMilliseconds -gt 0) {
        Start-Sleep -Milliseconds $SettleMilliseconds
    }

    $readyPath = Join-Path $RunDirectory 'visual-fixture-ready.json'
    $json = $Plan.Manifest | ConvertTo-Json -Depth 8
    [IO.File]::WriteAllText($readyPath, $json, [Text.UTF8Encoding]::new($false))
    Write-Output "VISUAL_FIXTURE_READY=$readyPath"
}

function Complete-ProcessLogs {
    param($Handle)
    if ($null -eq $Handle) {
        return
    }
    foreach ($copy in @($Handle.StdoutCopy, $Handle.StderrCopy)) {
        try {
            if (-not $copy.Wait(10000)) {
                Write-Warning 'timed out flushing a child log stream'
            }
        }
        catch {
            Write-Warning "child log stream failed: $_"
        }
    }
    foreach ($stream in @($Handle.StdoutStream, $Handle.StderrStream)) {
        try {
            $stream.Flush($true)
        }
        finally {
            $stream.Dispose()
        }
    }
}

function Stop-BoundedProcess {
    param(
        $Handle,
        [Parameter(Mandatory = $true)][ValidateSet('app', 'core', 'bds')][string]$Kind,
        [string]$BdsConsoleLogPath
    )

    if ($null -eq $Handle -or $Handle.Process.HasExited) {
        return
    }
    if ($Kind -eq 'bds') {
        try {
            try {
                Write-BdsConsoleCommand `
                    -Handle $Handle `
                    -Command 'stop' `
                    -LogPath $BdsConsoleLogPath
            }
            finally {
                $Handle.Process.StandardInput.Close()
            }
        }
        catch {
            Write-Warning "BDS graceful stop failed: $_"
        }
        $timeout = 20000
    }
    elseif ($Kind -eq 'core') {
        try {
            $Handle.Process.StandardInput.Close()
        }
        catch {
            Write-Warning "core EOF stop failed: $_"
        }
        $timeout = 10000
    }
    else {
        try {
            $null = $Handle.Process.CloseMainWindow()
        }
        catch {
            Write-Warning "app close request failed: $_"
        }
        $timeout = 10000
    }
    try {
        $exited = $Handle.Process.WaitForExit($timeout)
    }
    catch {
        if ($Handle.Process.HasExited) {
            return
        }
        throw
    }
    if (-not $exited) {
        Write-Warning "$Kind did not stop gracefully in $timeout ms; forcing termination"
        try {
            $Handle.Process.Kill()
        }
        catch {
            if (-not $Handle.Process.HasExited) {
                throw
            }
        }
        if (-not $Handle.Process.WaitForExit(10000)) {
            throw "$Kind remained alive after forced termination"
        }
    }
}

function Get-OptionalCimValue {
    param([string]$ClassName, [string]$Property)
    try {
        return @((Get-CimInstance -ClassName $ClassName -ErrorAction Stop) | ForEach-Object { $_.$Property })
    }
    catch {
        return @("unavailable: $($_.Exception.Message)")
    }
}

function Assert-AcceptanceMetrics {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "app did not write acceptance metrics: $Path"
    }
    $metrics = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    $required = @(
        'session_seconds', 'world_ready', 'requested_radius_chunks', 'received_radius_chunks',
        'publisher_radius_chunks', 'mutation_coordinate', 'visible_mutation_count', 'frame_count',
        'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms', 'max_decode_ms',
        'max_mesh_ms', 'max_remesh_ms', 'max_mutation_to_visible_ms', 'decode_error_count',
        'rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks',
        'peak_admitted_world_events', 'peak_admitted_heavy_events', 'peak_queued_decode_jobs',
        'peak_in_flight_decode_jobs', 'peak_completed_decode_results', 'peak_pending_retry_requests',
        'peak_outbound_requests', 'peak_pending_mesh_jobs', 'peak_in_flight_mesh_jobs',
        'gpu_upload_bytes'
    )
    foreach ($field in $required) {
        if ($null -eq $metrics.PSObject.Properties[$field]) {
            throw "acceptance metrics are missing $field"
        }
    }
    if ([double]$metrics.session_seconds -lt $DurationSeconds) {
        throw "session_seconds=$($metrics.session_seconds), expected at least $DurationSeconds"
    }
    if (-not [bool]$metrics.world_ready) {
        throw 'world_ready was false'
    }
    if ([int]$metrics.requested_radius_chunks -ne 16 -or
        [int]$metrics.received_radius_chunks -ne 16 -or
        [int]$metrics.publisher_radius_chunks -ne 16) {
        throw "radius gate failed: requested=$($metrics.requested_radius_chunks) received=$($metrics.received_radius_chunks) publisher=$($metrics.publisher_radius_chunks)"
    }
    if ([uint64]$metrics.frame_count -eq 0) {
        throw 'frame_count was zero'
    }
    $p99 = [double]$metrics.p99_frame_ms
    if ([double]::IsNaN($p99) -or [double]::IsInfinity($p99)) {
        throw "p99_frame_ms was not finite: $($metrics.p99_frame_ms)"
    }
    if ([uint64]$metrics.decode_error_count -ne 0) {
        throw "decode_error_count=$($metrics.decode_error_count), expected zero"
    }
    foreach ($field in @('rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks', 'visible_mutation_count')) {
        if ([uint64]$metrics.$field -eq 0) {
            throw "$field was zero"
        }
    }
    if ([double]$metrics.max_mutation_to_visible_ms -gt 100.0) {
        throw "max_mutation_to_visible_ms=$($metrics.max_mutation_to_visible_ms), expected <= 100"
    }
    return $metrics
}

if ($env:RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY -eq '1') {
    return
}

if ($DurationSeconds -lt 60) {
    throw 'DurationSeconds must be at least 60'
}
if ([string]::IsNullOrWhiteSpace($MetricsOut)) {
    throw 'MetricsOut must not be empty'
}
if (-not (Test-Path -LiteralPath $BdsDir -PathType Container)) {
    throw "BDS directory does not exist: $BdsDir"
}
$BdsDir = (Resolve-Path -LiteralPath $BdsDir).Path
if ($PSBoundParameters.ContainsKey('Assets')) {
    if (-not (Test-Path -LiteralPath $Assets -PathType Leaf)) {
        throw "assets file does not exist: $Assets"
    }
    $Assets = (Resolve-Path -LiteralPath $Assets).Path
}
$BdsExecutableName = 'bedrock_server.exe'
$BdsSourceExecutable = Join-Path $BdsDir $BdsExecutableName
if (-not (Test-Path -LiteralPath $BdsSourceExecutable -PathType Leaf)) {
    throw "BDS executable does not exist: $BdsSourceExecutable"
}

$ProjectRoot = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$MetricsOut = [IO.Path]::GetFullPath($MetricsOut)
$RuntimeDirectory = Join-Path (Join-Path $ProjectRoot '.local\bds-runtime') (Split-Path -Leaf $BdsDir)
$RunName = if ($DryRun) { 'dry-run' } else { "{0}-{1}" -f [DateTime]::UtcNow.ToString('yyyyMMddTHHmmssZ'), $PID }
$RunDirectory = Join-Path (Join-Path $ProjectRoot '.local\acceptance') $RunName
$SocketDirectory = Join-Path $RunDirectory 'socket'
$CanonicalMetrics = Join-Path $RunDirectory 'app-metrics.json'
$BdsExecutable = Join-Path $RuntimeDirectory $BdsExecutableName
$CoreExecutable = Join-Path $ProjectRoot 'target\release\bedrock-core.exe'
$AppExecutable = Join-Path $ProjectRoot 'target\release\bedrock-client.exe'
$Upstream = '127.0.0.1:19132'
$BdsArguments = @()
$CoreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Upstream)
$AppArguments = @(
    '--socket-dir', $SocketDirectory,
    '--acceptance-seconds', $DurationSeconds.ToString([Globalization.CultureInfo]::InvariantCulture),
    '--metrics-out', $CanonicalMetrics
)
if ($PSBoundParameters.ContainsKey('Assets')) {
    $AppArguments += @('--assets', $Assets)
}
if ($VisualFixturePose -eq 'None') {
    $AppArguments += '--auto-fly'
}
$AppArguments += '--no-vsync'
$BdsCommand = Format-ResolvedCommand $BdsExecutable $BdsArguments
$CoreCommand = Format-ResolvedCommand $CoreExecutable $CoreArguments
$AppCommand = Format-ResolvedCommand $AppExecutable $AppArguments

if ($DryRun) {
    Write-Output "BDS_COMMAND=$BdsCommand"
    Write-Output "CORE_COMMAND=$CoreCommand"
    Write-Output "APP_COMMAND=$AppCommand"
    if ($VisualFixturePose -ne 'None') {
        Write-Output "VISUAL_FIXTURE_POSE=$VisualFixturePose"
    }
    exit 0
}

$lease = $null
$portReservation = $null
$portV6Reservation = $null
$bdsHandle = $null
$coreHandle = $null
$appHandle = $null
$runFailure = $null
$metadata = $null

try {
    New-Item -ItemType Directory -Path $RunDirectory -Force | Out-Null

    $repoCommit = (& git -C $ProjectRoot rev-parse HEAD).Trim()
    if ($LASTEXITCODE -ne 0) {
        throw 'failed to resolve repository commit'
    }
    $metadata = [ordered]@{
        status = 'preparing'
        started_utc = [DateTime]::UtcNow.ToString('o')
        repo_commit = $repoCommit
        pinned_gophertunnel_commit = $PinnedGophertunnelCommit
        pinned_valentine_commit = $PinnedValentineCommit
        bds_source = $BdsDir
        bds_sha256 = (Get-FileHash -Algorithm SHA256 -LiteralPath $BdsSourceExecutable).Hash.ToLowerInvariant()
        duration_seconds = $DurationSeconds
        build_app_command = 'cargo build --release -p bedrock-client --locked'
        build_core_command = Format-ResolvedCommand 'go' @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core')
        bds_command = $BdsCommand
        core_command = $CoreCommand
        app_command = $AppCommand
        machine = $env:COMPUTERNAME
        operating_system = Get-OptionalCimValue 'Win32_OperatingSystem' 'Caption'
        cpu = Get-OptionalCimValue 'Win32_Processor' 'Name'
        gpu = Get-OptionalCimValue 'Win32_VideoController' 'Name'
        display = Get-OptionalCimValue 'Win32_VideoController' 'VideoModeDescription'
    }
    if ($PSBoundParameters.ContainsKey('Assets')) {
        $metadata['assets'] = $Assets
    }
    if ($VisualFixturePose -ne 'None') {
        $metadata['visual_fixture_pose'] = $VisualFixturePose
    }
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    New-Item -ItemType Directory -Path (Split-Path -Parent $RuntimeDirectory) -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null

    $lockPath = $RuntimeDirectory + '.lock'
    $lease = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $BdsExecutable = Set-StableRuntime -SourceDirectory $BdsDir -RuntimeDirectory $RuntimeDirectory -ExecutableName $BdsExecutableName

    $portReservation = New-ReservedUdpPort
    $portV6Reservation = New-ReservedUdpPort
    $Upstream = "127.0.0.1:$($portReservation.Port)"
    Set-ServerProperties -Path (Join-Path $RuntimeDirectory 'server.properties') -Port $portReservation.Port -PortV6 $portV6Reservation.Port
    $CoreArguments = @('-socket-dir', $SocketDirectory, '-upstream', $Upstream)
    $BdsCommand = Format-ResolvedCommand $BdsExecutable $BdsArguments
    $CoreCommand = Format-ResolvedCommand $CoreExecutable $CoreArguments
    $AppCommand = Format-ResolvedCommand $AppExecutable $AppArguments
    Write-Output "BDS_COMMAND=$BdsCommand"
    Write-Output "CORE_COMMAND=$CoreCommand"
    Write-Output "APP_COMMAND=$AppCommand"

    $metadata['status'] = 'building'
    $metadata['bds_command'] = $BdsCommand
    $metadata['core_command'] = $CoreCommand
    $metadata['app_command'] = $AppCommand
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    Invoke-CheckedBuild -Executable 'cargo' -Arguments @('build', '--release', '-p', 'bedrock-client', '--locked') -LogPath (Join-Path $RunDirectory 'build-app.log') -WorkingDirectory $ProjectRoot
    Invoke-CheckedBuild -Executable 'go' -Arguments @('build', '-trimpath', '-o', $CoreExecutable, './core/cmd/bedrock-core') -LogPath (Join-Path $RunDirectory 'build-core.log') -WorkingDirectory $ProjectRoot

    $metadata['status'] = 'launching'
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    $portReservation.Client.Close()
    $portReservation = $null
    $portV6Reservation.Client.Close()
    $portV6Reservation = $null

    $bdsHandle = Start-LoggedProcess -Executable $BdsExecutable -Arguments $BdsArguments -WorkingDirectory $RuntimeDirectory -StdoutPath (Join-Path $RunDirectory 'bds.stdout.log') -StderrPath (Join-Path $RunDirectory 'bds.stderr.log')
    $null = Wait-ProcessOutputMarker -Handle $bdsHandle -Marker 'Server started.' -TimeoutSeconds 120

    $coreHandle = Start-LoggedProcess -Executable $CoreExecutable -Arguments $CoreArguments -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'core.stdout.log') -StderrPath (Join-Path $RunDirectory 'core.stderr.log')
    $endpointPath = Join-Path $SocketDirectory 'game.addr'
    $endpointDeadline = [DateTime]::UtcNow.AddSeconds(30)
    while (-not (Test-Path -LiteralPath $endpointPath -PathType Leaf)) {
        if ($coreHandle.Process.HasExited) {
            throw "core exited before endpoint publication with code $($coreHandle.Process.ExitCode)"
        }
        if ([DateTime]::UtcNow -ge $endpointDeadline) {
            throw "timed out waiting for core endpoint: $endpointPath"
        }
        Start-Sleep -Milliseconds 100
    }

    $appHandle = Start-LoggedProcess -Executable $AppExecutable -Arguments $AppArguments -WorkingDirectory $ProjectRoot -StdoutPath (Join-Path $RunDirectory 'app.stdout.log') -StderrPath (Join-Path $RunDirectory 'app.stderr.log')
    $coordinateMarker = Wait-ProcessOutputMarker -Handle $appHandle -Marker 'RUST_MCBE_MUTATION_COORDINATE=' -TimeoutSeconds 180
    $null = Wait-ProcessOutputMarker -Handle $appHandle -Marker 'RUST_MCBE_WORLD_READY ' -TimeoutSeconds 180

    if ($coordinateMarker -notmatch '^RUST_MCBE_MUTATION_COORDINATE=(-?\d+),(-?\d+),(-?\d+)$') {
        throw "invalid mutation marker: $coordinateMarker"
    }
    $coordinate = @([int]$Matches[1], [int]$Matches[2], [int]$Matches[3])
    if ($VisualFixturePose -ne 'None') {
        $fixturePlan = New-VisualFixturePlan -MutationCoordinate $coordinate -Pose $VisualFixturePose
        Publish-VisualFixture -Handle $bdsHandle -Plan $fixturePlan -RunDirectory $RunDirectory
    }
    $blocks = @('minecraft:gold_block', 'minecraft:diamond_block')
    $blockIndex = 0
    $nextMutation = [DateTime]::UtcNow
    $appDeadline = [DateTime]::UtcNow.AddSeconds($DurationSeconds + 90)
    while (-not $appHandle.Process.HasExited) {
        if ([DateTime]::UtcNow -ge $appDeadline) {
            throw "app exceeded acceptance deadline of $($DurationSeconds + 90) seconds"
        }
        if ([DateTime]::UtcNow -ge $nextMutation) {
            $command = "setblock $($coordinate[0]) $($coordinate[1]) $($coordinate[2]) $($blocks[$blockIndex])"
            Write-BdsConsoleCommand `
                -Handle $bdsHandle `
                -Command $command `
                -LogPath (Join-Path $RunDirectory 'bds.console.log')
            $blockIndex = ($blockIndex + 1) % $blocks.Count
            $nextMutation = [DateTime]::UtcNow.AddSeconds(2)
        }
        Start-Sleep -Milliseconds 100
    }
    if ($appHandle.Process.ExitCode -ne 0) {
        throw "app exited with code $($appHandle.Process.ExitCode)"
    }

    $metrics = Assert-AcceptanceMetrics -Path $CanonicalMetrics
    $metrics | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'validated-metrics.json') -Encoding UTF8
    $metadata['status'] = 'passed'
    $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
    Write-Output "ACCEPTANCE_ARTIFACTS=$RunDirectory"
    Write-Output "ACCEPTANCE_P99_FRAME_MS=$($metrics.p99_frame_ms)"
}
catch {
    $runFailure = $_
    if ($null -ne $metadata) {
        try {
            $metadata['status'] = 'failed'
            $metadata['failure'] = $_.Exception.Message
            $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
            $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
        }
        catch {
            Write-Warning "failed to update failure metadata: $_"
        }
    }
    throw
}
finally {
    $cleanupErrors = [Collections.Generic.List[string]]::new()
    foreach ($child in @(
        [pscustomobject]@{ Handle = $appHandle; Kind = 'app' },
        [pscustomobject]@{ Handle = $coreHandle; Kind = 'core' },
        [pscustomobject]@{ Handle = $bdsHandle; Kind = 'bds' }
    )) {
        try {
            Stop-BoundedProcess `
                -Handle $child.Handle `
                -Kind $child.Kind `
                -BdsConsoleLogPath (Join-Path $RunDirectory 'bds.console.log')
        }
        catch {
            $cleanupErrors.Add("stop $($child.Kind): $($_.Exception.Message)")
            Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
        }
    }
    foreach ($handle in @($appHandle, $coreHandle, $bdsHandle)) {
        try {
            Complete-ProcessLogs $handle
        }
        catch {
            $cleanupErrors.Add("complete child logs: $($_.Exception.Message)")
            Write-Warning $cleanupErrors[$cleanupErrors.Count - 1]
        }
    }
    foreach ($reservation in @($portReservation, $portV6Reservation)) {
        if ($null -ne $reservation) {
            try {
                $reservation.Client.Close()
            }
            catch {
                $cleanupErrors.Add("close UDP reservation: $($_.Exception.Message)")
            }
        }
    }
    if ($null -ne $lease) {
        try {
            $lease.Dispose()
        }
        catch {
            $cleanupErrors.Add("release BDS runtime lease: $($_.Exception.Message)")
        }
    }
    try {
        if (Test-Path -LiteralPath $CanonicalMetrics -PathType Leaf) {
            New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null
            Copy-Item -LiteralPath $CanonicalMetrics -Destination $MetricsOut -Force
        }
    }
    catch {
        $cleanupErrors.Add("copy requested metrics: $($_.Exception.Message)")
    }
    try {
        if (($null -ne $runFailure -or $cleanupErrors.Count -ne 0) -and (Test-Path -LiteralPath $RunDirectory -PathType Container)) {
            $failureDetails = @()
            if ($null -ne $runFailure) {
                $failureDetails += ($runFailure | Out-String)
            }
            $failureDetails += $cleanupErrors
            ($failureDetails -join [Environment]::NewLine) | Set-Content -LiteralPath (Join-Path $RunDirectory 'failure.txt') -Encoding UTF8
        }
    }
    catch {
        Write-Warning "failed to write cleanup report: $_"
    }
    if ($null -eq $runFailure -and $cleanupErrors.Count -ne 0) {
        if ($null -ne $metadata) {
            try {
                $metadata['status'] = 'failed'
                $metadata['failure'] = "cleanup: $($cleanupErrors -join '; ')"
                $metadata['completed_utc'] = [DateTime]::UtcNow.ToString('o')
                $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
            }
            catch {
                Write-Warning "failed to update cleanup-failure metadata: $_"
            }
        }
        throw "acceptance cleanup failed: $($cleanupErrors -join '; ')"
    }
}
