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

pub(in crate::chunk) fn transparent_snapshot_references_allocation(
    snapshot: &TransparentOrderedSnapshot,
    allocation: &GpuChunkAllocation,
) -> bool {
    snapshot.key.visible_allocations.iter().any(|visible| {
        visible.key == allocation.key
            && visible.mesh_generation == allocation.generation
            && visible.metadata_index == allocation.metadata_index
            && allocation.liquid_range.as_ref() == Some(&visible.liquid_range)
            && allocation.liquid_lighting_range.as_ref() == Some(&visible.lighting_range)
    })
}

#[cfg(test)]
pub(in crate::chunk) fn transparent_view_key_satisfies_witness(
    key: &ViewSortKey,
    request: &TransparentWitnessRequest,
) -> bool {
    request.enabled()
        && request.keys.iter().all(|required| {
            key.visible_allocations
                .iter()
                .any(|allocation| allocation.key == *required)
        })
}

pub(in crate::chunk) fn transparent_view_missing_witness_keys(
    key: &ViewSortKey,
    request: &TransparentWitnessRequest,
) -> Vec<SubChunkKey> {
    request
        .keys
        .iter()
        .copied()
        .filter(|required| {
            !key.visible_allocations
                .iter()
                .any(|allocation| allocation.key == *required)
        })
        .collect()
}

pub(in crate::chunk) fn transparent_retirement_can_arm(
    committed: Option<&TransparentOrderedSnapshot>,
    retired: &GpuChunkAllocation,
) -> bool {
    committed.is_none_or(|snapshot| !transparent_snapshot_references_allocation(snapshot, retired))
}
