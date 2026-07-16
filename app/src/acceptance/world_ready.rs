use crate::*;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorldReadyWork {
    pub(crate) network_events: usize,
    pub(crate) network_commands: usize,
    pub(crate) admitted_world_events: usize,
    pub(crate) queued_decode_jobs: usize,
    pub(crate) in_flight_decode_jobs: usize,
    pub(crate) completed_decode_results: usize,
    pub(crate) pending_light_jobs: usize,
    pub(crate) in_flight_light_jobs: usize,
    pub(crate) terminal_light_failures: usize,
    pub(crate) pending_mesh_jobs: usize,
    pub(crate) in_flight_mesh_jobs: usize,
    pub(crate) pending_mesh_changes: usize,
    pub(crate) outbound_requests: usize,
    pub(crate) outstanding_sub_chunks: usize,
    pub(crate) pending_retry_requests: usize,
    pub(crate) render_queue_items: usize,
    pub(crate) pending_gpu_acknowledgements: usize,
    pub(crate) unacknowledged_meshes: usize,
}

impl WorldReadyWork {
    pub(crate) fn is_empty(self) -> bool {
        self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SubChunkTimeoutProgress {
    pub(crate) awaiting_responses: usize,
    pub(crate) timeouts: u64,
    pub(crate) retries_scheduled: u64,
    pub(crate) retry_exhaustions: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WorldReadySnapshot {
    pub(crate) mutation_coordinate: Option<[i32; 3]>,
    pub(crate) received_radius_chunks: Option<i32>,
    pub(crate) publisher_radius_chunks: Option<i32>,
    pub(crate) rendered_sub_chunks: usize,
    pub(crate) resident_sub_chunks: usize,
    pub(crate) visible_sub_chunks: usize,
    pub(crate) mutation_target_rendered: bool,
    pub(crate) mutation_target_visible: bool,
    pub(crate) mutation_target_clean: bool,
    pub(crate) work: WorldReadyWork,
}

#[derive(Debug, Default)]
pub(crate) struct WorldReadySettler {
    pub(crate) candidate: Option<(WorldReadySnapshot, Instant)>,
}

impl WorldReadySettler {
    pub(crate) fn observe(
        &mut self,
        snapshot: WorldReadySnapshot,
        now: Instant,
    ) -> Option<[String; 2]> {
        let markers = world_ready_markers(snapshot);
        if markers.is_none() {
            self.candidate = None;
            return None;
        }
        match self.candidate {
            Some((stable, since)) if stable == snapshot => (now.saturating_duration_since(since)
                >= WORLD_READY_QUIET_INTERVAL)
                .then_some(markers.expect("settled snapshots have markers")),
            _ => {
                self.candidate = Some((snapshot, now));
                None
            }
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct GalleryAnchorEmitter {
    pub(crate) emitted: bool,
}

impl GalleryAnchorEmitter {
    pub(crate) fn observe(
        &mut self,
        enabled: bool,
        snapshot: WorldReadySnapshot,
    ) -> Option<String> {
        if self.emitted
            || !enabled
            || !snapshot.mutation_target_rendered
            || !snapshot.mutation_target_clean
        {
            return None;
        }
        let coordinate = snapshot.mutation_coordinate?;
        self.emitted = true;
        Some(format!(
            "{GALLERY_ANCHOR_READY} coordinate={},{},{} rendered=true visible={} clean=true",
            coordinate[0], coordinate[1], coordinate[2], snapshot.mutation_target_visible
        ))
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn emit_world_ready(
    network: Res<NetworkHandle>,
    mut client_world: ResMut<ClientWorld>,
    cache: Res<CaveVisibilityCache>,
    diagnostic_quads: Res<DiagnosticQuads>,
    render_queue: Res<ChunkRenderQueue>,
    model_witness_source: Res<ModelWitnessFileSource>,
    acknowledgements: Res<ChunkUploadAcknowledgements>,
    presented_frames: Res<PresentedFrameGate>,
    mut acceptance: ResMut<AcceptanceRun>,
    mut auto_fly: ResMut<camera::AutoFly>,
    mut metrics: ResMut<AppMetrics>,
) {
    let missing_mapping_count = client_world.runtime_assets.missing_count();
    let Some(stream) = client_world.stream.as_mut() else {
        return;
    };
    let stats = stream.stats();
    let timeout_progress = SubChunkTimeoutProgress {
        awaiting_responses: stats.awaiting_sub_chunk_responses,
        timeouts: stats.sub_chunk_timeouts,
        retries_scheduled: stats.sub_chunk_retries_scheduled,
        retry_exhaustions: stats.sub_chunk_retry_exhaustions,
    };
    let work = WorldReadyWork {
        network_events: network.pending_event_count(),
        network_commands: network.pending_command_count(),
        admitted_world_events: stats.admitted_world_events,
        queued_decode_jobs: stats.queued_decode_jobs,
        in_flight_decode_jobs: stats.in_flight_decode_jobs,
        completed_decode_results: stats.completed_decode_results,
        pending_light_jobs: stats.pending_light_jobs,
        in_flight_light_jobs: stats.in_flight_light_jobs,
        terminal_light_failures: stats.terminal_light_failures,
        pending_mesh_jobs: stats.pending_mesh_jobs,
        in_flight_mesh_jobs: stats.in_flight_mesh_jobs,
        pending_mesh_changes: stream.pending_mesh_change_count(),
        outbound_requests: stream.pending_request_work_count(),
        outstanding_sub_chunks: stream.outstanding_sub_chunk_count(),
        pending_retry_requests: stats.pending_retry_requests,
        render_queue_items: render_queue.retained_len(),
        pending_gpu_acknowledgements: usize::from(!acknowledgements.is_empty()),
        unacknowledged_meshes: stream.unacknowledged_mesh_count(),
    };
    if acceptance.world_ready {
        let cohort = acceptance
            .full_view_teleport
            .target_cohort()
            .map(|target| stream.cohort_status(target));
        if let Some(status) = cohort {
            debug_assert_eq!(stream.committed_view_cohort(), status.committed);
        }
        let snapshot = TeleportReadySnapshot {
            received_radius_chunks: stats.received_radius_chunks,
            publisher_radius_chunks: stats.publisher_radius_chunks,
            rendered_sub_chunks: cache.rendered.len(),
            resident_sub_chunks: stats.resident_sub_chunks,
            visible_sub_chunks: cache.visible_rendered,
            loaded_columns: stream.loaded_column_count(),
            cohort,
            last_chunk_commit_at: stats.last_chunk_commit_at,
            last_mesh_dispatch_at: stats.last_mesh_dispatch_at,
            last_mesh_completion_at: stats.last_mesh_completion_at,
            last_mesh_ack_at: stats.last_mesh_ack_at,
            work,
        };
        let observed_at = Instant::now();
        let frame_count = metrics.0.frame_count();
        acceptance.full_view_teleport.note_frame(frame_count);
        let teleport = if let Some(pending) = acceptance.full_view_teleport.pending.as_ref() {
            let proposed = render_queue.freeze_target_expectation(
                render_view_cohort(pending.target),
                Some(render_view_cohort(pending.source)),
                0,
                observed_at,
            );
            let expectation = acceptance
                .full_view_teleport
                .reconcile_presented_expectation(snapshot, proposed, observed_at);
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
            } else {
                presented_frames.clear();
            }
            presented_frames
                .drain()
                .into_iter()
                .find_map(|acknowledgement| {
                    acceptance
                        .full_view_teleport
                        .observe_presented_frame(acknowledgement)
                })
        } else {
            None
        };
        if let Some(teleport) = teleport {
            presented_frames.clear();
            let cohort = snapshot
                .cohort
                .expect("teleport completion requires an exact cohort");
            let Some(remesh_manifest) =
                stream.remesh_published_manifest(&teleport.expectation.manifest, Instant::now())
            else {
                error!(
                    "could not start exact full-view remesh: frozen published manifest is stale"
                );
                return;
            };
            if !acceptance.full_view_remesh.start(
                Some(&teleport),
                cohort,
                remesh_manifest,
                frame_count,
            ) {
                error!("could not start exact full-view remesh gate after binding teleport");
                return;
            }
            let proof = teleport_proof(cohort, &teleport);
            metrics.0.record_teleport_proof(proof.clone());
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{}", teleport_settled_marker(&proof));
            let _ = writeln!(
                stdout,
                "{}",
                teleport_global_stage_diagnostic_marker(cohort.target, &teleport)
            );
            let _ = stdout.flush();
            return;
        }

        if let Some(status) = snapshot.cohort
            && let Some(marker) = acceptance.full_view_teleport.cohort_progress_line(
                status,
                snapshot.work,
                timeout_progress,
                observed_at,
            )
        {
            let mut stdout = std::io::stdout().lock();
            let _ = writeln!(stdout, "{marker}");
            let _ = stdout.flush();
        }

        let remesh_manifest = acceptance
            .full_view_remesh
            .pending
            .as_ref()
            .map(|pending| pending.manifest.clone());
        if let Some(remesh_manifest) = remesh_manifest {
            let manifest_state = stream.forced_remesh_manifest_state(&remesh_manifest);
            let proposal_target = acceptance.full_view_remesh.pending.as_ref().map(|pending| {
                (
                    render_view_cohort(pending.cohort.target),
                    pending.source_cohort,
                )
            });
            let proposed = if manifest_state == client_world::ForcedRemeshManifestState::Complete {
                proposal_target.map(|(target, source)| {
                    render_queue.freeze_target_expectation(target, source, 0, observed_at)
                })
            } else {
                None
            };
            let expectation = acceptance.full_view_remesh.reconcile_presented_expectation(
                snapshot,
                manifest_state,
                proposed,
                observed_at,
                frame_count,
            );
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
            } else {
                presented_frames.clear();
            }
            let completion = presented_frames
                .drain()
                .into_iter()
                .find_map(|acknowledgement| {
                    acceptance
                        .full_view_remesh
                        .observe_presented_frame(acknowledgement, frame_count)
                });
            if let Some(completion) = completion {
                presented_frames.clear();
                let cohort = snapshot
                    .cohort
                    .expect("forced remesh completion requires the frozen exact cohort");
                let proof = forced_remesh_proof(cohort, &completion);
                metrics
                    .0
                    .record_forced_full_view_remesh_proof(proof.clone());
                let Some(target) = acceptance.full_view_teleport.completed_target_mutation else {
                    error!("forced remesh completed without deterministic mutation coordinates");
                    return;
                };
                if !acceptance.retarget_mutation(target, completion.stable_frame.gpu_completed_at) {
                    error!(
                        ?target,
                        "could not arm target-only mutation after forced remesh"
                    );
                    return;
                }
                let Some(mutation_marker) = acceptance.target_mutation_marker() else {
                    error!("target mutation armed without complete manifest-comparable evidence");
                    return;
                };
                let mut stdout = std::io::stdout().lock();
                let _ = writeln!(stdout, "{}", forced_remesh_settled_marker(&proof));
                let _ = writeln!(stdout, "{mutation_marker}");
                let _ = stdout.flush();
            }
        }

        let completed_remesh_target =
            acceptance
                .full_view_remesh
                .completed
                .as_ref()
                .map(|completion| {
                    (
                        completion.expectation.cohort,
                        completion.expectation.source_cohort,
                    )
                });
        if let Some((target, source)) = completed_remesh_target {
            let proposed = render_queue.freeze_target_expectation(target, source, 0, observed_at);
            let expectation =
                acceptance.reconcile_mutation_presented_expectation(proposed, observed_at);
            if let Some(expectation) = expectation {
                presented_frames.set_expectation(expectation);
                if let Some(latency) =
                    presented_frames
                        .drain()
                        .into_iter()
                        .find_map(|acknowledgement| {
                            acceptance.observe_presented_mutation(acknowledgement)
                        })
                {
                    presented_frames.clear();
                    metrics.0.record_mutation_to_visible(latency);
                }
            } else {
                presented_frames.clear();
            }
        }
        return;
    }
    let mutation_coordinate = acceptance.mutation_coordinate();
    let mutation_target = mutation_coordinate.map(|coordinate| {
        SubChunkKey::new(
            stream.current_dimension(),
            coordinate[0].div_euclid(16),
            coordinate[1].div_euclid(16),
            coordinate[2].div_euclid(16),
        )
    });
    let snapshot = WorldReadySnapshot {
        mutation_coordinate,
        received_radius_chunks: stats.received_radius_chunks,
        publisher_radius_chunks: stats.publisher_radius_chunks,
        rendered_sub_chunks: cache.rendered.len(),
        resident_sub_chunks: stats.resident_sub_chunks,
        visible_sub_chunks: cache.visible_rendered,
        mutation_target_rendered: mutation_target
            .is_some_and(|target| cache.rendered.contains(&target)),
        mutation_target_visible: mutation_target.is_some_and(|target| cache.is_visible(target)),
        mutation_target_clean: mutation_target.is_some_and(|target| stream.is_mesh_clean(target)),
        work,
    };
    let ready_at = Instant::now();
    if let Some(marker) = acceptance
        .gallery_anchor
        .observe(model_witness_source.configured(), snapshot)
    {
        let mut stdout = std::io::stdout().lock();
        write_stdout_marker(&mut stdout, &marker);
    }
    let Some(markers) = acceptance.world_ready_settler.observe(snapshot, ready_at) else {
        return;
    };
    metrics
        .0
        .record_asset_counters(missing_mapping_count, diagnostic_quads.0.total());
    let asset_marker = metrics
        .0
        .asset_metrics()
        .world_ready_marker(snapshot.resident_sub_chunks, snapshot.visible_sub_chunks);
    let coordinate = snapshot
        .mutation_coordinate
        .expect("world-ready markers require a mutation coordinate");
    auto_fly.set_look_target(Vec3::new(
        coordinate[0] as f32 + 0.5,
        coordinate[1] as f32 + 0.5,
        coordinate[2] as f32 + 0.5,
    ));
    let mut stdout = std::io::stdout().lock();
    for marker in markers {
        let _ = writeln!(stdout, "{marker}");
    }
    let _ = writeln!(stdout, "{asset_marker}");
    let _ = stdout.flush();
    stream.begin_timed_session();
    metrics.0.begin_timed_session(ready_at);
    acceptance.begin_world_ready(
        ready_at,
        stream.resolved_server_position().position,
        stream.local_player_runtime_id(),
    );
}
