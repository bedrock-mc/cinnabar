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
    [string]$VisualFixturePose = 'None',
    [switch]$FullViewTeleportGate
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

function Test-RakNetUnconnectedPong {
    param(
        [Parameter(Mandatory = $true)][string]$Address,
        [Parameter(Mandatory = $true)][ValidateRange(1, 65535)][int]$Port,
        [Parameter(Mandatory = $true)][ValidateRange(1, 5000)][int]$TimeoutMilliseconds
    )

    $magic = [byte[]]@(
        0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe,
        0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78
    )
    $probe = [byte[]]::new(33)
    $probe[0] = 0x01
    $sentAt = [BitConverter]::GetBytes([DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds())
    $guid = [BitConverter]::GetBytes([DateTime]::UtcNow.Ticks)
    if ([BitConverter]::IsLittleEndian) {
        [Array]::Reverse($sentAt)
        [Array]::Reverse($guid)
    }
    [Array]::Copy($sentAt, 0, $probe, 1, $sentAt.Length)
    [Array]::Copy($magic, 0, $probe, 9, $magic.Length)
    [Array]::Copy($guid, 0, $probe, 25, $guid.Length)

    $client = [Net.Sockets.UdpClient]::new()
    try {
        $client.Client.ReceiveTimeout = $TimeoutMilliseconds
        $client.Connect($Address, $Port)
        $null = $client.Send($probe, $probe.Length)
        $remote = [Net.IPEndPoint]::new([Net.IPAddress]::Any, 0)
        $response = $client.Receive([ref]$remote)
        if ($response.Length -lt 33 -or $response[0] -ne 0x1c) {
            return $false
        }
        for ($index = 0; $index -lt $magic.Length; $index++) {
            if ($response[17 + $index] -ne $magic[$index]) {
                return $false
            }
        }
        return $true
    }
    catch [Net.Sockets.SocketException] {
        return $false
    }
    finally {
        $client.Dispose()
    }
}

function Wait-ProcessOutputMarker {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][string]$Marker,
        [Parameter(Mandatory = $true)][int]$TimeoutSeconds,
        [scriptblock]$ReadinessProbe
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
        if (-not $Handle.Process.HasExited -and
            $null -ne $ReadinessProbe -and
            (& $ReadinessProbe)) {
            return $Marker
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
        [pscustomobject][ordered]@{ x = $mx; y = $my + 12; z = $mz - 24 }
    }
    else {
        [pscustomobject][ordered]@{ x = $mx; y = $my + 10; z = $mz + 32 }
    }

    $galleryCommands = [Collections.Generic.List[string]]::new()
    $galleryCommands.Add(
        "fill $($clearMin.x) $($clearMin.y) $($clearMin.z) $($clearMax.x) $($clearMax.y) $($clearMax.z) minecraft:air"
    )
    $galleryCommands.Add("fill $($mx - 3) $($my + 1) $($mz - 11) $($mx + 3) $($my + 1) $($mz + 3) minecraft:oak_planks")
    $galleryCommands.Add("setblock $mx $($my + 1) $mz minecraft:air")
    $galleryCommands.Add("fill $($mx + 14) $my $($mz + 5) $($mx + 15) $my $($mz + 6) minecraft:stone")

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

    $fenceMarker = 'players online:'
    $fenceCommand = 'list'
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
        processing_fence = [pscustomobject][ordered]@{
            command = $fenceCommand
            stdout_marker = $fenceMarker
        }
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

function New-FullViewTeleportPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate
    )

    $offsetChunks = 65
    $offsetBlocks = $offsetChunks * 16
    $target = [pscustomobject][ordered]@{
        x = [int]$MutationCoordinate[0] + $offsetBlocks
        y = [int]$MutationCoordinate[1] + 12
        z = [int]$MutationCoordinate[2] + $offsetBlocks
    }
    $fenceCommand = 'list'
    $fenceMarker = 'players online:'
    $teleportCommand = "tp @a[name=RustMCBE] $($target.x) $($target.y) $($target.z) facing $($target.x) $($target.y) $($target.z + 1)"
    return [pscustomobject][ordered]@{
        Target = $target
        OffsetChunks = $offsetChunks
        FenceCommand = $fenceCommand
        FenceMarker = $fenceMarker
        TeleportCommand = $teleportCommand
        Manifest = [pscustomobject][ordered]@{
            schema = 'rust-mcbe-full-view-teleport-v1'
            origin = [pscustomobject][ordered]@{
                x = [int]$MutationCoordinate[0]
                y = [int]$MutationCoordinate[1]
                z = [int]$MutationCoordinate[2]
            }
            target = $target
            offset_chunks = $offsetChunks
            radius_chunks = 16
            teleport_command = $teleportCommand
        }
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

function Publish-FullViewTeleport {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RunDirectory
    )

    $consoleLogPath = Join-Path $RunDirectory 'bds.console.log'
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
    $null = Wait-ProcessOutputMarker -Handle $Handle -Marker $Plan.FenceMarker -TimeoutSeconds 30
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand -LogPath $consoleLogPath

    $planPath = Join-Path $RunDirectory 'full-view-teleport-plan.json'
    $json = $Plan.Manifest | ConvertTo-Json -Depth 6
    [IO.File]::WriteAllText($planPath, $json, [Text.UTF8Encoding]::new($false))
    Write-Output "FULL_VIEW_TELEPORT_PLAN=$planPath"
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

function Get-SteadyResourceSummary {
    param([Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][object[]]$Samples)

    $rssValues = @($Samples | ForEach-Object { [uint64]$_.combined_rss_bytes })
    $cpuValues = @($Samples | ForEach-Object { [double]$_.cpu_percent } | Sort-Object)
    $p95Index = [Math]::Ceiling(($cpuValues.Count - 1) * 0.95)
    return [pscustomobject][ordered]@{
        sample_count = $Samples.Count
        max_combined_rss_bytes = [uint64](($rssValues | Measure-Object -Maximum).Maximum)
        mean_cpu_percent = [double](($cpuValues | Measure-Object -Average).Average)
        p95_cpu_percent = [double]$cpuValues[$p95Index]
    }
}

function ConvertFrom-FullViewSettleMarker {
    param(
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][string]$Line,
        [Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind
    )

    $prefix = if ($Kind -ceq 'Teleport') {
        'RUST_MCBE_TELEPORT_SETTLED'
    }
    else {
        'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED'
    }
    if (-not $Line.StartsWith($prefix + ' ', [StringComparison]::Ordinal)) {
        throw "invalid $Kind settle marker prefix: $Line"
    }

    $values = [ordered]@{}
    foreach ($token in $Line.Substring($prefix.Length + 1).Split(
        [char[]]@(' '),
        [StringSplitOptions]::RemoveEmptyEntries
    )) {
        if ($token -notmatch '^(?<key>[a-z][a-z0-9_]*)=(?<value>\S+)$') {
            throw "invalid $Kind settle marker token: $token"
        }
        $key = $Matches['key']
        $text = $Matches['value']
        if ($values.Contains($key)) {
            throw "duplicate $Kind settle marker field: $key"
        }
        if ($text -ceq 'null') {
            $values[$key] = $null
            continue
        }
        if ($key -ceq 'ms' -or $key.EndsWith('_ms', [StringComparison]::Ordinal)) {
            $number = 0.0
            if (-not [double]::TryParse(
                $text,
                [Globalization.NumberStyles]::Float,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$number
            ) -or [double]::IsNaN($number) -or [double]::IsInfinity($number)) {
                throw "invalid $Kind settle marker number for ${key}: $text"
            }
            $values[$key] = $number
            continue
        }
        if ($key.EndsWith('_hash', [StringComparison]::Ordinal)) {
            if ($text -notmatch '^[0-9a-fA-F]{16}$') {
                throw "invalid $Kind settle marker hash for ${key}: $text"
            }
            $values[$key] = $text.ToLowerInvariant()
            continue
        }
        if ($text -match '^\d+$') {
            $number = [uint64]0
            if (-not [uint64]::TryParse(
                $text,
                [Globalization.NumberStyles]::None,
                [Globalization.CultureInfo]::InvariantCulture,
                [ref]$number
            )) {
                throw "invalid $Kind settle marker integer for ${key}: $text"
            }
            $values[$key] = $number
            continue
        }
        $values[$key] = $text
    }
    foreach ($required in @('target', 'ms')) {
        if (-not $values.Contains($required) -or $null -eq $values[$required]) {
            throw "$Kind settle marker is missing $required"
        }
    }
    return [pscustomobject]$values
}

function Get-RequiredEvidenceProperty {
    param(
        [Parameter(Mandatory = $true)]$Evidence,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][string]$Label,
        [switch]$AllowNull
    )

    $property = $Evidence.PSObject.Properties[$Name]
    if ($null -eq $property -or (-not $AllowNull -and $null -eq $property.Value)) {
        throw "$Label.$Name was missing"
    }
    return $property.Value
}

function ConvertTo-EvidenceUInt64 {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Field
    )

    $text = [Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture)
    $number = [uint64]0
    if ($text -notmatch '^\d+$' -or -not [uint64]::TryParse(
        $text,
        [Globalization.NumberStyles]::None,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$number
    )) {
        throw "$Field was not an unsigned integer: $text"
    }
    return $number
}

function ConvertTo-EvidenceDouble {
    param(
        [Parameter(Mandatory = $true)]$Value,
        [Parameter(Mandatory = $true)][string]$Field
    )

    $number = 0.0
    $text = [Convert]::ToString($Value, [Globalization.CultureInfo]::InvariantCulture)
    if (-not [double]::TryParse(
        $text,
        [Globalization.NumberStyles]::Float,
        [Globalization.CultureInfo]::InvariantCulture,
        [ref]$number
    ) -or [double]::IsNaN($number) -or [double]::IsInfinity($number)) {
        throw "$Field was not finite: $text"
    }
    return $number
}

function Get-FullViewProofFieldNames {
    param([Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind)

    $fields = @(
        'target', 'committed', 'ms', 'view_generation', 'render_ready_ms',
        'first_frame_sequence', 'stable_frame_sequence',
        'first_present_ms', 'first_gpu_ms', 'stable_present_ms', 'stable_gpu_ms', 'frame_count',
        'expected_manifest_count', 'expected_manifest_hash',
        'first_presented_manifest_count', 'first_presented_manifest_hash',
        'stable_presented_manifest_count', 'stable_presented_manifest_hash',
        'expected', 'loaded_target', 'missing_target', 'foreign_loaded', 'foreign_requested',
        'foreign_resident', 'source_leftover', 'resident_count', 'resident_hash',
        'known_air_count', 'known_air_hash', 'missing_target_instances',
        'unexpected_target_instances', 'source_instances', 'foreign_instances',
        'stale_generation_instances', 'orphan_allocations'
    )
    if ($Kind -ceq 'Teleport') {
        $fields += @(
            'publisher_ms', 'first_level_ms', 'last_level_ms', 'level_events',
            'first_sub_ms', 'last_sub_ms', 'sub_events'
        )
    }
    return $fields
}

function Assert-OptionalStageOffset {
    param(
        [Parameter(Mandatory = $true)]$Proof,
        [Parameter(Mandatory = $true)][string]$Field,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][double]$Maximum
    )

    $value = Get-RequiredEvidenceProperty -Evidence $Proof -Name $Field -Label $Label -AllowNull
    if ($null -eq $value) {
        return $null
    }
    try {
        $number = ConvertTo-EvidenceDouble -Value $value -Field "$Label.$Field"
    }
    catch {
        throw "$Label.$Field must be JSON null or a nonnegative finite value: $value"
    }
    if ($number -lt 0.0 -or $number -gt $Maximum) {
        throw "$Label.$Field must be JSON null or a nonnegative finite value at or before ${Maximum}ms: $value"
    }
    return $number
}

function Assert-ExactFullViewProof {
    param(
        [Parameter(Mandatory = $true)]$Proof,
        [Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind,
        [Parameter(Mandatory = $true)][string]$Label,
        [Parameter(Mandatory = $true)][string]$ExpectedTargetCohort
    )

    foreach ($field in Get-FullViewProofFieldNames -Kind $Kind) {
        $allowNull = $Kind -ceq 'Teleport' -and $field -in @(
            'publisher_ms', 'first_level_ms', 'last_level_ms', 'first_sub_ms', 'last_sub_ms'
        )
        $null = Get-RequiredEvidenceProperty `
            -Evidence $Proof `
            -Name $field `
            -Label $Label `
            -AllowNull:$allowNull
    }

    $target = [string]$Proof.target
    if ($target -notmatch '^-?\d+:-?\d+:-?\d+:16$') {
        throw "$Label.target was not a radius-16 cohort: $target"
    }
    if ($target -cne $ExpectedTargetCohort) {
        throw "$Label target cohort mismatch: expected=$ExpectedTargetCohort actual=$target"
    }
    if ([string]$Proof.committed -cne $target) {
        throw "$Label committed cohort did not equal target: committed=$($Proof.committed) target=$target"
    }

    $latency = ConvertTo-EvidenceDouble -Value $Proof.ms -Field "$Label.ms"
    if ($latency -le 0.0) {
        throw "$Label.ms must be positive: $latency"
    }
    $renderReady = ConvertTo-EvidenceDouble -Value $Proof.render_ready_ms -Field "$Label.render_ready_ms"
    $firstPresent = ConvertTo-EvidenceDouble -Value $Proof.first_present_ms -Field "$Label.first_present_ms"
    $firstGpu = ConvertTo-EvidenceDouble -Value $Proof.first_gpu_ms -Field "$Label.first_gpu_ms"
    $stablePresent = ConvertTo-EvidenceDouble -Value $Proof.stable_present_ms -Field "$Label.stable_present_ms"
    $stableGpu = ConvertTo-EvidenceDouble -Value $Proof.stable_gpu_ms -Field "$Label.stable_gpu_ms"
    if ($renderReady -lt 0.0 -or
        $firstPresent -lt $renderReady -or
        $firstGpu -lt $firstPresent -or
        $stablePresent -lt $renderReady -or
        $stablePresent -lt $firstPresent -or
        $stableGpu -lt $firstGpu -or
        $stableGpu -lt $stablePresent -or
        [Math]::Abs($stableGpu - $latency) -gt 0.001) {
        throw "$Label presentation timestamps were not monotonic through the binding GPU completion"
    }

    $firstSequence = ConvertTo-EvidenceUInt64 -Value $Proof.first_frame_sequence -Field "$Label.first_frame_sequence"
    $stableSequence = ConvertTo-EvidenceUInt64 -Value $Proof.stable_frame_sequence -Field "$Label.stable_frame_sequence"
    if ($firstSequence -eq [uint64]::MaxValue -or $stableSequence -ne $firstSequence + 1) {
        throw "$Label frame sequences were not adjacent: first=$firstSequence stable=$stableSequence"
    }

    $expectedColumns = ConvertTo-EvidenceUInt64 -Value $Proof.expected -Field "$Label.expected"
    $loadedColumns = ConvertTo-EvidenceUInt64 -Value $Proof.loaded_target -Field "$Label.loaded_target"
    if ($expectedColumns -ne 1089 -or $loadedColumns -ne $expectedColumns) {
        throw "$Label loaded/expected cohort counts were not exact: expected=$expectedColumns loaded=$loadedColumns"
    }
    foreach ($field in @(
        'missing_target', 'foreign_loaded', 'foreign_requested', 'foreign_resident', 'source_leftover'
    )) {
        $value = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
        if ($value -ne 0) {
            throw "$Label.$field=$value, expected zero"
        }
    }

    foreach ($field in @('resident_count', 'known_air_count', 'view_generation', 'frame_count')) {
        $null = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
    }
    if ([uint64]$Proof.resident_count + [uint64]$Proof.known_air_count -eq 0) {
        throw "$Label resident and known-air identities were both empty"
    }
    foreach ($field in @('resident_hash', 'known_air_hash')) {
        if ([string]$Proof.$field -notmatch '^[0-9a-fA-F]{16}$') {
            throw "$Label.$field was not a 16-digit deterministic hash: $($Proof.$field)"
        }
    }

    $expectedManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $Proof.expected_manifest_count `
        -Field "$Label.expected_manifest_count"
    $firstManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $Proof.first_presented_manifest_count `
        -Field "$Label.first_presented_manifest_count"
    $stableManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $Proof.stable_presented_manifest_count `
        -Field "$Label.stable_presented_manifest_count"
    if ($expectedManifestCount -eq 0 -or
        $firstManifestCount -ne $expectedManifestCount -or
        $stableManifestCount -ne $expectedManifestCount) {
        throw "$Label presented manifest count did not equal expected: expected=$expectedManifestCount first=$firstManifestCount stable=$stableManifestCount"
    }
    $expectedManifestHash = ([string]$Proof.expected_manifest_hash).ToLowerInvariant()
    $firstManifestHash = ([string]$Proof.first_presented_manifest_hash).ToLowerInvariant()
    $stableManifestHash = ([string]$Proof.stable_presented_manifest_hash).ToLowerInvariant()
    foreach ($entry in @(
        [pscustomobject]@{ Name = 'expected_manifest_hash'; Value = $expectedManifestHash },
        [pscustomobject]@{ Name = 'first_presented_manifest_hash'; Value = $firstManifestHash },
        [pscustomobject]@{ Name = 'stable_presented_manifest_hash'; Value = $stableManifestHash }
    )) {
        if ($entry.Value -notmatch '^[0-9a-f]{16}$') {
            throw "$Label.$($entry.Name) was not a 16-digit deterministic hash: $($entry.Value)"
        }
    }
    if ($firstManifestHash -cne $expectedManifestHash -or $stableManifestHash -cne $expectedManifestHash) {
        throw "$Label presented manifest hash did not equal expected: expected=$expectedManifestHash first=$firstManifestHash stable=$stableManifestHash"
    }

    foreach ($field in @(
        'missing_target_instances', 'unexpected_target_instances', 'source_instances',
        'foreign_instances', 'stale_generation_instances', 'orphan_allocations'
    )) {
        $value = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
        if ($value -ne 0) {
            throw "$Label.$field=$value, expected zero"
        }
    }

    $intervalFrameCount = ConvertTo-EvidenceUInt64 -Value $Proof.frame_count -Field "$Label.frame_count"
    if ($intervalFrameCount -lt 2) {
        throw "$Label.frame_count must cover at least two presented frames: $intervalFrameCount"
    }
    # One boundary frame is allowed because the measured interval begins and
    # ends on callbacks rather than between frame ticks.
    $maximumCappedFrames = [uint64][Math]::Ceiling($latency * 60.0 / 1000.0) + 1
    if ($intervalFrameCount -gt $maximumCappedFrames) {
        throw "$Label exceeded its 60fps cap: frames=$intervalFrameCount maximum=$maximumCappedFrames interval_ms=$latency"
    }

    if ($Kind -ceq 'Teleport') {
        $publisher = Assert-OptionalStageOffset -Proof $Proof -Field 'publisher_ms' -Label $Label -Maximum $renderReady
        $firstLevel = Assert-OptionalStageOffset -Proof $Proof -Field 'first_level_ms' -Label $Label -Maximum $renderReady
        $lastLevel = Assert-OptionalStageOffset -Proof $Proof -Field 'last_level_ms' -Label $Label -Maximum $renderReady
        $firstSub = Assert-OptionalStageOffset -Proof $Proof -Field 'first_sub_ms' -Label $Label -Maximum $renderReady
        $lastSub = Assert-OptionalStageOffset -Proof $Proof -Field 'last_sub_ms' -Label $Label -Maximum $renderReady
        $null = $publisher
        foreach ($pair in @(
            [pscustomobject]@{ Name = 'level'; First = $firstLevel; Last = $lastLevel },
            [pscustomobject]@{ Name = 'sub'; First = $firstSub; Last = $lastSub }
        )) {
            if (($null -eq $pair.First) -ne ($null -eq $pair.Last) -or
                ($null -ne $pair.First -and $pair.First -gt $pair.Last)) {
                throw "$Label $($pair.Name) stage offsets were not a monotonic pair"
            }
        }
        foreach ($field in @('level_events', 'sub_events')) {
            $null = ConvertTo-EvidenceUInt64 -Value $Proof.$field -Field "$Label.$field"
        }
    }
}

function Assert-MarkerMatchesProof {
    param(
        [Parameter(Mandatory = $true)]$Marker,
        [Parameter(Mandatory = $true)]$Proof,
        [Parameter(Mandatory = $true)][ValidateSet('Teleport', 'ForcedRemesh')][string]$Kind,
        [Parameter(Mandatory = $true)][string]$Label
    )

    foreach ($field in Get-FullViewProofFieldNames -Kind $Kind) {
        $markerProperty = $Marker.PSObject.Properties[$field]
        if ($null -eq $markerProperty) {
            throw "$Label marker is missing $field"
        }
        $proofValue = $Proof.PSObject.Properties[$field].Value
        $markerValue = $markerProperty.Value
        if ($null -eq $proofValue -or $null -eq $markerValue) {
            if (-not ($null -eq $proofValue -and $null -eq $markerValue)) {
                throw "$Label marker/metrics mismatch for ${field}: marker=$markerValue metrics=$proofValue"
            }
            continue
        }
        if ($field -ceq 'ms' -or $field.EndsWith('_ms', [StringComparison]::Ordinal)) {
            $markerNumber = ConvertTo-EvidenceDouble -Value $markerValue -Field "$Label marker $field"
            $proofNumber = ConvertTo-EvidenceDouble -Value $proofValue -Field "$Label metrics $field"
            if ([Math]::Abs($markerNumber - $proofNumber) -gt 0.001) {
                throw "$Label marker/metrics mismatch for ${field}: marker=$markerNumber metrics=$proofNumber"
            }
            continue
        }
        if ([string]$markerValue -cne [string]$proofValue) {
            throw "$Label marker/metrics mismatch for ${field}: marker=$markerValue metrics=$proofValue"
        }
    }
}

function Assert-FullViewProofCohortContinuity {
    param(
        [Parameter(Mandatory = $true)]$TeleportProof,
        [Parameter(Mandatory = $true)]$ForcedRemeshProof
    )

    foreach ($field in @(
        'target', 'committed', 'expected', 'loaded_target', 'resident_count', 'resident_hash',
        'known_air_count', 'known_air_hash'
    )) {
        $teleportValue = [string]$TeleportProof.PSObject.Properties[$field].Value
        $remeshValue = [string]$ForcedRemeshProof.PSObject.Properties[$field].Value
        if ($field.EndsWith('_hash', [StringComparison]::Ordinal)) {
            $teleportValue = $teleportValue.ToLowerInvariant()
            $remeshValue = $remeshValue.ToLowerInvariant()
        }
        if ($teleportValue -cne $remeshValue) {
            throw "full-view proof cohort changed between teleport and forced remesh at ${field}: teleport=$teleportValue remesh=$remeshValue"
        }
    }
    $teleportStableFrame = ConvertTo-EvidenceUInt64 `
        -Value $TeleportProof.stable_frame_sequence `
        -Field 'teleport_proof.stable_frame_sequence'
    $remeshFirstFrame = ConvertTo-EvidenceUInt64 `
        -Value $ForcedRemeshProof.first_frame_sequence `
        -Field 'forced_full_view_remesh_proof.first_frame_sequence'
    if ($remeshFirstFrame -le $teleportStableFrame) {
        throw "forced remesh frames were not later than teleport frames: teleport_stable=$teleportStableFrame remesh_first=$remeshFirstFrame"
    }
    $teleportGeneration = ConvertTo-EvidenceUInt64 `
        -Value $TeleportProof.view_generation `
        -Field 'teleport_proof.view_generation'
    $remeshGeneration = ConvertTo-EvidenceUInt64 `
        -Value $ForcedRemeshProof.view_generation `
        -Field 'forced_full_view_remesh_proof.view_generation'
    if ($remeshGeneration -le $teleportGeneration) {
        throw "forced remesh view generation did not advance beyond teleport: teleport=$teleportGeneration remesh=$remeshGeneration"
    }
    $teleportManifestHash = ([string]$TeleportProof.expected_manifest_hash).ToLowerInvariant()
    $remeshManifestHash = ([string]$ForcedRemeshProof.expected_manifest_hash).ToLowerInvariant()
    $teleportManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $TeleportProof.expected_manifest_count `
        -Field 'teleport_proof.expected_manifest_count'
    $remeshManifestCount = ConvertTo-EvidenceUInt64 `
        -Value $ForcedRemeshProof.expected_manifest_count `
        -Field 'forced_full_view_remesh_proof.expected_manifest_count'
    if ($remeshManifestCount -ne $teleportManifestCount) {
        throw "forced remesh expected manifest count changed from teleport: teleport=$teleportManifestCount remesh=$remeshManifestCount"
    }
    if ($remeshManifestHash -ceq $teleportManifestHash) {
        throw "forced remesh expected manifest hash did not change from teleport: $remeshManifestHash"
    }
}

function New-FullViewResourceTrigger {
    param(
        [Parameter(Mandatory = $true)]$TeleportMarker,
        [Parameter(Mandatory = $true)]$ForcedRemeshMarker
    )

    foreach ($entry in @(
        [pscustomobject]@{ Marker = $TeleportMarker; Label = 'teleport marker' },
        [pscustomobject]@{ Marker = $ForcedRemeshMarker; Label = 'forced-remesh marker' }
    )) {
        foreach ($field in @('target', 'view_generation', 'stable_frame_sequence')) {
            $null = Get-RequiredEvidenceProperty `
                -Evidence $entry.Marker `
                -Name $field `
                -Label $entry.Label
        }
    }
    if ([string]$TeleportMarker.target -cne [string]$ForcedRemeshMarker.target) {
        throw "steady-resource trigger targets differ: teleport=$($TeleportMarker.target) remesh=$($ForcedRemeshMarker.target)"
    }
    return [pscustomobject][ordered]@{
        kind = 'FullViewPresented'
        target = [string]$TeleportMarker.target
        teleport_view_generation = ConvertTo-EvidenceUInt64 `
            -Value $TeleportMarker.view_generation `
            -Field 'teleport marker view_generation'
        teleport_stable_frame_sequence = ConvertTo-EvidenceUInt64 `
            -Value $TeleportMarker.stable_frame_sequence `
            -Field 'teleport marker stable_frame_sequence'
        forced_remesh_view_generation = ConvertTo-EvidenceUInt64 `
            -Value $ForcedRemeshMarker.view_generation `
            -Field 'forced-remesh marker view_generation'
        forced_remesh_stable_frame_sequence = ConvertTo-EvidenceUInt64 `
            -Value $ForcedRemeshMarker.stable_frame_sequence `
            -Field 'forced-remesh marker stable_frame_sequence'
    }
}

function New-SteadyResourceDocument {
    param(
        [Parameter(Mandatory = $true)][ValidateNotNullOrEmpty()][object[]]$Samples,
        [Parameter(Mandatory = $true)][ValidateRange(1, 300)][int]$DurationSeconds,
        [Parameter(Mandatory = $true)]$Trigger
    )

    return [pscustomobject][ordered]@{
        schema = 'rust-mcbe-steady-resources-v2'
        trigger = $Trigger
        duration_seconds = $DurationSeconds
        processor_count = [Environment]::ProcessorCount
        samples = @($Samples)
        summary = Get-SteadyResourceSummary -Samples $Samples
    }
}

function Assert-SteadyResourceArtifact {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]$TeleportMarker,
        [Parameter(Mandatory = $true)]$ForcedRemeshMarker
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "steady resource artifact was not written before full-view SLA validation: $Path"
    }
    $document = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    if ([string]$document.schema -cne 'rust-mcbe-steady-resources-v2') {
        throw "steady resource artifact schema was not rust-mcbe-steady-resources-v2: $($document.schema)"
    }
    if ([int]$document.duration_seconds -ne 30 -or @($document.samples).Count -ne 30) {
        throw "steady resource artifact did not contain 30 one-second samples: duration=$($document.duration_seconds) samples=$(@($document.samples).Count)"
    }
    if ([int]$document.processor_count -le 0) {
        throw "steady resource artifact processor_count was not positive: $($document.processor_count)"
    }
    if ($null -eq $document.summary) {
        throw 'steady resource artifact summary was missing'
    }
    if ([int]$document.summary.sample_count -ne 30) {
        throw "steady resource artifact summary sample_count was not 30: $($document.summary.sample_count)"
    }

    $expectedTrigger = New-FullViewResourceTrigger `
        -TeleportMarker $TeleportMarker `
        -ForcedRemeshMarker $ForcedRemeshMarker
    if ($null -eq $document.trigger) {
        throw 'steady resource artifact trigger was missing'
    }
    foreach ($field in @(
        'kind', 'target', 'teleport_view_generation', 'teleport_stable_frame_sequence',
        'forced_remesh_view_generation', 'forced_remesh_stable_frame_sequence'
    )) {
        $actual = [string]$document.trigger.PSObject.Properties[$field].Value
        $expected = [string]$expectedTrigger.PSObject.Properties[$field].Value
        if ($actual -cne $expected) {
            throw "steady resource artifact trigger mismatch for ${field}: expected=$expected actual=$actual"
        }
    }

    $samples = @($document.samples)
    $previousElapsed = 0.0
    for ($index = 0; $index -lt $samples.Count; $index++) {
        $sample = $samples[$index]
        $elapsed = ConvertTo-EvidenceDouble `
            -Value (Get-RequiredEvidenceProperty -Evidence $sample -Name 'elapsed_seconds' -Label "steady resource sample $index") `
            -Field "steady resource sample $index elapsed_seconds"
        $rss = ConvertTo-EvidenceUInt64 `
            -Value (Get-RequiredEvidenceProperty -Evidence $sample -Name 'combined_rss_bytes' -Label "steady resource sample $index") `
            -Field "steady resource sample $index combined_rss_bytes"
        $cpu = ConvertTo-EvidenceDouble `
            -Value (Get-RequiredEvidenceProperty -Evidence $sample -Name 'cpu_percent' -Label "steady resource sample $index") `
            -Field "steady resource sample $index cpu_percent"
        $elapsedDelta = $elapsed - $previousElapsed
        if ($elapsedDelta -lt 0.5 -or $elapsedDelta -gt 2.5) {
            throw "steady resource sample cadence was not one second at index ${index}: delta=$elapsedDelta"
        }
        if ($rss -eq 0 -or $cpu -lt 0.0) {
            throw "steady resource sample $index contained an impossible value: rss=$rss cpu=$cpu"
        }
        $previousElapsed = $elapsed
    }

    $recomputed = Get-SteadyResourceSummary -Samples $samples
    $storedMaxRss = ConvertTo-EvidenceUInt64 `
        -Value $document.summary.max_combined_rss_bytes `
        -Field 'steady resource artifact max_combined_rss_bytes'
    $storedMeanCpu = ConvertTo-EvidenceDouble `
        -Value $document.summary.mean_cpu_percent `
        -Field 'steady resource artifact mean_cpu_percent'
    $storedP95Cpu = ConvertTo-EvidenceDouble `
        -Value $document.summary.p95_cpu_percent `
        -Field 'steady resource artifact p95_cpu_percent'
    if ($storedMaxRss -ne [uint64]$recomputed.max_combined_rss_bytes -or
        [Math]::Abs($storedMeanCpu - [double]$recomputed.mean_cpu_percent) -gt 0.000001 -or
        [Math]::Abs($storedP95Cpu - [double]$recomputed.p95_cpu_percent) -gt 0.000001) {
        throw "steady resource artifact summary did not match samples: stored_rss=$storedMaxRss recomputed_rss=$($recomputed.max_combined_rss_bytes) stored_mean=$storedMeanCpu recomputed_mean=$($recomputed.mean_cpu_percent) stored_p95=$storedP95Cpu recomputed_p95=$($recomputed.p95_cpu_percent)"
    }
    if ([uint64]$recomputed.max_combined_rss_bytes -gt 650MB) {
        throw "combined steady RSS exceeded 650 MiB: $($recomputed.max_combined_rss_bytes) bytes"
    }
    if ([double]$recomputed.mean_cpu_percent -gt 15.0 -or
        [double]$recomputed.p95_cpu_percent -gt 15.0) {
        throw "steady CPU exceeded 15%: mean=$($recomputed.mean_cpu_percent) p95=$($recomputed.p95_cpu_percent)"
    }
}

function Measure-SteadyResources {
    param(
        [Parameter(Mandatory = $true)]$ClientHandle,
        [Parameter(Mandatory = $true)]$CoreHandle,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [Parameter(Mandatory = $true)]$TeleportMarker,
        [Parameter(Mandatory = $true)]$ForcedRemeshMarker,
        [ValidateRange(1, 300)][int]$DurationSeconds = 30
    )

    $client = $ClientHandle.Process
    $core = $CoreHandle.Process
    $client.Refresh()
    $core.Refresh()
    $previousCpuSeconds = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
    $previousWallSeconds = 0.0
    $stopwatch = [Diagnostics.Stopwatch]::StartNew()
    $samples = [Collections.Generic.List[object]]::new()
    for ($index = 0; $index -lt $DurationSeconds; $index++) {
        Start-Sleep -Seconds 1
        if ($client.HasExited -or $core.HasExited) {
            throw 'client or core exited during steady resource sampling'
        }
        $client.Refresh()
        $core.Refresh()
        $wallSeconds = $stopwatch.Elapsed.TotalSeconds
        $cpuSeconds = $client.TotalProcessorTime.TotalSeconds + $core.TotalProcessorTime.TotalSeconds
        $wallDelta = $wallSeconds - $previousWallSeconds
        $cpuDelta = $cpuSeconds - $previousCpuSeconds
        $cpuPercent = 100.0 * $cpuDelta / ($wallDelta * [Environment]::ProcessorCount)
        $samples.Add([pscustomobject][ordered]@{
            elapsed_seconds = $wallSeconds
            combined_rss_bytes = [uint64]($client.WorkingSet64 + $core.WorkingSet64)
            cpu_percent = [Math]::Max(0.0, $cpuPercent)
        })
        $previousWallSeconds = $wallSeconds
        $previousCpuSeconds = $cpuSeconds
    }
    $stopwatch.Stop()

    $trigger = New-FullViewResourceTrigger `
        -TeleportMarker $TeleportMarker `
        -ForcedRemeshMarker $ForcedRemeshMarker
    $document = New-SteadyResourceDocument `
        -Samples @($samples) `
        -DurationSeconds $DurationSeconds `
        -Trigger $trigger
    $summary = $document.summary
    $path = Join-Path $RunDirectory 'steady-resources.json'
    [IO.File]::WriteAllText(
        $path,
        ($document | ConvertTo-Json -Depth 6),
        [Text.UTF8Encoding]::new($false)
    )
    if ([uint64]$summary.max_combined_rss_bytes -gt 650MB) {
        throw "combined steady RSS exceeded 650 MiB: $($summary.max_combined_rss_bytes) bytes"
    }
    if ([double]$summary.mean_cpu_percent -gt 15.0 -or [double]$summary.p95_cpu_percent -gt 15.0) {
        throw "steady CPU exceeded 15%: mean=$($summary.mean_cpu_percent) p95=$($summary.p95_cpu_percent)"
    }
    return $document
}

function Assert-AcceptanceMetrics {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [switch]$RequireFullViewTeleport,
        $TeleportMarker,
        $ForcedRemeshMarker,
        [string]$ExpectedTargetCohort,
        [string]$SteadyResourceArtifactPath
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "app did not write acceptance metrics: $Path"
    }
    $metrics = Get-Content -Raw -LiteralPath $Path | ConvertFrom-Json
    $required = @(
        'session_seconds', 'world_ready', 'requested_radius_chunks', 'received_radius_chunks',
        'publisher_radius_chunks', 'mutation_coordinate', 'visible_mutation_count', 'frame_count',
        'p50_frame_ms', 'p95_frame_ms', 'p99_frame_ms', 'max_frame_ms', 'max_decode_ms',
        'max_mesh_ms', 'max_remesh_ms', 'teleport_settle_ms', 'forced_full_view_remesh_ms',
        'max_mutation_to_visible_ms', 'decode_error_count',
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
    foreach ($field in @('rendered_sub_chunks', 'resident_sub_chunks', 'visible_sub_chunks')) {
        if ([uint64]$metrics.$field -eq 0) {
            throw "$field was zero"
        }
    }
    if (-not $RequireFullViewTeleport -and [uint64]$metrics.visible_mutation_count -eq 0) {
        throw 'visible_mutation_count was zero'
    }
    if ($RequireFullViewTeleport) {
        if ([string]::IsNullOrWhiteSpace($SteadyResourceArtifactPath) -or
            -not (Test-Path -LiteralPath $SteadyResourceArtifactPath -PathType Leaf)) {
            throw "steady resource artifact was not written before full-view SLA validation: $SteadyResourceArtifactPath"
        }
        if ($null -eq $TeleportMarker) {
            throw 'parsed teleport settle marker was not supplied'
        }
        if ($null -eq $ForcedRemeshMarker) {
            throw 'parsed forced-remesh settle marker was not supplied'
        }
        if ([string]::IsNullOrWhiteSpace($ExpectedTargetCohort)) {
            throw 'expected target cohort was not supplied'
        }
        $teleportProofProperty = $metrics.PSObject.Properties['teleport_proof']
        if ($null -eq $teleportProofProperty -or $null -eq $teleportProofProperty.Value) {
            throw 'acceptance metrics are missing teleport_proof'
        }
        $remeshProofProperty = $metrics.PSObject.Properties['forced_full_view_remesh_proof']
        if ($null -eq $remeshProofProperty -or $null -eq $remeshProofProperty.Value) {
            throw 'acceptance metrics are missing forced_full_view_remesh_proof'
        }
        if ($null -eq $metrics.teleport_settle_ms) {
            throw 'teleport_settle_ms was not recorded'
        }
        $teleport = [double]$metrics.teleport_settle_ms
        if ($null -eq $metrics.forced_full_view_remesh_ms) {
            throw 'forced_full_view_remesh_ms was not recorded'
        }
        $remesh = [double]$metrics.forced_full_view_remesh_ms

        Assert-ExactFullViewProof `
            -Proof $teleportProofProperty.Value `
            -Kind Teleport `
            -Label 'teleport_proof' `
            -ExpectedTargetCohort $ExpectedTargetCohort
        Assert-ExactFullViewProof `
            -Proof $remeshProofProperty.Value `
            -Kind ForcedRemesh `
            -Label 'forced_full_view_remesh_proof' `
            -ExpectedTargetCohort $ExpectedTargetCohort
        Assert-FullViewProofCohortContinuity `
            -TeleportProof $teleportProofProperty.Value `
            -ForcedRemeshProof $remeshProofProperty.Value
        Assert-MarkerMatchesProof `
            -Marker $TeleportMarker `
            -Proof $teleportProofProperty.Value `
            -Kind Teleport `
            -Label 'teleport'
        Assert-MarkerMatchesProof `
            -Marker $ForcedRemeshMarker `
            -Proof $remeshProofProperty.Value `
            -Kind ForcedRemesh `
            -Label 'forced remesh'
        Assert-SteadyResourceArtifact `
            -Path $SteadyResourceArtifactPath `
            -TeleportMarker $TeleportMarker `
            -ForcedRemeshMarker $ForcedRemeshMarker

        if ([double]::IsNaN($teleport) -or
            [double]::IsInfinity($teleport) -or
            [Math]::Abs($teleport - [double]$teleportProofProperty.Value.ms) -gt 0.001) {
            throw "teleport_settle_ms did not match its exact proof: metric=$teleport proof=$($teleportProofProperty.Value.ms)"
        }
        if ([double]::IsNaN($remesh) -or
            [double]::IsInfinity($remesh) -or
            [Math]::Abs($remesh - [double]$remeshProofProperty.Value.ms) -gt 0.001) {
            throw "forced_full_view_remesh_ms did not match its exact proof: metric=$remesh proof=$($remeshProofProperty.Value.ms)"
        }
        if ($teleport -gt 2000.0) {
            throw "teleport_settle_ms failed the 2000ms gate: $($metrics.teleport_settle_ms)"
        }
        if ($remesh -gt 2000.0) {
            throw "forced_full_view_remesh_ms failed the 2000ms gate: $($metrics.forced_full_view_remesh_ms)"
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
if ($FullViewTeleportGate -and $VisualFixturePose -ne 'None') {
    throw 'FullViewTeleportGate and VisualFixturePose cannot be combined'
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
if ($VisualFixturePose -eq 'None' -and -not $FullViewTeleportGate) {
    $AppArguments += '--auto-fly'
}
if ($FullViewTeleportGate) {
    $AppArguments += @('--full-view-teleport-gate', '--frame-cap', '60')
}
else {
    $AppArguments += '--no-vsync'
}
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
    if ($FullViewTeleportGate) {
        Write-Output 'FULL_VIEW_TELEPORT_GATE=1'
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
$teleportMarkerEvidence = $null
$forcedRemeshMarkerEvidence = $null
$expectedTargetCohort = $null
$steadyResourceArtifactPath = Join-Path $RunDirectory 'steady-resources.json'

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
    if ($FullViewTeleportGate) {
        $metadata['full_view_teleport_gate'] = $true
        $metadata['frame_cap'] = 60
    }
    $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8

    New-Item -ItemType Directory -Path (Split-Path -Parent $RuntimeDirectory) -Force | Out-Null
    New-Item -ItemType Directory -Path (Split-Path -Parent $MetricsOut) -Force | Out-Null

    $lockPath = $RuntimeDirectory + '.lock'
    $lease = [IO.File]::Open($lockPath, [IO.FileMode]::OpenOrCreate, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $BdsExecutable = Set-StableRuntime -SourceDirectory $BdsDir -RuntimeDirectory $RuntimeDirectory -ExecutableName $BdsExecutableName

    $portReservation = New-ReservedUdpPort
    $portV6Reservation = New-ReservedUdpPort
    $bdsPort = $portReservation.Port
    $Upstream = "127.0.0.1:$bdsPort"
    Set-ServerProperties -Path (Join-Path $RuntimeDirectory 'server.properties') -Port $bdsPort -PortV6 $portV6Reservation.Port
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
    # BDS can buffer redirected stdout until shutdown, so also accept its protocol-level readiness signal.
    $bdsReadinessProbe = {
        Test-RakNetUnconnectedPong `
            -Address '127.0.0.1' `
            -Port $bdsPort `
            -TimeoutMilliseconds 500
    }.GetNewClosure()
    $null = Wait-ProcessOutputMarker `
        -Handle $bdsHandle `
        -Marker 'Server started.' `
        -TimeoutSeconds 120 `
        -ReadinessProbe $bdsReadinessProbe

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
    if ($FullViewTeleportGate) {
        $teleportPlan = New-FullViewTeleportPlan -MutationCoordinate $coordinate
        Publish-FullViewTeleport -Handle $bdsHandle -Plan $teleportPlan -RunDirectory $RunDirectory
        $targetChunkX = [int][Math]::Floor([double]$teleportPlan.Target.x / 16.0)
        $targetChunkZ = [int][Math]::Floor([double]$teleportPlan.Target.z / 16.0)
        $expectedTargetCohort = '{0}:{1}:{2}:16' -f 0, $targetChunkX, $targetChunkZ
        $teleportMarkerLine = Wait-ProcessOutputMarker `
            -Handle $appHandle `
            -Marker 'RUST_MCBE_TELEPORT_SETTLED ' `
            -TimeoutSeconds 180
        $teleportMarkerEvidence = ConvertFrom-FullViewSettleMarker `
            -Line $teleportMarkerLine `
            -Kind Teleport
        $teleportMilliseconds = [double]$teleportMarkerEvidence.ms
        $forcedRemeshMarkerLine = Wait-ProcessOutputMarker `
            -Handle $appHandle `
            -Marker 'RUST_MCBE_FORCED_FULL_VIEW_REMESH_SETTLED ' `
            -TimeoutSeconds 30
        $forcedRemeshMarkerEvidence = ConvertFrom-FullViewSettleMarker `
            -Line $forcedRemeshMarkerLine `
            -Kind ForcedRemesh
        $remeshMilliseconds = [double]$forcedRemeshMarkerEvidence.ms
        $resourceDocument = Measure-SteadyResources `
            -ClientHandle $appHandle `
            -CoreHandle $coreHandle `
            -RunDirectory $RunDirectory `
            -TeleportMarker $teleportMarkerEvidence `
            -ForcedRemeshMarker $forcedRemeshMarkerEvidence `
            -DurationSeconds 30
        $metadata['teleport_settle_ms'] = $teleportMilliseconds
        $metadata['forced_full_view_remesh_ms'] = $remeshMilliseconds
        $metadata['steady_resources'] = $resourceDocument.summary
        $metadata | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $RunDirectory 'metadata.json') -Encoding UTF8
    }
    elseif ($VisualFixturePose -ne 'None') {
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
        if (-not $FullViewTeleportGate -and [DateTime]::UtcNow -ge $nextMutation) {
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

    if ($FullViewTeleportGate) {
        $metrics = Assert-AcceptanceMetrics `
            -Path $CanonicalMetrics `
            -RequireFullViewTeleport `
            -TeleportMarker $teleportMarkerEvidence `
            -ForcedRemeshMarker $forcedRemeshMarkerEvidence `
            -ExpectedTargetCohort $expectedTargetCohort `
            -SteadyResourceArtifactPath $steadyResourceArtifactPath
    }
    else {
        $metrics = Assert-AcceptanceMetrics -Path $CanonicalMetrics
    }
    $metrics | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath (Join-Path $RunDirectory 'validated-metrics.json') -Encoding UTF8
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
