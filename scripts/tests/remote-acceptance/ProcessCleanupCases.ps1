Describe 'Acceptance process cleanup ownership' {
    BeforeEach {
        . (Join-Path $ProjectRoot 'scripts/acceptance/Process.ps1')
        . (Join-Path $ProjectRoot 'scripts/acceptance/Common.ps1')
        $script:cleanupCalls = [Collections.Generic.List[string]]::new()
    }

    It 'disposes the first log when acquiring the second log fails' {
        $script:firstLog = $null
        Mock New-ProcessLogStream {
            if ($null -ne $script:firstLog) { throw 'second log failed' }
            $script:firstLog = [IO.FileStream]::new((Join-Path $TestDrive 'first.log'), [IO.FileMode]::Create)
            return $script:firstLog
        }
        {
            Start-LoggedProcess -Executable 'unused' -Arguments @() -WorkingDirectory $TestDrive `
                -StdoutPath (Join-Path $TestDrive 'stdout.log') -StderrPath (Join-Path $TestDrive 'stderr.log')
        } | Should Throw '*second log failed*'
        $script:firstLog.CanWrite | Should Be $false
    }

    It 'stops and disposes a started child when copy setup fails' {
        $script:stoppedProcess = $null
        $script:reaped = $false
        Mock Start-ProcessLogCopy { throw 'copy setup failed' }
        Mock Stop-BoundedProcess {
            $script:stoppedProcess = $Handle.Process
            $Handle.Process.Kill()
            $script:reaped = $Handle.Process.WaitForExit(10000)
        }
        {
            Start-LoggedProcess -Executable (Get-Process -Id $PID).Path `
                -Arguments @('-NoProfile', '-Command', '[Console]::In.ReadLine() | Out-Null') `
                -WorkingDirectory $TestDrive -StdoutPath (Join-Path $TestDrive 'child.out') `
                -StderrPath (Join-Path $TestDrive 'child.err')
        } | Should Throw '*copy setup failed*'
        $script:reaped | Should Be $true
        { $script:stoppedProcess.Id } | Should Throw
        foreach ($path in @('child.out', 'child.err')) {
            $stream = [IO.FileStream]::new((Join-Path $TestDrive $path), [IO.FileMode]::Open,
                [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
            $stream.Dispose()
        }
    }

    It 'attempts both flushes and disposals after independent stream failures' {
        $first = [pscustomobject]@{}
        $first | Add-Member ScriptMethod Flush { param($sync) $script:cleanupCalls.Add('flush first'); throw 'flush failed' }
        $first | Add-Member ScriptMethod Dispose { $script:cleanupCalls.Add('dispose first'); throw 'dispose failed' }
        $second = [pscustomobject]@{}
        $second | Add-Member ScriptMethod Flush { param($sync) $script:cleanupCalls.Add('flush second') }
        $second | Add-Member ScriptMethod Dispose { $script:cleanupCalls.Add('dispose second') }
        $handle = [pscustomobject]@{ StdoutStream = $first; StderrStream = $second; StdoutCopy = $null; StderrCopy = $null }
        { Complete-ProcessLogs $handle } | Should Throw '*Process cleanup failed*'
        ($script:cleanupCalls -join ',') | Should Be 'flush first,dispose first,flush second,dispose second'
    }

    It 'attempts client cleanup and disposal even when reporting and log completion fail' {
        Mock Write-AcceptanceJson { $script:cleanupCalls.Add('manifest'); throw 'manifest failed' }
        Mock Stop-BoundedProcess {
            $script:cleanupCalls.Add("stop $Kind")
            if ($Kind -eq 'app') { throw 'client stop failed' }
        }
        Mock Complete-ProcessLogs {
            $script:cleanupCalls.Add("logs $($Handle.Name)")
            if ($Handle.Name -eq 'client') { throw 'client logs failed' }
        }
        $clientProcess = [pscustomobject]@{}
        $clientProcess | Add-Member ScriptMethod Dispose { $script:cleanupCalls.Add('dispose client') }
        $client = [pscustomobject]@{ Name = 'client'; Process = $clientProcess }
        $parameters = @{ Manifest = @{ status = 'running' }; ManifestPath = 'unused'; RunSucceeded = $false
            ClientHandle = $client; ClientLogsCompleted = $false }
        { Complete-RemoteAcceptanceRun @parameters } | Should Throw '*Process cleanup failed*'
        ($script:cleanupCalls -join ',') | Should Be 'manifest,stop app,logs client,dispose client'
        $script:cleanupCalls.Clear()
        { Complete-RemoteAcceptanceRun @parameters -PrimaryFailure ([Exception]::new('original run failed')) } | Should Not Throw
        ($script:cleanupCalls -join ',') | Should Be 'manifest,stop app,logs client,dispose client'
    }
}
