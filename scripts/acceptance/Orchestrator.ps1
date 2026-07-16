function New-FullViewTeleportPlan {
    param(
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$MutationCoordinate,
        [switch]$LeafForest
    )

    if ($LeafForest) {
        return New-LeafForestPlan -MutationCoordinate $MutationCoordinate -Mode FullView
    }

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

function Set-BdsSourceWorldIdentityOnPlan {
    param(
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][AllowNull()]$Identity,
        [AllowNull()]$RuntimeIdentity = $null
    )

    $identityField = if ($null -ne $Identity) {
        'source_world_identity'
    }
    elseif ($null -ne $RuntimeIdentity) {
        'runtime_world_identity'
    }
    elseif ([string]$Plan.Manifest.schema -ceq 'rust-mcbe-visual-fixture-v2') {
        throw 'schema-v2 fixture plan requires source or runtime world identity evidence'
    }
    else {
        return
    }
    if ($null -ne $Plan.Manifest.PSObject.Properties[$identityField]) {
        throw "fixture plan already contains $identityField"
    }
    $selectedIdentity = if ($null -ne $Identity) { $Identity } else { $RuntimeIdentity }
    $Plan.Manifest | Add-Member -MemberType NoteProperty -Name $identityField -Value ([pscustomobject][ordered]@{
        schema = [string]$selectedIdentity.schema
        level_name = [string]$selectedIdentity.level_name
        file_count = [uint64]$selectedIdentity.file_count
        total_bytes = [uint64]$selectedIdentity.total_bytes
        level_dat_sha256 = [string]$selectedIdentity.level_dat_sha256
        sha256 = [string]$selectedIdentity.sha256
    })
}

function Get-BdsFixtureLoadAreaPlanIdentity {
    param([Parameter(Mandatory = $true)]$Plan)

    foreach ($propertyName in @(
        'LoadAreaName', 'LoadAreaCommand', 'LoadAreaMarker', 'LoadAreaSettleMilliseconds',
        'CleanupCommand', 'CleanupMarker'
    )) {
        if ($null -eq $Plan.PSObject.Properties[$propertyName]) {
            throw "fixture load-area plan is missing $propertyName"
        }
    }
    if ($null -eq $Plan.Manifest.PSObject.Properties['clear'] -or $null -eq $Plan.Manifest.clear) {
        throw 'fixture load-area plan is missing exact clear bounds'
    }
    $clear = $Plan.Manifest.clear
    return Get-CanonicalObjectHash -Value ([pscustomobject][ordered]@{
        schema = 'rust-mcbe-fixture-load-area-plan-v1'
        name = [string]$Plan.LoadAreaName
        command = [string]$Plan.LoadAreaCommand
        acknowledgement_marker = [string]$Plan.LoadAreaMarker
        configured_settle_milliseconds = [int]$Plan.LoadAreaSettleMilliseconds
        cleanup_command = [string]$Plan.CleanupCommand
        cleanup_acknowledgement_marker = [string]$Plan.CleanupMarker
        clear_min = @([int]$clear.min.x, [int]$clear.min.y, [int]$clear.min.z)
        clear_max = @([int]$clear.max.x, [int]$clear.max.y, [int]$clear.max.z)
    })
}

function Start-BdsFixtureLoadArea {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [ValidateRange(-1, 10000)][int]$SettleMilliseconds = -1,
        [scriptblock]$WaitForLoadArea
    )

    $loadCommandProperty = $Plan.PSObject.Properties['LoadAreaCommand']
    if ($null -eq $loadCommandProperty) {
        return $null
    }
    $planIdentitySha256 = Get-BdsFixtureLoadAreaPlanIdentity -Plan $Plan
    $activeProperty = $Handle.PSObject.Properties['ActiveTickingArea']
    if ($null -ne $activeProperty -and $null -ne $activeProperty.Value) {
        $active = $activeProperty.Value
        if ([string]$active.PlanIdentitySha256 -cne $planIdentitySha256) {
            throw "BDS handle already owns a different exact ticking-area plan: active=$($active.PlanIdentitySha256) requested=$planIdentitySha256 name=$($active.Name)"
        }
        if ([string]$active.Status -cne 'ready' -or $null -eq $active.Acknowledgement) {
            throw "BDS handle ticking area is not ready for exact-plan reuse: status=$($active.Status) name=$($active.Name)"
        }
        $area = Assert-BdsTickingAreaPreloadResult `
            -Line ([string]$active.Acknowledgement.stdout) `
            -ExpectedMinimum $Plan.Manifest.clear.min `
            -ExpectedMaximum $Plan.Manifest.clear.max
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'load_area_reused' -Fields ([ordered]@{
            name = [string]$active.Name
            plan_identity_sha256 = $planIdentitySha256
            initial_settle_milliseconds = [int]$active.SettleMilliseconds
        })
        return $area
    }
    $effectiveSettleMilliseconds = if ($SettleMilliseconds -ge 0) {
        $SettleMilliseconds
    }
    else {
        [int]$Plan.LoadAreaSettleMilliseconds
    }
    $activeState = [pscustomobject][ordered]@{
        Name = [string]$Plan.LoadAreaName
        PlanIdentitySha256 = $planIdentitySha256
        Command = [string]$Plan.LoadAreaCommand
        Marker = [string]$Plan.LoadAreaMarker
        CleanupCommand = [string]$Plan.CleanupCommand
        CleanupMarker = [string]$Plan.CleanupMarker
        SettleMilliseconds = $effectiveSettleMilliseconds
        Status = 'pending'
        Acknowledgement = $null
    }
    $Handle | Add-Member -MemberType NoteProperty -Name ActiveTickingArea -Value $activeState -Force
    $consoleLogPath = Join-Path $RunDirectory 'bds.console.log'
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.LoadAreaCommand -LogPath $consoleLogPath
    $rawEvidence = if ($null -eq $WaitForLoadArea) {
        Wait-ProcessOutputMarker `
            -Handle $Handle `
            -Marker $Plan.LoadAreaMarker `
            -TimeoutSeconds 30 `
            -RejectMarker ' ERROR] ' `
            -PassThruEvidence
    }
    else {
        & $WaitForLoadArea $Handle $Plan.LoadAreaMarker 30
    }
    $markerEvidence = Get-RequiredBdsMarkerEvidence `
        -Evidence $rawEvidence `
        -Context 'fixture load-area wait'
    $area = Assert-BdsTickingAreaPreloadResult `
        -Line ([string]$markerEvidence.Line) `
        -ExpectedMinimum $Plan.Manifest.clear.min `
        -ExpectedMaximum $Plan.Manifest.clear.max
    $activeState.Status = 'ready'
    $activeState.Acknowledgement = $area
    Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'load_area_ready' -Fields ([ordered]@{
        name = [string]$Plan.LoadAreaName
        command = [string]$Plan.LoadAreaCommand
        plan_identity_sha256 = $planIdentitySha256
        settle_milliseconds = $effectiveSettleMilliseconds
        acknowledged_min_x = [int]$area.min_x
        acknowledged_min_z = [int]$area.min_z
        acknowledged_max_x = [int]$area.max_x
        acknowledged_max_z = [int]$area.max_z
    })
    if ($effectiveSettleMilliseconds -gt 0) {
        Start-Sleep -Milliseconds $effectiveSettleMilliseconds
    }
    return $area
}

function Complete-BdsFixtureCommandBatch {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [scriptblock]$WaitForFence
    )

    $fixtureCommands = @($Plan.FixtureCommands)
    $consoleLogPath = Join-Path $RunDirectory 'bds.console.log'
    foreach ($command in $fixtureCommands) {
        Write-BdsConsoleCommand -Handle $Handle -Command $command -LogPath $consoleLogPath
    }
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
    $rawEvidence = if ($null -eq $WaitForFence) {
        Wait-ProcessOutputMarker `
            -Handle $Handle `
            -Marker $Plan.FenceMarker `
            -TimeoutSeconds 30 `
            -PassThruEvidence
    }
    else {
        & $WaitForFence $Handle $Plan.FenceMarker 30
    }
    $markerEvidence = Get-RequiredBdsMarkerEvidence `
        -Evidence $rawEvidence `
        -Context 'schema-v2 fixture fence wait' `
        -RequireSkippedLines
    $lineNumberProperty = $markerEvidence.PSObject.Properties['LineNumber']
    $readOffsetProperty = $markerEvidence.PSObject.Properties['ReadOffset']
    $observedAtProperty = $markerEvidence.PSObject.Properties['ObservedAtUtc']
    $stdoutEvidencePath = Join-Path $RunDirectory 'fixture-command-stdout.json'
    $stdoutEvidence = [pscustomobject][ordered]@{
        schema = 'rust-mcbe-fixture-command-stdout-v1'
        marker = [string]$Plan.FenceMarker
        marker_line = [string]$markerEvidence.Line
        marker_line_number = if ($null -eq $lineNumberProperty) { $null } else { [uint64]$lineNumberProperty.Value }
        read_offset = if ($null -eq $readOffsetProperty) { $null } else { [long]$readOffsetProperty.Value }
        observed_at_utc = if ($null -eq $observedAtProperty) { $null } else { [string]$observedAtProperty.Value }
        skipped_line_count = @($markerEvidence.SkippedLines).Count
        skipped_lines_sha256 = Get-Utf8Sha256 -Text (@($markerEvidence.SkippedLines) -join "`n")
        skipped_lines = @($markerEvidence.SkippedLines)
    }
    $stdoutEvidenceSha256 = Write-AtomicJsonArtifact -Path $stdoutEvidencePath -Value $stdoutEvidence
    $resultEvidence = Assert-BdsFixtureCommandResults `
        -Commands $fixtureCommands `
        -Lines @($markerEvidence.SkippedLines)
    Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'fixture_commands_completed' -Fields ([ordered]@{
        command_count = $fixtureCommands.Count
        result_count = [int]$resultEvidence.result_count
        result_stdout_sha256 = [string]$resultEvidence.stdout_sha256
        stdout_evidence = $stdoutEvidencePath
        stdout_evidence_sha256 = $stdoutEvidenceSha256
        pose = [string]$Plan.Pose
    })
    Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'processing_fence_observed' -Fields ([ordered]@{
        command = [string]$Plan.FenceCommand
        marker = [string]$Plan.FenceMarker
        stdout = [string]$markerEvidence.Line
    })
    return $resultEvidence
}

function Remove-BdsTickingArea {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [scriptblock]$WaitForAck
    )

    $activeProperty = $Handle.PSObject.Properties['ActiveTickingArea']
    if ($null -eq $activeProperty -or $null -eq $activeProperty.Value) {
        return $null
    }
    $active = $activeProperty.Value
    $hasExitedProperty = $Handle.Process.PSObject.Properties['HasExited']
    if ($null -ne $hasExitedProperty -and [bool]$hasExitedProperty.Value) {
        throw "BDS exited before ticking-area cleanup: $($active.Name)"
    }
    Write-BdsConsoleCommand `
        -Handle $Handle `
        -Command $active.CleanupCommand `
        -LogPath (Join-Path $RunDirectory 'bds.console.log')
    $rawEvidence = if ($null -eq $WaitForAck) {
        Wait-ProcessOutputMarker `
            -Handle $Handle `
            -Marker $active.CleanupMarker `
            -TimeoutSeconds 30 `
            -RejectMarker ' ERROR] ' `
            -PassThruEvidence
    }
    else {
        & $WaitForAck $Handle $active.CleanupMarker 30
    }
    $markerEvidence = Get-RequiredBdsMarkerEvidence `
        -Evidence $rawEvidence `
        -Context 'ticking-area cleanup wait'
    $expectedPattern = '^(?:NO LOG FILE! - )?\[[^\]\r\n]+ INFO\] ' + [regex]::Escape([string]$active.CleanupMarker) + '$'
    if ([string]$markerEvidence.Line -notmatch $expectedPattern) {
        throw "invalid ticking-area cleanup acknowledgement: $($markerEvidence.Line)"
    }
    $result = [pscustomobject][ordered]@{
        name = [string]$active.Name
        command = [string]$active.CleanupCommand
        stdout = [string]$markerEvidence.Line
    }
    Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'load_area_removed' -Fields ([ordered]@{
        name = [string]$active.Name
        command = [string]$active.CleanupCommand
        stdout = [string]$markerEvidence.Line
    })
    $Handle.PSObject.Properties.Remove('ActiveTickingArea')
    return $result
}

function Publish-BaselineSourceMutation {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]
        [ValidateCount(3, 3)]
        [int[]]$Coordinate,
        [Parameter(Mandatory = $true)][string]$RunDirectory
    )

    $command = "setblock $($Coordinate[0]) $($Coordinate[1]) $($Coordinate[2]) minecraft:gold_block"
    Write-BdsConsoleCommand `
        -Handle $Handle `
        -Command $command `
        -LogPath (Join-Path $RunDirectory 'bds.console.log')
    Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'source_mutation_command' -Fields ([ordered]@{
        coordinate = $Coordinate -join ','
        block = 'minecraft:gold_block'
        command = $command
    })
    return $command
}

function Publish-VisualFixture {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [ValidateRange(0, 10000)][int]$SettleMilliseconds = 3000,
        [ValidateRange(-1, 10000)][int]$PreloadSettleMilliseconds = -1,
        [scriptblock]$WaitForLoadArea,
        [scriptblock]$WaitForFence,
        $AppHandle,
        [scriptblock]$WaitForAppMarker
    )

    $consoleLogPath = Join-Path $RunDirectory 'bds.console.log'
    $fixtureCommandsProperty = $Plan.PSObject.Properties['FixtureCommands']
    $fixtureCommands = if ($null -eq $fixtureCommandsProperty) {
        @($Plan.GalleryCommands)
    }
    else {
        @($fixtureCommandsProperty.Value)
    }
    $fixtureKindProperty = $Plan.Manifest.PSObject.Properties['fixture_kind']
    $isModelWitnessGallery = $null -ne $fixtureKindProperty -and @('SlabStairGallery', 'VineGallery') -ccontains [string]$fixtureKindProperty.Value
    $isV2 = [string]$Plan.Manifest.schema -ceq 'rust-mcbe-visual-fixture-v2'
    if ($isV2) {
        $null = Start-BdsFixtureLoadArea `
            -Handle $Handle `
            -Plan $Plan `
            -RunDirectory $RunDirectory `
            -SettleMilliseconds $PreloadSettleMilliseconds `
            -WaitForLoadArea $WaitForLoadArea
        if ($isV2 -and $isModelWitnessGallery) {
            if ($null -eq $AppHandle) {
                throw 'gallery witness acceptance requires AppHandle for causal GPU evidence'
            }
            $null = Advance-ProcessOutputCursorToCurrentEnd -Handle $AppHandle
            Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand -LogPath $consoleLogPath
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'teleport_issued' -Fields ([ordered]@{
                command = [string]$Plan.TeleportCommand
            })
            Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
            if ($null -eq $WaitForFence) {
                $modelCameraFenceEvidence = Wait-ProcessOutputMarker `
                    -Handle $Handle `
                    -Marker $Plan.FenceMarker `
                    -TimeoutSeconds 30 `
                    -PassThruEvidence
            }
            else {
                $modelCameraFenceEvidence = & $WaitForFence $Handle $Plan.FenceMarker 30
            }
            $modelCameraResultLine = Assert-BdsCameraResortResult -Evidence $modelCameraFenceEvidence
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'model_witness_camera_fence_observed' -Fields ([ordered]@{
                command = [string]$Plan.FenceCommand
                stdout_marker = [string]$Plan.FenceMarker
                result_line = [string]$modelCameraResultLine
            })
            if ($null -eq $WaitForAppMarker) {
                $modelCameraCommitEvidence = Wait-ProcessOutputMarker `
                    -Handle $AppHandle `
                    -Marker 'RUST_MCBE_CAMERA_COMMITTED ' `
                    -TimeoutSeconds 60 `
                    -PassThruEvidence
            }
            else {
                $modelCameraCommitEvidence = & $WaitForAppMarker $AppHandle 'RUST_MCBE_CAMERA_COMMITTED ' 60
            }
            $modelCameraCommit = ConvertFrom-CameraCommittedMarker -Line ([string]$modelCameraCommitEvidence.Line)
            $null = Assert-ModelGalleryCommittedCamera -Committed $modelCameraCommit -Target $Plan.CameraTarget
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'model_witness_camera_committed' -Fields ([ordered]@{
                control_sequence = [uint64]$modelCameraCommit.sequence
                position = @($modelCameraCommit.position) -join ','
                yaw = [double]$modelCameraCommit.yaw
                pitch = [double]$modelCameraCommit.pitch
            })
        }
        $null = Complete-BdsFixtureCommandBatch `
            -Handle $Handle `
            -Plan $Plan `
            -RunDirectory $RunDirectory `
            -WaitForFence $WaitForFence
    }
    else {
        foreach ($command in $fixtureCommands) {
            Write-BdsConsoleCommand -Handle $Handle -Command $command -LogPath $consoleLogPath
        }
        Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
        if ($null -eq $WaitForFence) {
            $fixtureFenceEvidence = Wait-ProcessOutputMarker -Handle $Handle -Marker $Plan.FenceMarker -TimeoutSeconds 30 -PassThruEvidence
        }
        else {
            $fixtureFenceEvidence = & $WaitForFence $Handle $Plan.FenceMarker 30
        }
        $validateResultsProperty = $Plan.PSObject.Properties['ValidateFixtureCommandResults']
        if ($null -ne $validateResultsProperty -and [bool]$validateResultsProperty.Value) {
            $skippedProperty = $fixtureFenceEvidence.PSObject.Properties['SkippedLines']
            if ($null -eq $skippedProperty) {
                throw 'water fixture fence did not retain command output for validation'
            }
            $fixtureResultEvidence = Assert-BdsFixtureCommandResults -Commands $fixtureCommands -Lines @($skippedProperty.Value)
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'fixture_commands_validated' -Fields ([ordered]@{
                result_count = [uint64]$fixtureResultEvidence.result_count
                stdout_sha256 = [string]$fixtureResultEvidence.stdout_sha256
            })
        }
    }

    $readyPath = Join-Path $RunDirectory 'visual-fixture-ready.json'
    $publication = $null
    if ($isV2) {
        $publication = Publish-FixtureManifest -Plan $Plan -Path $readyPath
        $targetMutationProperty = $Plan.Manifest.PSObject.Properties['target_mutation']
        $targetMutation = if ($null -eq $targetMutationProperty) {
            $null
        }
        else {
            $value = $targetMutationProperty.Value
            Assert-PublishedTargetMutation -Path $publication.Path -Expected $value
            "$($value.x),$($value.y),$($value.z)"
        }
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'visual_fixture_ready' -Fields ([ordered]@{
            path = $publication.Path
            manifest_sha256 = $publication.ManifestSha256
            fixture_layout_hash = $publication.LayoutHash
            target_mutation = $targetMutation
        })
    }
    $cameraResortProperty = $Plan.PSObject.Properties['CameraResortCommand']
    if ($null -ne $cameraResortProperty -and -not [string]::IsNullOrWhiteSpace([string]$cameraResortProperty.Value)) {
        if ($null -eq $AppHandle) {
            throw 'gallery witness acceptance requires AppHandle for causal GPU evidence'
        }
        $null = Advance-ProcessOutputCursorToCurrentEnd -Handle $AppHandle
    }
    if (-not $isModelWitnessGallery) {
        Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand -LogPath $consoleLogPath
        if ($isV2) {
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'teleport_issued' -Fields ([ordered]@{
                command = [string]$Plan.TeleportCommand
            })
        }
    }
    $remainingSettleMilliseconds = $SettleMilliseconds
    if ($isModelWitnessGallery) {
        $modelRequest = New-ModelGalleryWitnessRequest -Plan $Plan -Revision 1
        $modelRequestPath = Join-Path $RunDirectory 'model-witness-request.json'
        $null = Write-AtomicJsonArtifact -Path $modelRequestPath -Value $modelRequest
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'model_witness_request_published' -Fields ([ordered]@{
            path = $modelRequestPath
            revision = [uint64]$modelRequest.revision
            request_sha256 = [string]$modelRequest.request_sha256
            key_count = [uint64]@($modelRequest.sub_chunks).Count
        })
        $modelWitnesses = [Collections.Generic.List[object]]::new()
        $modelEvidence = [Collections.Generic.List[object]]::new()
        foreach ($expectedConsecutive in 1..2) {
            if ($null -eq $WaitForAppMarker) {
                $evidence = Wait-ProcessOutputMarker `
                    -Handle $AppHandle `
                    -Marker 'RUST_MCBE_MODEL_WITNESS_COMPLETE ' `
                    -TimeoutSeconds 30 `
                    -PassThruEvidence
            }
            else {
                $evidence = & $WaitForAppMarker $AppHandle 'RUST_MCBE_MODEL_WITNESS_COMPLETE ' 30
            }
            $witness = ConvertFrom-ModelWitnessCompleteMarker -Line ([string]$evidence.Line)
            if ([int]$witness.consecutive -ne $expectedConsecutive) {
                throw "model witness completion was duplicate or out of order: expected=$expectedConsecutive actual=$($witness.consecutive)"
            }
            $modelWitnesses.Add($witness)
            $modelEvidence.Add($evidence)
            Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'model_witness_complete' -Fields ([ordered]@{
                revision = [uint64]$witness.revision
                request_sha256 = [string]$witness.request_sha256
                sequence = [uint64]$witness.sequence
                view_generation = [uint64]$witness.view_generation
                key_count = [uint64]$witness.key_count
                model_ref_count = [uint64]$witness.model_ref_count
                manifest_count = [uint64]$witness.manifest_count
                manifest_sha256 = [string]$witness.manifest_sha256
                consecutive = [int]$witness.consecutive
                stdout_line = [uint64]$evidence.LineNumber
            })
        }
        if ([uint64]$modelEvidence[0].LineNumber -eq 0 -or
            [uint64]$modelEvidence[1].LineNumber -le [uint64]$modelEvidence[0].LineNumber) {
            throw 'model witness markers were stale, duplicated, or non-causal in app stdout'
        }
        $null = Assert-StableModelWitnessEvidence `
            -Request $modelRequest `
            -First $modelWitnesses[0] `
            -Second $modelWitnesses[1]
    }
    if ($null -ne $cameraResortProperty -and -not [string]::IsNullOrWhiteSpace([string]$cameraResortProperty.Value)) {
        Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
        if ($null -eq $WaitForFence) {
            $initialCameraFenceEvidence = Wait-ProcessOutputMarker `
                -Handle $Handle `
                -Marker $Plan.FenceMarker `
                -TimeoutSeconds 30 `
                -PassThruEvidence
        }
        else {
            $initialCameraFenceEvidence = & $WaitForFence $Handle $Plan.FenceMarker 30
        }
        $initialCameraResultLine = Assert-BdsCameraResortResult -Evidence $initialCameraFenceEvidence
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'initial_camera_fence_observed' -Fields ([ordered]@{
            command = [string]$Plan.FenceCommand
            stdout_marker = [string]$Plan.FenceMarker
            result_line = [string]$initialCameraResultLine
        })
        $initialPoseSettle = [Math]::Min(1000, $SettleMilliseconds)
        if ($initialPoseSettle -gt 0) {
            Start-Sleep -Milliseconds $initialPoseSettle
        }
        if ($null -eq $WaitForAppMarker) {
            $initialSortEvidence = Wait-ProcessOutputMarker -Handle $AppHandle -Marker 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED ' -TimeoutSeconds 30 -PassThruEvidence
        }
        else {
            $initialSortEvidence = & $WaitForAppMarker $AppHandle 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED ' 30
        }
        $initialSort = ConvertFrom-TransparentSortCommittedMarker -Line ([string]$initialSortEvidence.Line)
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'initial_transparent_sort_committed' -Fields ([ordered]@{
            generation = [uint64]$initialSort.generation
            ref_count = [uint64]$initialSort.ref_count
            stdout_line = [uint64]$initialSortEvidence.LineNumber
        })
        $null = Advance-ProcessOutputCursorToCurrentEnd -Handle $AppHandle
        Write-BdsConsoleCommand -Handle $Handle -Command ([string]$cameraResortProperty.Value) -LogPath $consoleLogPath
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'camera_resort_issued' -Fields ([ordered]@{
            command = [string]$cameraResortProperty.Value
        })
        Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
        if ($null -eq $WaitForFence) {
            $resortFenceEvidence = Wait-ProcessOutputMarker `
                -Handle $Handle `
                -Marker $Plan.FenceMarker `
                -TimeoutSeconds 30 `
                -PassThruEvidence
        }
        else {
            $resortFenceEvidence = & $WaitForFence $Handle $Plan.FenceMarker 30
        }
        $resortResultLine = Assert-BdsCameraResortResult -Evidence $resortFenceEvidence
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'camera_resort_fence_observed' -Fields ([ordered]@{
            command = [string]$Plan.FenceCommand
            stdout_marker = [string]$Plan.FenceMarker
            result_line = [string]$resortResultLine
        })
        if ($null -eq $WaitForAppMarker) {
            $resortSortEvidence = Wait-ProcessOutputMarker -Handle $AppHandle -Marker 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED ' -TimeoutSeconds 30 -PassThruEvidence
        }
        else {
            $resortSortEvidence = & $WaitForAppMarker $AppHandle 'RUST_MCBE_TRANSPARENT_SORT_COMMITTED ' 30
        }
        $resortSort = ConvertFrom-TransparentSortCommittedMarker -Line ([string]$resortSortEvidence.Line)
        $null = Assert-NewerTransparentSortCommit `
            -Initial $initialSort `
            -InitialLineNumber ([uint64]$initialSortEvidence.LineNumber) `
            -Resort $resortSort `
            -ResortLineNumber ([uint64]$resortSortEvidence.LineNumber)
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'resort_transparent_sort_committed' -Fields ([ordered]@{
            generation = [uint64]$resortSort.generation
            ref_count = [uint64]$resortSort.ref_count
            stdout_line = [uint64]$resortSortEvidence.LineNumber
        })
        $liquidWitnessProperty = $Plan.PSObject.Properties['LiquidWitnessBlocks']
        if ($null -ne $liquidWitnessProperty) {
            $witnessRequest = New-WaterGalleryTransparentWitnessRequest -Plan $Plan -Revision 1
            $witnessRequestPath = Join-Path $RunDirectory 'transparent-witness-request.json'
            $null = Advance-ProcessOutputCursorToCurrentEnd -Handle $AppHandle
            $null = Write-AtomicJsonArtifact -Path $witnessRequestPath -Value $witnessRequest
            $witnesses = [Collections.Generic.List[object]]::new()
            foreach ($expectedConsecutive in 1..2) {
                if ($null -eq $WaitForAppMarker) {
                    $evidence = Wait-ProcessOutputMarker `
                        -Handle $AppHandle `
                        -Marker 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE ' `
                        -TimeoutSeconds 30 `
                        -PassThruEvidence
                }
                else {
                    $evidence = & $WaitForAppMarker $AppHandle 'RUST_MCBE_TRANSPARENT_WITNESS_COMPLETE ' 30
                }
                $witness = ConvertFrom-TransparentWitnessCompleteMarker -Line ([string]$evidence.Line)
                if ([int]$witness.consecutive -ne $expectedConsecutive) {
                    throw "transparent witness did not complete twice consecutively: expected=$expectedConsecutive actual=$($witness.consecutive)"
                }
                $witnesses.Add($witness)
                Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'transparent_witness_complete' -Fields ([ordered]@{
                    revision = [uint64]$witness.revision
                    sequence = [uint64]$witness.sequence
                    generation = [uint64]$witness.generation
                    key_count = [uint64]$witness.key_count
                    consecutive = [int]$witness.consecutive
                    stdout_line = [uint64]$evidence.LineNumber
                })
            }
            $null = Assert-StableTransparentWitnessEvidence `
                -Request $witnessRequest `
                -First $witnesses[0] `
                -Second $witnesses[1]
        }
        $remainingSettleMilliseconds -= $initialPoseSettle
    }
    if ($remainingSettleMilliseconds -gt 0) {
        Start-Sleep -Milliseconds $remainingSettleMilliseconds
    }
    if (-not $isV2) {
        $publication = Publish-FixtureManifest -Plan $Plan -Path $readyPath
    }
    return $publication
}

function Publish-FullViewTeleport {
    param(
        [Parameter(Mandatory = $true)]$Handle,
        [Parameter(Mandatory = $true)]$Plan,
        [Parameter(Mandatory = $true)][string]$RunDirectory,
        [ValidateRange(-1, 10000)][int]$PreloadSettleMilliseconds = -1,
        [scriptblock]$WaitForLoadArea,
        [scriptblock]$WaitForFence
    )

    $consoleLogPath = Join-Path $RunDirectory 'bds.console.log'
    $fixtureCommandsProperty = $Plan.PSObject.Properties['FixtureCommands']
    $isLeafForest = [string]$Plan.Manifest.schema -ceq 'rust-mcbe-visual-fixture-v2' -and
        $null -ne $fixtureCommandsProperty
    if ($isLeafForest) {
        $null = Start-BdsFixtureLoadArea `
            -Handle $Handle `
            -Plan $Plan `
            -RunDirectory $RunDirectory `
            -SettleMilliseconds $PreloadSettleMilliseconds `
            -WaitForLoadArea $WaitForLoadArea
        $null = Complete-BdsFixtureCommandBatch `
            -Handle $Handle `
            -Plan $Plan `
            -RunDirectory $RunDirectory `
            -WaitForFence $WaitForFence
        $readyPath = Join-Path $RunDirectory 'visual-fixture-ready.json'
        $publication = Publish-FixtureManifest -Plan $Plan -Path $readyPath
        $targetMutation = $Plan.TargetMutation
        Assert-PublishedTargetMutation -Path $publication.Path -Expected $targetMutation
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'visual_fixture_ready' -Fields ([ordered]@{
            path = $publication.Path
            manifest_sha256 = $publication.ManifestSha256
            fixture_layout_hash = $publication.LayoutHash
            target_mutation = "$($targetMutation.x),$($targetMutation.y),$($targetMutation.z)"
        })
        Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand -LogPath $consoleLogPath
        Write-AcceptanceEvent -RunDirectory $RunDirectory -Event 'teleport_issued' -Fields ([ordered]@{
            command = [string]$Plan.TeleportCommand
        })
        return $publication
    }

    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.FenceCommand -LogPath $consoleLogPath
    if ($null -eq $WaitForFence) {
        $null = Wait-ProcessOutputMarker -Handle $Handle -Marker $Plan.FenceMarker -TimeoutSeconds 30
    }
    else {
        $null = & $WaitForFence $Handle $Plan.FenceMarker 30
    }
    Write-BdsConsoleCommand -Handle $Handle -Command $Plan.TeleportCommand -LogPath $consoleLogPath

    $planPath = Join-Path $RunDirectory 'full-view-teleport-plan.json'
    $manifestSha256 = Write-AtomicJsonArtifact -Path $planPath -Value $Plan.Manifest
    [Console]::Out.WriteLine("FULL_VIEW_TELEPORT_PLAN=$planPath")
    return [pscustomobject][ordered]@{
        Path = $planPath
        ManifestSha256 = $manifestSha256
        LayoutHash = $null
        Pose = 'FullViewTeleport'
    }
}

function Invoke-CinnabarAcceptance {
    param(
        [switch]$DryRun,
        [Parameter(Mandatory = $true)]
        [ValidateRange(1, [int]::MaxValue)]
        [int]$DurationSeconds,
        [Parameter(Mandatory = $true)]
        [string]$BdsDir,
        [string]$BdsRuntimeDirectory,
        [Parameter(Mandatory = $true)]
        [string]$MetricsOut,
        [string]$Assets,
        [ValidateSet('None', 'Front', 'Back', 'LeafGalleryFront', 'LeafGalleryBack', 'CrossCropGalleryFront', 'CrossCropGalleryBack', 'AquaticGalleryFront', 'AquaticGalleryBack', 'WaterGalleryFront', 'WaterGalleryBack', 'FlowerBedGalleryTop', 'FlowerBedGalleryNorth', 'FlowerBedGalleryEast', 'FlowerBedGalleryOblique', 'FlowerBedGalleryObliqueOpposite', 'SlabStairGalleryTop', 'SlabStairGalleryNorth', 'SlabStairGalleryEast', 'SlabStairGalleryOblique', 'SlabStairGalleryObliqueOpposite', 'VineGalleryTop', 'VineGalleryNorth', 'VineGalleryEast', 'VineGalleryOblique', 'VineGalleryObliqueOpposite')]
        [string]$VisualFixturePose = 'None',
        [switch]$FullViewTeleportGate,
        [switch]$LeafForestBaseline,
        [switch]$LeafForestFullView,
        [string]$ClientExecutable,
        [switch]$SkipClientBuild,
        [switch]$UseVsync,
        [switch]$NoVsync,
        [string]$SteadyResourceTrigger
    )

    $AcceptanceBoundParameters = @{} + $PSBoundParameters
    . $script:AcceptanceValidationPhase
    . $script:AcceptanceExecutionPhase
}
