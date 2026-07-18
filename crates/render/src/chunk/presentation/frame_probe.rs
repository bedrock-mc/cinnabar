use crate::chunk::*;

pub(in crate::chunk) fn build_presented_frame_ack(
    probe: CompletedFrameProbe,
    evidence: FrameCompletionEvidence,
) -> Option<PresentedFrameAck> {
    let present_returned_at = evidence.present_returned_at?;
    let gpu_completed_at = evidence.submitted_work_done_at?;
    if probe.expectation.render_ready_at > present_returned_at
        || present_returned_at > gpu_completed_at
    {
        return None;
    }
    let model_witness = probe.model_witness.map(|model| ModelWitnessFrameAck {
        revision: model.revision,
        request_hash: model.request_hash,
        frame_sequence: probe.frame_sequence,
        view_generation: probe.expectation.view_generation,
        present_returned_at,
        gpu_completed_at,
        total_model_ref_count: model.total_model_ref_count,
        manifest: model.manifest,
        missing_key_count: model.missing_key_count,
        stale_generation_count: model.stale_generation_count,
        wrong_stream_count: model.wrong_stream_count,
        zero_model_ref_count: model.zero_model_ref_count,
        draw_mismatch_count: model.draw_mismatch_count,
    });
    Some(PresentedFrameAck {
        cohort: probe.expectation.cohort,
        frame_sequence: probe.frame_sequence,
        allocation_manifest: probe.allocation_manifest,
        visible_allocation_manifest: probe.visible_allocation_manifest,
        drawn_manifest: probe.drawn_manifest,
        view_generation: probe.expectation.view_generation,
        render_ready_at: probe.expectation.render_ready_at,
        present_returned_at,
        gpu_completed_at,
        missing_target_instances: probe.missing_target_instances,
        unexpected_target_instances: probe.unexpected_target_instances,
        source_instances: probe.source_instances,
        foreign_instances: probe.foreign_instances,
        stale_generation_instances: probe.stale_generation_instances,
        orphan_allocations: probe.orphan_allocations,
        transparent_sort_generation: probe.transparent_sort_generation,
        model_witness,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct FrameInstanceIdentity {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct FrameAllocationIdentity {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) generation: u64,
}

#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct ChunkStreamMask(u8);

impl ChunkStreamMask {
    pub(in crate::chunk) const CUBE: Self = Self(1 << 0);
    pub(in crate::chunk) const MODEL: Self = Self(1 << 1);
    pub(in crate::chunk) const LIQUID: Self = Self(1 << 2);

    pub(in crate::chunk) const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

impl std::ops::BitOr for ChunkStreamMask {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

pub(in crate::chunk) trait IntoFrameAllocationEvidence {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize);
}

impl IntoFrameAllocationEvidence for FrameAllocationIdentity {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize) {
        (self, ChunkStreamMask::CUBE, 0)
    }
}

impl IntoFrameAllocationEvidence for (FrameAllocationIdentity, ChunkStreamMask) {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize) {
        (self.0, self.1, 0)
    }
}

impl IntoFrameAllocationEvidence for (FrameAllocationIdentity, ChunkStreamMask, usize) {
    fn into_evidence(self) -> (FrameAllocationIdentity, ChunkStreamMask, usize) {
        self
    }
}

pub(in crate::chunk) struct FrameProbe {
    pub(in crate::chunk) expectation: TargetRenderExpectation,
    pub(in crate::chunk) frame_sequence: u64,
    pub(in crate::chunk) eligible: HashMap<Entity, (FrameAllocationIdentity, ChunkStreamMask)>,
    pub(in crate::chunk) expected_streams: BTreeMap<(SubChunkKey, u64), ChunkStreamMask>,
    pub(in crate::chunk) model_request: ModelWitnessRequest,
    pub(in crate::chunk) model_allocations: BTreeMap<(SubChunkKey, u64), (ChunkStreamMask, usize)>,
    pub(in crate::chunk) allocation_manifest: BTreeSet<(SubChunkKey, u64)>,
    pub(in crate::chunk) visible_allocation_manifest: Mutex<BTreeSet<(SubChunkKey, u64)>>,
    pub(in crate::chunk) target_allocation_count: usize,
    pub(in crate::chunk) duplicate_target_instances: usize,
    pub(in crate::chunk) drawn: Mutex<BTreeMap<(SubChunkKey, u64), ChunkStreamMask>>,
    pub(in crate::chunk) drawn_transparent_generation: Mutex<Option<ViewSortGeneration>>,
    pub(in crate::chunk) source_instances: usize,
    pub(in crate::chunk) foreign_instances: usize,
    pub(in crate::chunk) stale_generation_instances: usize,
    pub(in crate::chunk) orphan_allocations: usize,
}

impl FrameProbe {
    #[cfg(test)]
    pub(in crate::chunk) fn begin(
        expectation: TargetRenderExpectation,
        instances: impl IntoIterator<Item = FrameInstanceIdentity>,
        allocations: impl IntoIterator<Item = impl IntoFrameAllocationEvidence>,
    ) -> Self {
        Self::begin_with_model_witness(
            expectation,
            instances,
            allocations,
            ModelWitnessRequest::default(),
        )
    }

    pub(in crate::chunk) fn begin_with_model_witness(
        expectation: TargetRenderExpectation,
        instances: impl IntoIterator<Item = FrameInstanceIdentity>,
        allocations: impl IntoIterator<Item = impl IntoFrameAllocationEvidence>,
        model_request: ModelWitnessRequest,
    ) -> Self {
        let model_target_keys = model_request.enabled().then(|| {
            model_request
                .keys()
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
        });
        let expectation_target_keys = expectation
            .target_keys
            .as_ref()
            .map(|keys| keys.iter().copied().collect::<BTreeSet<_>>());
        let target_columns = expectation
            .target_columns
            .as_ref()
            .map(|columns| columns.iter().copied().collect::<BTreeSet<_>>());
        debug_assert!(expectation_target_keys.is_none() || target_columns.is_none());
        let target_keys = if target_columns.is_some() {
            None
        } else {
            expectation_target_keys
                .as_ref()
                .or(model_target_keys.as_ref())
        };
        let is_scoped_instance =
            |key: SubChunkKey| target_keys.is_none_or(|target_keys| target_keys.contains(&key));
        let is_target = |key: SubChunkKey| {
            target_keys.map_or_else(
                || {
                    target_columns.as_ref().map_or_else(
                        || expectation.cohort.contains(key),
                        |target_columns| target_columns.contains(&key.chunk()),
                    )
                },
                |target_keys| target_keys.contains(&key),
            )
        };
        let is_model_target = |key: SubChunkKey| {
            model_target_keys
                .as_ref()
                .is_some_and(|target_keys| target_keys.contains(&key))
        };
        let expected = expectation
            .manifest
            .iter()
            .copied()
            .collect::<BTreeMap<_, _>>();
        let instances = instances
            .into_iter()
            .map(|instance| (instance.entity, instance))
            .collect::<HashMap<_, _>>();
        let source_instances = instances
            .values()
            .filter(|instance| {
                is_scoped_instance(instance.key)
                    && expectation
                        .source_cohort
                        .is_some_and(|source| source.contains(instance.key))
            })
            .count();
        let foreign_instances = instances
            .values()
            .filter(|instance| {
                is_scoped_instance(instance.key)
                    && !expectation.cohort.contains(instance.key)
                    && expectation
                        .source_cohort
                        .is_none_or(|source| !source.contains(instance.key))
            })
            .count();
        let target_instance_count = instances
            .values()
            .filter(|instance| is_target(instance.key))
            .count();
        let unique_target_instance_keys = instances
            .values()
            .filter(|instance| is_target(instance.key))
            .map(|instance| instance.key)
            .collect::<BTreeSet<_>>()
            .len();
        let duplicate_target_instances =
            target_instance_count.saturating_sub(unique_target_instance_keys);
        let mut stale_entities = instances
            .values()
            .filter_map(|instance| {
                expected
                    .get(&instance.key)
                    .is_some_and(|generation| *generation != instance.generation)
                    .then_some(instance.entity)
            })
            .collect::<BTreeSet<_>>();
        let mut eligible = HashMap::new();
        let mut expected_streams_by_identity = BTreeMap::new();
        let mut model_allocations = BTreeMap::new();
        let mut allocation_manifest = BTreeSet::new();
        let mut target_allocation_count = 0;
        let mut orphan_allocations = 0;
        for allocation in allocations {
            let (allocation, expected_streams, model_ref_count) = allocation.into_evidence();
            let Some(instance) = instances.get(&allocation.entity) else {
                if is_scoped_instance(allocation.key) {
                    orphan_allocations += 1;
                }
                continue;
            };
            if instance.key != allocation.key || instance.generation != allocation.generation {
                if target_keys.is_none() || is_target(instance.key) || is_target(allocation.key) {
                    stale_entities.insert(allocation.entity);
                }
                continue;
            }
            if is_target(allocation.key) {
                target_allocation_count += 1;
                allocation_manifest.insert((allocation.key, allocation.generation));
            }
            let identity = (allocation.key, allocation.generation);
            let mask = expected_streams_by_identity.entry(identity).or_default();
            *mask = *mask | expected_streams;
            if is_model_target(allocation.key) {
                let model = model_allocations
                    .entry(identity)
                    .or_insert((ChunkStreamMask::default(), 0));
                model.0 = model.0 | expected_streams;
                model.1 = model.1.max(model_ref_count);
            }
            eligible.insert(allocation.entity, (allocation, expected_streams));
        }
        Self {
            expectation,
            frame_sequence: 0,
            eligible,
            expected_streams: expected_streams_by_identity,
            model_request,
            model_allocations,
            allocation_manifest,
            visible_allocation_manifest: Mutex::new(BTreeSet::new()),
            target_allocation_count,
            duplicate_target_instances,
            drawn: Mutex::new(BTreeMap::new()),
            drawn_transparent_generation: Mutex::new(None),
            source_instances,
            foreign_instances,
            stale_generation_instances: stale_entities.len(),
            orphan_allocations,
        }
    }

    pub(in crate::chunk) fn record_direct_draw(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
    ) -> bool {
        self.record_direct_streams(entity, allocation, ChunkStreamMask::CUBE)
    }

    pub(in crate::chunk) fn record_visible(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
    ) -> bool {
        let Some(&(eligible, _)) = self.eligible.get(&entity) else {
            return false;
        };
        if eligible != allocation {
            return false;
        }
        let identity = (allocation.key, allocation.generation);
        if self.allocation_manifest.contains(&identity) {
            self.visible_allocation_manifest
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(identity);
        }
        true
    }

    pub(in crate::chunk) fn record_direct_streams(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
        streams: ChunkStreamMask,
    ) -> bool {
        let Some(&(eligible, _expected_streams)) = self.eligible.get(&entity) else {
            return false;
        };
        if eligible != allocation {
            return false;
        }
        let identity = (allocation.key, allocation.generation);
        if self.allocation_manifest.contains(&identity) {
            let mut drawn = self
                .drawn
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            let mask = drawn.entry(identity).or_default();
            *mask = *mask | streams;
        }
        true
    }

    pub(in crate::chunk) fn record_mdi_draws(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        draws
            .into_iter()
            .filter(|(entity, allocation)| self.record_direct_draw(*entity, *allocation))
            .count()
    }

    pub(in crate::chunk) fn record_mdi_streams(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
        streams: ChunkStreamMask,
    ) -> usize {
        draws
            .into_iter()
            .filter(|(entity, allocation)| {
                self.record_direct_streams(*entity, *allocation, streams)
            })
            .count()
    }

    pub(in crate::chunk) fn record_transparent_draw(
        &self,
        generation: ViewSortGeneration,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        let draws = draws.into_iter().collect::<Vec<_>>();
        let encoded = !draws.is_empty();
        let count = draws
            .into_iter()
            .filter(|(entity, allocation)| {
                self.record_direct_streams(*entity, *allocation, ChunkStreamMask::LIQUID)
            })
            .count();
        if encoded {
            *self
                .drawn_transparent_generation
                .lock()
                .unwrap_or_else(|poison| poison.into_inner()) = Some(generation);
        }
        count
    }

    pub(in crate::chunk) fn complete(self) -> CompletedFrameProbe {
        let transparent_sort_generation = self
            .drawn_transparent_generation
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner())
            .map_or(0, ViewSortGeneration::get);
        let expected = self
            .expectation
            .manifest
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let visible_allocation_manifest = self
            .visible_allocation_manifest
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        let drawn_streams = self
            .drawn
            .into_inner()
            .unwrap_or_else(|poison| poison.into_inner());
        let model_witness = self.model_request.enabled().then(|| {
            let expected = self.expectation.manifest.as_ref();
            let allocations = self
                .model_allocations
                .iter()
                .map(|(&(key, generation), &(streams, model_ref_count))| {
                    (key, generation, streams, model_ref_count)
                })
                .collect::<Vec<_>>();
            let drawn = drawn_streams
                .iter()
                .map(|(&(key, generation), &streams)| (key, generation, streams))
                .collect::<Vec<_>>();
            evaluate_model_witness_frame(
                &self.model_request,
                self.frame_sequence,
                self.expectation.view_generation,
                expected,
                &allocations,
                &drawn,
            )
        });
        let drawn = drawn_streams
            .into_iter()
            .filter_map(|(identity, drawn)| {
                let expected = self.expected_streams.get(&identity).copied()?;
                drawn.contains(expected).then_some(identity)
            })
            .collect::<BTreeSet<_>>();
        let matched_target_instances = expected.intersection(&self.allocation_manifest).count();
        let missing_target_instances = expected.len().saturating_sub(matched_target_instances);
        let unexpected_target_instances = self
            .target_allocation_count
            .saturating_sub(matched_target_instances)
            .max(self.duplicate_target_instances);
        CompletedFrameProbe {
            expectation: self.expectation,
            frame_sequence: self.frame_sequence,
            allocation_manifest: Arc::from(
                self.allocation_manifest.into_iter().collect::<Vec<_>>(),
            ),
            visible_allocation_manifest: Arc::from(
                visible_allocation_manifest.into_iter().collect::<Vec<_>>(),
            ),
            drawn_manifest: Arc::from(drawn.into_iter().collect::<Vec<_>>()),
            missing_target_instances,
            unexpected_target_instances,
            source_instances: self.source_instances,
            foreign_instances: self.foreign_instances,
            stale_generation_instances: self.stale_generation_instances,
            orphan_allocations: self.orphan_allocations,
            transparent_sort_generation,
            model_witness,
        }
    }
}

#[derive(Default)]
pub(in crate::chunk) struct ActiveFrameProbeState {
    pub(in crate::chunk) current: Option<FrameProbe>,
    pub(in crate::chunk) next_frame_sequence: u64,
}

#[derive(Resource, Default)]
pub(in crate::chunk) struct ActiveFrameProbe(Mutex<ActiveFrameProbeState>);

impl ActiveFrameProbe {
    pub(in crate::chunk) fn is_active(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .is_some()
    }

    pub(in crate::chunk) fn begin(&self, mut probe: FrameProbe) {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        state.next_frame_sequence = state.next_frame_sequence.wrapping_add(1).max(1);
        probe.frame_sequence = state.next_frame_sequence;
        state.current = Some(probe);
    }

    pub(in crate::chunk) fn clear(&self) {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current = None;
    }

    pub(in crate::chunk) fn accepts(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
    ) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| {
                probe
                    .eligible
                    .get(&entity)
                    .is_some_and(|(eligible, _)| *eligible == allocation)
            })
    }

    pub(in crate::chunk) fn record_visible(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
    ) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_visible(entity, allocation))
    }

    pub(in crate::chunk) fn record_direct_draw(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
    ) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_direct_draw(entity, allocation))
    }

    pub(in crate::chunk) fn record_direct_streams(
        &self,
        entity: Entity,
        allocation: FrameAllocationIdentity,
        streams: ChunkStreamMask,
    ) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .as_ref()
            .is_none_or(|probe| probe.record_direct_streams(entity, allocation, streams))
    }

    pub(in crate::chunk) fn record_mdi_draws(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let draws = draws.into_iter().collect::<Vec<_>>();
        state.current.as_ref().map_or(draws.len(), |probe| {
            probe.record_mdi_draws(draws.iter().copied())
        })
    }

    pub(in crate::chunk) fn record_mdi_streams(
        &self,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
        streams: ChunkStreamMask,
    ) -> usize {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let draws = draws.into_iter().collect::<Vec<_>>();
        state.current.as_ref().map_or(draws.len(), |probe| {
            probe.record_mdi_streams(draws.iter().copied(), streams)
        })
    }

    pub(in crate::chunk) fn record_transparent_draw(
        &self,
        generation: ViewSortGeneration,
        draws: impl IntoIterator<Item = (Entity, FrameAllocationIdentity)>,
    ) -> usize {
        let state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        let draws = draws.into_iter().collect::<Vec<_>>();
        state.current.as_ref().map_or(draws.len(), |probe| {
            probe.record_transparent_draw(generation, draws.iter().copied())
        })
    }

    pub(in crate::chunk) fn take_completed(&self) -> Option<CompletedFrameProbe> {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .current
            .take()
            .map(FrameProbe::complete)
    }
}

#[allow(clippy::too_many_arguments)]
pub(in crate::chunk) fn submit_presented_frame_probe(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    frame_probe: Res<ActiveFrameProbe>,
    presented_frame_gate: Res<PresentedFrameGate>,
    transparent_metrics: Res<TransparentSortMetrics>,
    transparent_fence: Res<TransparentPresentationFence>,
    transparent_runtime: Res<TransparentSortRuntime>,
    mut arena: ResMut<ChunkGpuArena>,
    retirement_fence: Res<TransparentRetirementFence>,
    witness_request: Res<TransparentWitnessRequest>,
    witness_evidence: Res<TransparentWitnessEvidence>,
    visibility_probe: Res<ActiveVisibilityFrameProbe>,
    visibility_diagnostics: Res<VisibilityDiagnostics>,
) {
    let visibility_snapshot = visibility_probe.take_completed();
    let completed_probe = frame_probe.take_completed().and_then(|probe| {
        presented_frame_gate
            .try_reserve_callback(&probe.expectation)
            .then_some(probe)
    });
    let transparent_snapshot = transparent_metrics.snapshot();
    let transparent_generation = (transparent_snapshot.encoded_generation != 0
        && transparent_snapshot.encoded_generation == transparent_snapshot.committed_generation
        && transparent_snapshot.encoded_generation != transparent_snapshot.presented_generation
        && transparent_fence.try_reserve(transparent_snapshot.encoded_generation))
    .then_some(transparent_snapshot.encoded_generation);
    let has_releasable_retirement = arena.retired_allocations.iter().any(|retirement| {
        retirement.release_epoch.is_none()
            && transparent_retirement_can_arm(
                transparent_runtime.state.committed(),
                &retirement.identity,
            )
    });
    let retirement_epoch = has_releasable_retirement
        .then(|| retirement_fence.try_reserve())
        .flatten();
    if let Some(epoch) = retirement_epoch {
        for retirement in &mut arena.retired_allocations {
            if retirement.release_epoch.is_none()
                && transparent_retirement_can_arm(
                    transparent_runtime.state.committed(),
                    &retirement.identity,
                )
            {
                retirement.release_epoch = Some(epoch);
            }
        }
    }
    let witness_generation = transparent_runtime
        .state
        .committed()
        .map_or(0, |snapshot| snapshot.generation().get());
    let witness_missing = transparent_runtime.state.committed().map_or_else(
        || witness_request.keys().to_vec(),
        |snapshot| transparent_view_missing_witness_keys(snapshot.key(), &witness_request),
    );
    let witness_token =
        witness_evidence.try_reserve_missing(&witness_request, witness_generation, witness_missing);
    if completed_probe.is_none()
        && transparent_generation.is_none()
        && retirement_epoch.is_none()
        && witness_token.is_none()
        && visibility_snapshot.is_none()
    {
        if let Err(error) = render_device.poll(PollType::Poll) {
            bevy::log::warn!(
                ?error,
                "could not nonblockingly poll presented-frame fences"
            );
        }
        return;
    }
    let present_returned_at = Instant::now();
    let encoder = render_device.create_command_encoder(&CommandEncoderDescriptor {
        label: Some("presented frame completion sentinel"),
    });
    let command_buffer = encoder.finish();
    let callback_gate = presented_frame_gate.clone();
    let callback_metrics = transparent_metrics.clone();
    let callback_transparent_fence = transparent_fence.clone();
    let callback_retirement_fence = retirement_fence.clone();
    let callback_witness_evidence = witness_evidence.clone();
    let callback_visibility_diagnostics = visibility_diagnostics.clone();
    command_buffer.on_submitted_work_done(move || {
        if let Some(snapshot) = visibility_snapshot {
            callback_visibility_diagnostics.publish(snapshot.gpu_completed());
        }
        if let Some(completed_probe) = completed_probe {
            callback_gate.publish_reserved_probe(
                completed_probe,
                present_returned_at,
                Instant::now(),
            );
        }
        if let Some(generation) = transparent_generation
            && callback_transparent_fence.complete(generation)
        {
            record_gpu_completed_transparent_generation(&callback_metrics, generation);
        }
        if let Some(epoch) = retirement_epoch {
            callback_retirement_fence.complete(epoch);
        }
        if let Some(token) = witness_token {
            callback_witness_evidence.complete(token);
        }
    });
    render_queue.submit([command_buffer]);
    if let Err(error) = render_device.poll(PollType::Poll) {
        bevy::log::warn!(
            ?error,
            "could not nonblockingly poll presented-frame fences"
        );
    }
}
