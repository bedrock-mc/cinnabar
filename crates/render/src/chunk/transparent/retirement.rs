use crate::chunk::*;

pub(in crate::chunk) fn record_encoded_transparent_generation(
    metrics: &TransparentSortMetrics,
    generation: ViewSortGeneration,
) {
    metrics.update(|snapshot| snapshot.encoded_generation = generation.get());
}

pub(in crate::chunk) fn record_gpu_completed_transparent_generation(
    metrics: &TransparentSortMetrics,
    generation: u64,
) {
    metrics.update(|snapshot| {
        if generation != 0
            && snapshot.committed_generation == generation
            && snapshot.encoded_generation == generation
        {
            snapshot.presented_generation = snapshot.presented_generation.max(generation);
        }
    });
}

#[derive(Resource, Debug, Clone, Default)]
pub(in crate::chunk) struct TransparentPresentationFence(Arc<Mutex<Option<u64>>>);

impl TransparentPresentationFence {
    #[cfg(feature = "publication-test-support")]
    pub(in crate::chunk) fn is_in_flight(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .is_some()
    }

    pub(in crate::chunk) fn try_reserve(&self, generation: u64) -> bool {
        let mut in_flight = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if generation == 0 || in_flight.is_some() {
            return false;
        }
        *in_flight = Some(generation);
        true
    }

    pub(in crate::chunk) fn complete(&self, generation: u64) -> bool {
        let mut in_flight = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if *in_flight != Some(generation) {
            return false;
        }
        *in_flight = None;
        true
    }
}

#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentRetirementFenceState {
    pub(in crate::chunk) next_epoch: u64,
    pub(in crate::chunk) in_flight: Option<u64>,
    pub(in crate::chunk) completed_epoch: u64,
}

/// Independent queue-completion epoch for reclaiming retired arena addresses.
/// It deliberately does not use `ViewSortGeneration`: view resets and stale
/// sort callbacks must not make physical GPU memory reusable early.
#[derive(Resource, Debug, Clone, Default)]
pub(in crate::chunk) struct TransparentRetirementFence(Arc<Mutex<TransparentRetirementFenceState>>);

impl TransparentRetirementFence {
    #[cfg(feature = "publication-test-support")]
    pub(in crate::chunk) fn is_in_flight(&self) -> bool {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .in_flight
            .is_some()
    }

    pub(in crate::chunk) fn try_reserve(&self) -> Option<u64> {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.in_flight.is_some() {
            return None;
        }
        state.next_epoch = state.next_epoch.checked_add(1)?;
        let epoch = state.next_epoch;
        state.in_flight = Some(epoch);
        Some(epoch)
    }

    pub(in crate::chunk) fn complete(&self, epoch: u64) -> bool {
        let mut state = self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if state.in_flight != Some(epoch) {
            return false;
        }
        state.in_flight = None;
        state.completed_epoch = state.completed_epoch.max(epoch);
        true
    }

    pub(in crate::chunk) fn completed_epoch(&self) -> u64 {
        self.0
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .completed_epoch
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentRetirementBudget {
    pub(in crate::chunk) max_items: usize,
    pub(in crate::chunk) max_bytes: u64,
    pub(in crate::chunk) items: usize,
    pub(in crate::chunk) bytes: u64,
}

impl TransparentRetirementBudget {
    pub(in crate::chunk) const fn with_limits(max_items: usize, max_bytes: u64) -> Self {
        Self {
            max_items,
            max_bytes,
            items: 0,
            bytes: 0,
        }
    }

    pub(in crate::chunk) fn try_reserve(&mut self, items: usize, bytes: u64) -> bool {
        let Some(next_items) = self.items.checked_add(items) else {
            return false;
        };
        let Some(next_bytes) = self.bytes.checked_add(bytes) else {
            return false;
        };
        if next_items > self.max_items || next_bytes > self.max_bytes {
            return false;
        }
        self.items = next_items;
        self.bytes = next_bytes;
        true
    }

    pub(in crate::chunk) fn can_reserve(self, items: usize, bytes: u64) -> bool {
        let mut next = self;
        next.try_reserve(items, bytes)
    }

    pub(in crate::chunk) fn release(&mut self, items: usize, bytes: u64) {
        self.items = self.items.saturating_sub(items);
        self.bytes = self.bytes.saturating_sub(bytes);
    }

    #[cfg(test)]
    pub(in crate::chunk) const fn items(self) -> usize {
        self.items
    }

    #[cfg(test)]
    pub(in crate::chunk) const fn bytes(self) -> u64 {
        self.bytes
    }
}

/// Matches retained references against the still-resident physical liquid stream.
pub(in crate::chunk) fn transparent_resident_allocation_contains(
    identity: &TransparentAllocationIdentity,
    allocation: &GpuChunkAllocation,
) -> bool {
    let (Some(liquid), Some(lighting)) = (
        allocation.liquid_range.as_ref(),
        allocation.liquid_lighting_range.as_ref(),
    ) else {
        return false;
    };
    !liquid.is_empty()
        && !lighting.is_empty()
        && liquid.start.is_multiple_of(4)
        && liquid.end.is_multiple_of(4)
        && lighting.start.is_multiple_of(2)
        && lighting.end.is_multiple_of(2)
        && liquid.end.saturating_sub(liquid.start) / 4
            == lighting.end.saturating_sub(lighting.start) / 2
        && allocation.key == identity.key
        && allocation.metadata_index == identity.metadata_index
        && liquid.start == identity.liquid_range.start
        && liquid.end >= identity.liquid_range.end
}

/// Resolves an active allocation with the same physical contract as snapshot retention.
pub(in crate::chunk) fn transparent_snapshot_references_resident_allocation(
    snapshot: &TransparentOrderedSnapshot,
    allocation: &GpuChunkAllocation,
) -> bool {
    allocation.tint_identity == snapshot.key.tint_identity
        && snapshot
            .key
            .allocation(allocation.key)
            .is_some_and(|identity| transparent_resident_allocation_contains(identity, allocation))
}

pub(in crate::chunk) fn transparent_snapshot_references_allocation(
    snapshot: &TransparentOrderedSnapshot,
    allocation: &GpuChunkAllocation,
) -> bool {
    snapshot.key.references_exact(allocation)
}

/// Matches the exact mesh generation and liquid addresses a snapshot reference was sorted against.
pub(in crate::chunk) fn transparent_allocation_is_exact(
    identity: &TransparentAllocationIdentity,
    allocation: &GpuChunkAllocation,
) -> bool {
    identity.key == allocation.key
        && identity.mesh_generation == allocation.generation
        && identity.metadata_index == allocation.metadata_index
        && allocation.liquid_range.as_ref() == Some(&identity.liquid_range)
        && allocation.liquid_lighting_range.as_ref() == Some(&identity.lighting_range)
}

#[cfg(test)]
pub(in crate::chunk) fn transparent_view_key_satisfies_witness(
    key: &ViewSortKey,
    request: &TransparentWitnessRequest,
) -> bool {
    request.enabled()
        && request
            .keys
            .iter()
            .all(|required| key.allocation(*required).is_some())
}

/// Witness keys whose water is neither in `committed`'s sort nor drawn directly in any order.
pub(in crate::chunk) fn transparent_view_missing_witness_keys(
    committed: Option<&ViewSortKey>,
    request: &TransparentWitnessRequest,
    drawn_directly: impl Fn(SubChunkKey) -> bool,
) -> Vec<SubChunkKey> {
    request
        .keys
        .iter()
        .copied()
        .filter(|&required| {
            committed.is_none_or(|key| key.allocation(required).is_none())
                && !drawn_directly(required)
        })
        .collect()
}

/// Arms every retired allocation that no committed or staged snapshot reads with one new
/// fence epoch and returns it; `None` when nothing is releasable or an epoch is in flight.
///
/// The allocations are released once the GPU completes the frame submitted with the epoch.
pub(in crate::chunk) fn arm_transparent_retirements(
    arena: &mut ChunkGpuArena,
    state: &TransparentSortState,
    fence: &TransparentRetirementFence,
) -> Option<u64> {
    let releasable = |retirement: &RetiredArenaAllocation| {
        retirement.release_epoch.is_none()
            && transparent_retirement_can_arm(state.retained_keys(), &retirement.identity)
    };
    if !arena.retired_allocations.iter().any(releasable) {
        return None;
    }
    let epoch = fence.try_reserve()?;
    for retirement in &mut arena.retired_allocations {
        if releasable(retirement) {
            retirement.release_epoch = Some(epoch);
        }
    }
    Some(epoch)
}

/// Whether no snapshot a frame may still draw, committed or staged, reads `retired`.
pub(in crate::chunk) fn transparent_retirement_can_arm<'a>(
    retained: impl IntoIterator<Item = &'a ViewSortKey>,
    retired: &GpuChunkAllocation,
) -> bool {
    retained
        .into_iter()
        .all(|key| !key.references_exact(retired))
}
