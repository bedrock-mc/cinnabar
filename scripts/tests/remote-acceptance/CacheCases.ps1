    It 'classifies cache boundary negotiation, disabled ordinary, capable ordinary, and cache-backed routes' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-cache-boundary-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $path = Join-Path $temporary 'core.stderr.log'
            $write = {
                param(
                    [bool]$Seen,
                    [bool]$Enabled,
                    [uint64]$CachedLevel,
                    [uint64]$OrdinaryLevel,
                    [uint64]$CachedSub,
                    [uint64]$OrdinarySub
                )
                "time=sentinel level=INFO msg=PHASE2_CACHE_BOUNDARY upstream_status_seen=$($Seen.ToString().ToLowerInvariant()) upstream_status_enabled=$($Enabled.ToString().ToLowerInvariant()) cached_level_chunks=$CachedLevel ordinary_level_chunks=$OrdinaryLevel cached_sub_chunks=$CachedSub ordinary_sub_chunks=$OrdinarySub" |
                    Set-Content -LiteralPath $path
            }

            & $write $false $false 0 0 0 0
            (Get-Phase2CacheBoundaryEvidence -CoreLogPath $path).Classification |
                Should Be 'negotiation_failure'

            & $write $true $false 0 177 0 3894
            (Get-Phase2CacheBoundaryEvidence -CoreLogPath $path).Classification |
                Should Be 'ordinary_payload_cache_disabled'

            & $write $true $true 0 177 0 3894
            (Get-Phase2CacheBoundaryEvidence -CoreLogPath $path).Classification |
                Should Be 'server_ordinary_despite_cache_capability'

            & $write $true $true 3 174 29 3865
            (Get-Phase2CacheBoundaryEvidence -CoreLogPath $path).Classification |
                Should Be 'cache_backed'
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects missing duplicate malformed and incoherent cache boundary evidence' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-cache-boundary-invalid-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $path = Join-Path $temporary 'core.stderr.log'
            Set-Content -LiteralPath $path -Value 'no cache boundary marker'
            { Get-Phase2CacheBoundaryEvidence -CoreLogPath $path } | Should Throw

            $valid = 'time=sentinel level=INFO msg=PHASE2_CACHE_BOUNDARY upstream_status_seen=true upstream_status_enabled=true cached_level_chunks=0 ordinary_level_chunks=1 cached_sub_chunks=0 ordinary_sub_chunks=1'
            @($valid, $valid) | Set-Content -LiteralPath $path
            { Get-Phase2CacheBoundaryEvidence -CoreLogPath $path } | Should Throw

            ($valid + ' ' + $valid) | Set-Content -LiteralPath $path
            { Get-Phase2CacheBoundaryEvidence -CoreLogPath $path } | Should Throw

            ($valid + ' extra_field=1') | Set-Content -LiteralPath $path
            { Get-Phase2CacheBoundaryEvidence -CoreLogPath $path } | Should Throw

            $valid.Replace('upstream_status_seen=true', 'upstream_status_seen=false') |
                Set-Content -LiteralPath $path
            { Get-Phase2CacheBoundaryEvidence -CoreLogPath $path } | Should Throw

            'time=sentinel level=INFO msg=PHASE2_CACHE_BOUNDARY upstream_status_seen=true upstream_status_enabled=true cached_level_chunks=0 ordinary_level_chunks=0 cached_sub_chunks=0 ordinary_sub_chunks=0' |
                Set-Content -LiteralPath $path
            { Get-Phase2CacheBoundaryEvidence -CoreLogPath $path } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects Lunar publication and independent cache boundary contradictions' {
        $ordinaryBoundary = [pscustomobject][ordered]@{
            classification = 'server_ordinary_despite_cache_capability'
            upstream_status_seen = $true
            upstream_status_enabled = $true
            cached_level_chunks = [uint64]0
            ordinary_level_chunks = [uint64]177
            cached_sub_chunks = [uint64]0
            ordinary_sub_chunks = [uint64]3894
        }
        { Assert-Phase2CacheBoundaryConsistency -Server Lunar `
            -ClientBlobCacheRoute cache_backed -BoundaryEvidence $ordinaryBoundary } |
            Should Throw

        { Assert-Phase2CacheBoundaryConsistency -Server Zeqa `
            -ClientBlobCacheRoute ordinary_payload -BoundaryEvidence $ordinaryBoundary } |
            Should Not Throw

        $cacheBackedBoundary = $ordinaryBoundary.PSObject.Copy()
        $cacheBackedBoundary.classification = 'cache_backed'
        $cacheBackedBoundary.cached_level_chunks = [uint64]1
        { Assert-Phase2CacheBoundaryConsistency -Server Lunar `
            -ClientBlobCacheRoute cache_backed -BoundaryEvidence $cacheBackedBoundary } |
            Should Not Throw

        $disabledBoundary = $ordinaryBoundary.PSObject.Copy()
        $disabledBoundary.classification = 'ordinary_payload_cache_disabled'
        $disabledBoundary.upstream_status_enabled = $false
        { Assert-Phase2CacheBoundaryConsistency -Server Lunar `
            -ClientBlobCacheRoute ordinary_payload -BoundaryEvidence $disabledBoundary } |
            Should Not Throw
        { Assert-Phase2CacheBoundaryConsistency -Server Lunar `
            -ClientBlobCacheRoute cache_backed -BoundaryEvidence $disabledBoundary } |
            Should Throw

        $unknownBoundary = $ordinaryBoundary.PSObject.Copy()
        $unknownBoundary.classification = 'future_unproven_route'
        { Assert-Phase2CacheBoundaryConsistency -Server Lunar `
            -ClientBlobCacheRoute ordinary_payload -BoundaryEvidence $unknownBoundary } |
            Should Throw
        { Assert-Phase2CacheBoundaryConsistency -Server Zeqa `
            -ClientBlobCacheRoute ordinary_payload -BoundaryEvidence $unknownBoundary } |
            Should Throw

        { Assert-Phase2CacheBoundaryConsistency -Server Zeqa `
            -ClientBlobCacheRoute cache_backed -BoundaryEvidence $ordinaryBoundary } |
            Should Throw
        { Assert-Phase2CacheBoundaryConsistency -Server Zeqa `
            -ClientBlobCacheRoute ordinary_payload -BoundaryEvidence $cacheBackedBoundary } |
            Should Throw

        $negotiationBoundary = $ordinaryBoundary.PSObject.Copy()
        $negotiationBoundary.classification = 'negotiation_failure'
        $negotiationBoundary.upstream_status_seen = $false
        $negotiationBoundary.upstream_status_enabled = $false
        { Assert-Phase2CacheBoundaryConsistency -Server Zeqa `
            -ClientBlobCacheRoute ordinary_payload -BoundaryEvidence $negotiationBoundary } |
            Should Not Throw
    }

    It 'accepts disabled ordinary Lunar compatibility without claiming cache-backed evidence' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-cache-boundary-manifest-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $clientPath = Join-Path $temporary 'client.log'
            $corePath = Join-Path $temporary 'core.stderr.log'
            $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $record.client_blob_cache_enabled = $false
            'PHASE2_PUBLICATION=' + ($record | ConvertTo-Json -Depth 20 -Compress) |
                Set-Content -LiteralPath $clientPath
            'time=sentinel level=INFO msg=PHASE2_CACHE_BOUNDARY upstream_status_seen=true upstream_status_enabled=false cached_level_chunks=0 ordinary_level_chunks=177 cached_sub_chunks=0 ordinary_sub_chunks=3894' |
                Set-Content -LiteralPath $corePath
            $manifest = [pscustomobject][ordered]@{ mode = 'Diagnostic'; initial_radius = 16 }

            { Complete-Phase2DiagnosticEvidence -Manifest $manifest -ClientLogPath $clientPath `
                -CoreLogPath $corePath -ExpectedPresentMode Fifo -WorldReadyObserved:$false `
                -Server Lunar } | Should Not Throw

            $manifest.cache_boundary_evidence.classification |
                Should Be 'ordinary_payload_cache_disabled'
            $manifest.cache_boundary_evidence.ordinary_sub_chunks | Should Be 3894
            $manifest.client_blob_cache_route | Should Be 'ordinary_payload'
            (@($manifest.findings) -ccontains 'client_blob_cache_performance_gate_deferred') | Should Be $true
            (@($manifest.PSObject.Properties.Name) -contains 'final_publication') | Should Be $true
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects client blob cache counter regression and enablement changes across a sequence' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-cache-sequence-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $first = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $first.client_blob_cache.hashes_classified = 7
            $first.client_blob_cache.hits = 3
            $first.client_blob_cache.misses = 4
            $first.client_blob_cache.admitted_blobs = 4

            $regressed = $first | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            $regressed.client_blob_cache.hits = 2
            $regressed.client_blob_cache.misses = 5
            $regressionPath = Join-Path $temporary 'regression.log'
            @(
                'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress),
                'PHASE2_PUBLICATION=' + ($regressed | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $regressionPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $regressionPath `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw

            $disabled = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $disabled.client_blob_cache_enabled = $false
            $enablementPath = Join-Path $temporary 'enablement.log'
            @(
                'PHASE2_PUBLICATION=' + ((New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                    -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894) |
                    ConvertTo-Json -Depth 20 -Compress),
                'PHASE2_PUBLICATION=' + ($disabled | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $enablementPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $enablementPath `
                -ExpectedPresentMode Fifo -WorldReadyObserved:$false -Server Zeqa } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'does not classify a cohort identity gap across malformed response outcomes' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
            -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
        $record.publication.outcomes.malformed = 1
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$false) |
            Should Be 'response_semantics'
    }

    It 'classifies a complete cohort with a no-ready mesh backlog as meshing' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 4000
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$false) |
            Should Be 'meshing'
    }

    It 'requires empty downstream work before terminal none after world ready' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 4
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$true) |
            Should Be 'meshing'
    }

    It 'requires the exact raw block radius schema and derived retention radius' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-radius-schema-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $invalidValues = @($null, $true, '128', 128.5, -1, [decimal]18446744073709551616)
            $case = 0
            foreach ($invalid in $invalidValues) {
                $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                    -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894 `
                    -PublisherRadiusBlocks $invalid -PublisherRadius 8
                $path = Join-Path $temporary ("invalid-$case.log")
                'PHASE2_PUBLICATION=' + ($record | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $path
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path -ExpectedPresentMode Fifo `
                    -WorldReadyObserved:$false -Server Zeqa } | Should Throw
                $case++
            }

            $missing = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $missing.publication.Remove('publisher_radius_blocks')
            $missingPath = Join-Path $temporary 'missing.log'
            'PHASE2_PUBLICATION=' + ($missing | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $missingPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $missingPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false -Server Zeqa } | Should Throw

            $wrongDerived = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894 `
                -PublisherRadiusBlocks 120 -PublisherRadius 7
            $wrongPath = Join-Path $temporary 'wrong-derived.log'
            'PHASE2_PUBLICATION=' + ($wrongDerived | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $wrongPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $wrongPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false -Server Zeqa } | Should Throw

            foreach ($geometry in @(
                @{ blocks = 120; chunks = 8; columns = 177 },
                @{ blocks = 128; chunks = 8; columns = 197 },
                @{ blocks = 256; chunks = 16; columns = 797 }
            )) {
                $valid = New-SyntheticPhase2Publication -RequiredColumns $geometry.columns -LoadedColumns $geometry.columns `
                    -RequestsConstructed $geometry.columns -RequestsSent $geometry.columns `
                    -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
                    -PublisherRadiusBlocks $geometry.blocks -PublisherRadius $geometry.chunks
                $parsed = $valid | ConvertTo-Json -Depth 20 | ConvertFrom-Json
                { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
                    Should Not Throw
            }
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects unknown fields, inexact integral fields, and mixed sequence identities' {
        $temporary = Join-Path ([IO.Path]::GetTempPath()) ('phase2-strict-schema-' + [guid]::NewGuid().ToString('N'))
        try {
            New-Item -ItemType Directory -Path $temporary | Out-Null
            $unknown = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
            $unknown.presentation['access_token'] = 'must-not-pass'
            $unknownPath = Join-Path $temporary 'unknown.log'
            'PHASE2_PUBLICATION=' + ($unknown | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $unknownPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $unknownPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false -Server Zeqa } | Should Throw

            foreach ($invalid in @($null, $true, '1', 1.5, -1, [decimal]18446744073709551616)) {
                $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 177 `
                    -RequestsConstructed 177 -RequestsSent 177 -ResponsesAdmitted 3894 -SubchunksCommitted 3894
                $record.publication.stages.requests_sent = $invalid
                $path = Join-Path $temporary ("stage-$([guid]::NewGuid().ToString('N')).log")
                'PHASE2_PUBLICATION=' + ($record | ConvertTo-Json -Depth 20 -Compress) | Set-Content -LiteralPath $path
                { Get-Phase2PublicationSequenceEvidence -ClientLogPath $path -ExpectedPresentMode Fifo `
                    -WorldReadyObserved:$false -Server Zeqa } | Should Throw
            }

            $first = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 1 `
                -RequestsConstructed 1 -RequestsSent 1 -ResponsesAdmitted 22 -SubchunksCommitted 22
            $mixed = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 2 `
                -RequestsConstructed 2 -RequestsSent 2 -ResponsesAdmitted 44 -SubchunksCommitted 44
            $mixed.publication.required_cohort_hash = '2222222222222222'
            foreach ($name in @('publisher_disk', 'resident', 'allocation', 'visible', 'submitted', 'gpu_presented')) {
                $mixed.presentation.$name.required_cohort_hash = '2222222222222222'
            }
            $mixedPath = Join-Path $temporary 'mixed.log'
            @(
                'PHASE2_PUBLICATION=' + ($first | ConvertTo-Json -Depth 20 -Compress)
                'PHASE2_PUBLICATION=' + ($mixed | ConvertTo-Json -Depth 20 -Compress)
            ) | Set-Content -LiteralPath $mixedPath
            { Get-Phase2PublicationSequenceEvidence -ClientLogPath $mixedPath -ExpectedPresentMode Fifo `
                -WorldReadyObserved:$false -Server Zeqa } | Should Throw
        }
        finally {
            Remove-Item -LiteralPath $temporary -Recurse -Force -ErrorAction SilentlyContinue
        }
    }

    It 'rejects terminal none with adversarial mesh and upload backlog' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096 `
            -MeshJobsCompleted 12 -MeshJobsQueued 400000 -UploadsAcknowledged 12 -UploadsUnacknowledged 500000
        (Get-Phase2FirstStalledStage -PublicationRecord ([pscustomobject]$record) -WorldReadyObserved:$true) |
            Should Be 'meshing'
    }

    It 'accepts coherent transport-pending requests and rejects an incoherent handoff gauge' {
        $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 4 `
            -RequestsConstructed 4 -RequestsSent 0 -RequestsTransportPending 4 `
            -ResponsesAdmitted 0 -SubchunksCommitted 0
        $parsed = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json

        { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
            Should Not Throw
        (Get-Phase2FirstStalledStage -PublicationRecord $parsed -WorldReadyObserved:$false) |
            Should Be 'transport'

        $record.publication.stages.requests_transport_pending = 5
        $incoherent = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        { Assert-Phase2PublicationRecord -Record $incoherent -ExpectedPresentMode Fifo } |
            Should Throw
    }

    It 'rejects incoherent stage gauges and response outcomes before terminal classification' {
        $mutations = @(
            { param($record) $record.publication.stages.decode_jobs_dispatched = 1; $record.publication.stages.decode_jobs_completed = 2 },
            { param($record) $record.publication.stages.light_jobs_dispatched = 1; $record.publication.stages.light_jobs_completed = 2 },
            { param($record) $record.publication.stages.mesh_jobs_dispatched = 1; $record.publication.stages.mesh_jobs_completed = 2 },
            { param($record) $record.publication.stages.mesh_changes_queued = 1; $record.publication.stages.mesh_changes_dequeued = 2 },
            { param($record) $record.publication.stages.mesh_changes_queued = 100; $record.publication.stages.mesh_changes_dequeued = 0; $record.publication.stages.mesh_changes_pending = 0 },
            { param($record) foreach ($name in @('success','all_air','unavailable','malformed','stale','timed_out')) { $record.publication.outcomes[$name] = 0 } },
            { param($record) $record.publication.stages.responses_admitted = 4095 },
            { param($record) $record.publication.stages.subchunks_committed = 4097 }
        )
        foreach ($mutation in $mutations) {
            $record = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
                -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
            & $mutation $record
            $parsed = $record | ConvertTo-Json -Depth 20 | ConvertFrom-Json
            { Assert-Phase2PublicationRecord -Record $parsed -ExpectedPresentMode Fifo } |
                Should Throw
        }

        $adversarial = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
        foreach ($name in @('success','all_air','unavailable','malformed','stale','timed_out')) {
            $adversarial.publication.outcomes[$name] = 0
        }
        $adversarial.publication.stages.mesh_changes_queued = 100
        $adversarial.publication.stages.mesh_changes_dequeued = 0
        $adversarial.publication.stages.mesh_changes_pending = 0
        $parsedAdversarial = $adversarial | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        (Get-Phase2FirstStalledStage -PublicationRecord $parsedAdversarial -WorldReadyObserved:$true) |
            Should Not Be 'none'

        $incompleteCommit = New-SyntheticPhase2Publication -RequiredColumns 197 -LoadedColumns 197 `
            -RequestsConstructed 197 -RequestsSent 197 -ResponsesAdmitted 4096 -SubchunksCommitted 4096
        $incompleteCommit.publication.stages.subchunks_committed = 4095
        $parsedIncompleteCommit = $incompleteCommit | ConvertTo-Json -Depth 20 | ConvertFrom-Json
        (Get-Phase2FirstStalledStage -PublicationRecord $parsedIncompleteCommit -WorldReadyObserved:$true) |
            Should Be 'response_semantics'
    }
