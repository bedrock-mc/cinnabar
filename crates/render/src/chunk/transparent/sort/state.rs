use super::groups::{TransparentGroupInput, TransparentGroupOrder, TransparentGroups};
use super::manifest::TransparentManifest;
use super::{
    MAX_TRANSPARENT_DRAW_REFS, PackedTransparentDrawRef, TransparentLiquidPhaseGroup,
    transparent_liquid_phase_groups,
};
use crate::chunk::transparent::face_metric::{FaceOrderCamera, TransparentFaceMetric};
use crate::chunk::*;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransparentSortError {
    ReferenceCeiling { requested: usize, ceiling: usize },
    ConflictingAllocation { key: SubChunkKey },
    InvalidCameraTransform,
}
pub const fn validate_transparent_sort_ref_count(
    requested: usize,
) -> Result<(), TransparentSortError> {
    if requested > MAX_TRANSPARENT_DRAW_REFS {
        Err(TransparentSortError::ReferenceCeiling {
            requested,
            ceiling: MAX_TRANSPARENT_DRAW_REFS,
        })
    } else {
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ViewSortGeneration(pub(in crate::chunk) u64);

impl ViewSortGeneration {
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[cfg(test)]
    #[must_use]
    pub(in crate::chunk) const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_gate_keeps_one_in_flight_and_only_the_newest_replacement() {
        let mut gate = TransparentSortJobGate::default();
        let first = ViewSortGeneration::new(1);
        let second = ViewSortGeneration::new(2);
        let newest = ViewSortGeneration::new(3);
        assert_eq!(gate.submit(first, "first"), Some((first, "first")));
        assert_eq!(gate.submit(second, "second"), None);
        assert_eq!(gate.submit(newest, "newest"), None);
        assert_eq!(gate.in_flight_generation(), Some(first));
        assert_eq!(gate.pending_generation(), Some(newest));
        assert_eq!(gate.complete(first), Some((newest, "newest")));
        assert_eq!(gate.in_flight_generation(), Some(newest));
        assert_eq!(gate.pending_generation(), None);
        assert_eq!(gate.complete(newest), None);
        assert_eq!(gate.in_flight_generation(), None);
    }
}

#[derive(Debug)]
pub struct TransparentSortJobGate<T> {
    pub(in crate::chunk) in_flight: Option<ViewSortGeneration>,
    pub(in crate::chunk) pending: Option<(ViewSortGeneration, T)>,
}

impl<T> Default for TransparentSortJobGate<T> {
    fn default() -> Self {
        Self {
            in_flight: None,
            pending: None,
        }
    }
}

impl<T> TransparentSortJobGate<T> {
    pub fn submit(
        &mut self,
        generation: ViewSortGeneration,
        payload: T,
    ) -> Option<(ViewSortGeneration, T)> {
        if self.in_flight.is_none() {
            self.in_flight = Some(generation);
            Some((generation, payload))
        } else {
            self.pending = Some((generation, payload));
            None
        }
    }

    pub(in crate::chunk) fn submit_with_replacement(
        &mut self,
        generation: ViewSortGeneration,
        payload: T,
    ) -> (Option<(ViewSortGeneration, T)>, Option<ViewSortGeneration>) {
        if self.in_flight.is_none() {
            self.in_flight = Some(generation);
            (Some((generation, payload)), None)
        } else {
            let replaced = self
                .pending
                .replace((generation, payload))
                .map(|(generation, _)| generation);
            (None, replaced)
        }
    }

    pub fn complete(&mut self, generation: ViewSortGeneration) -> Option<(ViewSortGeneration, T)> {
        if self.in_flight != Some(generation) {
            return None;
        }
        self.in_flight = None;
        if let Some((next_generation, payload)) = self.pending.take() {
            self.in_flight = Some(next_generation);
            Some((next_generation, payload))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn in_flight_generation(&self) -> Option<ViewSortGeneration> {
        self.in_flight
    }

    #[must_use]
    pub fn pending_generation(&self) -> Option<ViewSortGeneration> {
        self.pending.as_ref().map(|(generation, _)| *generation)
    }

    pub(in crate::chunk) fn contains_generation(&self, generation: ViewSortGeneration) -> bool {
        self.in_flight == Some(generation) || self.pending_generation() == Some(generation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TransparentAllocationIdentity {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) mesh_generation: u64,
    pub(in crate::chunk) liquid_range: Range<u32>,
    pub(in crate::chunk) lighting_range: Range<u32>,
    pub(in crate::chunk) metadata_index: u32,
}

impl TransparentAllocationIdentity {
    #[must_use]
    pub const fn new(
        key: SubChunkKey,
        mesh_generation: u64,
        liquid_range: Range<u32>,
        lighting_range: Range<u32>,
        metadata_index: u32,
    ) -> Self {
        Self {
            key,
            mesh_generation,
            liquid_range,
            lighting_range,
            metadata_index,
        }
    }

    #[must_use]
    pub const fn key(&self) -> SubChunkKey {
        self.key
    }

    pub(in crate::chunk) fn canonical_tuple(&self) -> (SubChunkKey, u64, u32, u32, u32, u32, u32) {
        (
            self.key,
            self.mesh_generation,
            self.liquid_range.start,
            self.liquid_range.end,
            self.lighting_range.start,
            self.lighting_range.end,
            self.metadata_index,
        )
    }
}

/// Omits camera rotation: each sub-chunk is its own phase item and its faces sort by position.
///
/// The allocations are every resident group that needs sorting, not the frustum's, so a
/// turn never changes the key and newly visible water is already in order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSortKey {
    pub(in crate::chunk) order_camera: FaceOrderCamera,
    /// Sorted by key, each key at most once.
    pub(in crate::chunk) sorted_allocations: Arc<[TransparentAllocationIdentity]>,
    pub(in crate::chunk) asset_identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct TransparentAddressIdentity {
    pub(in crate::chunk) sorted_allocations: Arc<[TransparentAllocationIdentity]>,
    pub(in crate::chunk) asset_identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
}

impl ViewSortKey {
    /// Keys the camera only as far as some sorted sub-chunk's face order depends on it.
    pub fn try_new(
        camera_position: [f32; 3],
        mut sorted_allocations: Vec<TransparentAllocationIdentity>,
        asset_identity: ChunkTextureAssetIdentity,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> Result<Self, TransparentSortError> {
        if !camera_position.into_iter().all(f32::is_finite) {
            return Err(TransparentSortError::InvalidCameraTransform);
        }
        sorted_allocations.sort_by_key(TransparentAllocationIdentity::canonical_tuple);
        sorted_allocations.dedup();
        for pair in sorted_allocations.windows(2) {
            if pair[0].key == pair[1].key {
                return Err(TransparentSortError::ConflictingAllocation { key: pair[0].key });
            }
        }
        let order_camera = TransparentFaceMetric::new(Vec3::from_array(camera_position))
            .order_camera(sorted_allocations.iter().map(|identity| identity.key));
        Ok(Self {
            order_camera,
            sorted_allocations: Arc::from(sorted_allocations),
            asset_identity,
            tint_identity,
        })
    }

    /// Keys a manifest that is already canonical; `any_near` says whether some allocation
    /// lies in the camera's near box.
    pub(in crate::chunk) fn from_canonical(
        camera_position: Vec3,
        sorted_allocations: Arc<[TransparentAllocationIdentity]>,
        any_near: bool,
        asset_identity: ChunkTextureAssetIdentity,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> Result<Self, TransparentSortError> {
        if !camera_position.is_finite() {
            return Err(TransparentSortError::InvalidCameraTransform);
        }
        debug_assert!(
            sorted_allocations
                .windows(2)
                .all(|pair| pair[0].key < pair[1].key)
        );
        Ok(Self {
            order_camera: TransparentFaceMetric::new(camera_position)
                .order_camera_with_near(any_near),
            sorted_allocations,
            asset_identity,
            tint_identity,
        })
    }

    /// The allocation this key sorts for `key`, if any.
    pub(in crate::chunk) fn allocation(
        &self,
        key: SubChunkKey,
    ) -> Option<&TransparentAllocationIdentity> {
        self.sorted_allocations
            .binary_search_by(|identity| identity.key.cmp(&key))
            .ok()
            .map(|index| &self.sorted_allocations[index])
    }

    /// Whether this key's refs point into exactly `allocation`'s liquid records.
    pub(in crate::chunk) fn references_exact(&self, allocation: &GpuChunkAllocation) -> bool {
        self.allocation(allocation.key)
            .is_some_and(|identity| transparent_allocation_is_exact(identity, allocation))
    }

    pub(in crate::chunk) fn address_identity_eq(&self, other: &Self) -> bool {
        self.sorted_allocations == other.sorted_allocations
            && self.asset_identity == other.asset_identity
            && self.tint_identity == other.tint_identity
    }

    pub(in crate::chunk) fn address_identity(&self) -> TransparentAddressIdentity {
        TransparentAddressIdentity {
            sorted_allocations: Arc::clone(&self.sorted_allocations),
            asset_identity: self.asset_identity,
            tint_identity: self.tint_identity,
        }
    }
}

/// Spans of `refs` that differ from a base order, computed off the render thread.
pub(in crate::chunk) type TransparentRefPatch =
    (Arc<[PackedTransparentDrawRef]>, Vec<Range<usize>>);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentSortResult {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
    pub(in crate::chunk) patch: Option<TransparentRefPatch>,
}

impl TransparentSortResult {
    pub fn new(
        generation: ViewSortGeneration,
        key: ViewSortKey,
        refs: Vec<PackedTransparentDrawRef>,
    ) -> Result<Self, TransparentSortError> {
        Self::with_patch(generation, key, refs.into(), None)
    }

    pub(in crate::chunk) fn with_patch(
        generation: ViewSortGeneration,
        key: ViewSortKey,
        refs: Arc<[PackedTransparentDrawRef]>,
        patch: Option<TransparentRefPatch>,
    ) -> Result<Self, TransparentSortError> {
        validate_transparent_sort_ref_count(refs.len())?;
        Ok(Self {
            generation,
            key,
            refs,
            patch,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentOrderedSnapshot {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
    pub(in crate::chunk) buffer_slot: u8,
    pub(in crate::chunk) phase_groups: PhaseGroupCache,
}

/// Per-snapshot memo of its validated sub-chunk partition; never part of snapshot equality.
#[derive(Debug, Clone, Default)]
pub(in crate::chunk) struct PhaseGroupCache(OnceLock<Option<Arc<[TransparentLiquidPhaseGroup]>>>);

impl PartialEq for PhaseGroupCache {
    fn eq(&self, _other: &Self) -> bool {
        true
    }
}

impl Eq for PhaseGroupCache {}

impl TransparentOrderedSnapshot {
    /// Validates the sub-chunk partition once per committed order instead of every frame.
    pub(in crate::chunk) fn phase_groups(&self) -> Option<Arc<[TransparentLiquidPhaseGroup]>> {
        self.phase_groups
            .0
            .get_or_init(|| transparent_liquid_phase_groups(self).map(Arc::from))
            .clone()
    }

    #[must_use]
    pub const fn generation(&self) -> ViewSortGeneration {
        self.generation
    }

    #[must_use]
    pub const fn key(&self) -> &ViewSortKey {
        &self.key
    }

    #[must_use]
    pub fn refs(&self) -> &[PackedTransparentDrawRef] {
        &self.refs
    }

    #[must_use]
    pub const fn buffer_slot(&self) -> u8 {
        self.buffer_slot
    }
}

#[derive(Debug)]
pub struct TransparentSortState {
    pub(in crate::chunk) next_generation: u64,
    pub(in crate::chunk) requested: Option<(ViewSortGeneration, ViewSortKey)>,
    pub(in crate::chunk) committed: Option<TransparentOrderedSnapshot>,
    pub(in crate::chunk) staged: Option<TransparentStagedSnapshot>,
    pub(in crate::chunk) upload_cap: usize,
    /// Committed-slot ranges the caller must write before this frame renders.
    pub(in crate::chunk) pending_patch: Vec<Range<usize>>,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentStagedSnapshot {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
    pub(in crate::chunk) uploaded: usize,
    pub(in crate::chunk) buffer_slot: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentUploadBatch<'a> {
    pub(in crate::chunk) buffer_slot: u8,
    pub(in crate::chunk) ref_range: Range<usize>,
    pub(in crate::chunk) refs: &'a [PackedTransparentDrawRef],
}

impl TransparentUploadBatch<'_> {
    #[must_use]
    pub const fn buffer_slot(&self) -> u8 {
        self.buffer_slot
    }

    #[must_use]
    pub fn ref_range(&self) -> Range<usize> {
        self.ref_range.clone()
    }

    #[must_use]
    pub const fn refs(&self) -> &[PackedTransparentDrawRef] {
        self.refs
    }
}

impl TransparentSortState {
    #[must_use]
    pub const fn with_upload_cap(upload_cap: usize) -> Self {
        Self {
            next_generation: 0,
            requested: None,
            committed: None,
            staged: None,
            upload_cap: if upload_cap == 0 { 1 } else { upload_cap },
            pending_patch: Vec::new(),
        }
    }

    pub fn request(&mut self, key: &ViewSortKey) -> ViewSortGeneration {
        self.request_retaining_resident_snapshot(key, false, false)
    }

    /// Requests a sort for `key`, keeping each snapshot whose addresses stay readable.
    ///
    /// A staged upload that is still readable finishes and commits before any newer key is
    /// requested, so neither camera motion nor streaming that changes the residents every
    /// frame can starve the bounded inactive-slot upload.
    pub(in crate::chunk) fn request_retaining_resident_snapshot(
        &mut self,
        key: &ViewSortKey,
        committed_addresses_are_resident: bool,
        staged_addresses_are_resident: bool,
    ) -> ViewSortGeneration {
        if let Some((generation, requested_key)) = &self.requested
            && requested_key == key
        {
            return *generation;
        }
        let address_identity_is_safe = self.committed.as_ref().is_none_or(|snapshot| {
            snapshot.key.address_identity_eq(key) || committed_addresses_are_resident
        });
        if !address_identity_is_safe {
            self.committed = None;
        }
        if let Some(staged) = self.staged.as_ref().filter(|snapshot| {
            snapshot.key.address_identity_eq(key) || staged_addresses_are_resident
        }) {
            return staged.generation;
        }
        self.staged = None;
        self.next_generation = self.next_generation.wrapping_add(1).max(1);
        let generation = ViewSortGeneration(self.next_generation);
        self.requested = Some((generation, key.clone()));
        generation
    }

    pub fn complete(
        &mut self,
        result: TransparentSortResult,
    ) -> Result<bool, TransparentSortError> {
        if self
            .requested
            .as_ref()
            .is_none_or(|(generation, key)| *generation != result.generation || key != &result.key)
        {
            return Ok(false);
        }
        // The same address set has the same layout, so changed spans can be written into the
        // committed slot atomically within one frame instead of re-staging every reference.
        if let Some(committed) = self.committed.as_mut()
            && committed.key.address_identity_eq(&result.key)
            && committed.refs.len() == result.refs.len()
        {
            let patch = match result.patch {
                Some((base, spans)) if Arc::ptr_eq(&base, &committed.refs) => spans,
                _ => changed_ref_spans(&committed.refs, &result.refs),
            };
            if patch.iter().map(ExactSizeIterator::len).sum::<usize>() <= self.upload_cap {
                committed.generation = result.generation;
                committed.key = result.key;
                if !patch.is_empty() {
                    committed.refs = result.refs;
                    committed.phase_groups = PhaseGroupCache::default();
                }
                self.pending_patch = patch;
                self.staged = None;
                return Ok(true);
            }
        }
        let buffer_slot = self
            .committed
            .as_ref()
            .map_or(0, |snapshot| 1 - snapshot.buffer_slot);
        if result.refs.is_empty() {
            self.committed = Some(TransparentOrderedSnapshot {
                generation: result.generation,
                key: result.key,
                refs: result.refs,
                buffer_slot,
                phase_groups: PhaseGroupCache::default(),
            });
            self.staged = None;
            return Ok(true);
        }
        self.staged = Some(TransparentStagedSnapshot {
            generation: result.generation,
            key: result.key,
            refs: result.refs,
            uploaded: 0,
            buffer_slot,
        });
        Ok(false)
    }

    #[must_use]
    pub fn next_upload_batch(&self) -> Option<TransparentUploadBatch<'_>> {
        let staged = self.staged.as_ref()?;
        let end = staged
            .uploaded
            .saturating_add(self.upload_cap)
            .min(staged.refs.len());
        (end > staged.uploaded).then(|| TransparentUploadBatch {
            buffer_slot: staged.buffer_slot,
            ref_range: staged.uploaded..end,
            refs: &staged.refs[staged.uploaded..end],
        })
    }

    /// Acknowledges that the batch returned by [`Self::next_upload_batch`] was
    /// written successfully. Returns true only when the inactive slot became
    /// complete and was atomically promoted to the committed snapshot.
    pub fn acknowledge_upload(&mut self) -> bool {
        let Some(staged) = self.staged.as_mut() else {
            return false;
        };
        let remaining = staged.refs.len().saturating_sub(staged.uploaded);
        let uploaded = remaining.min(self.upload_cap);
        if uploaded == 0 {
            return false;
        }
        staged.uploaded += uploaded;
        if staged.uploaded == staged.refs.len() {
            let staged = self.staged.take().expect("staged snapshot exists");
            self.committed = Some(TransparentOrderedSnapshot {
                generation: staged.generation,
                key: staged.key,
                refs: staged.refs,
                buffer_slot: staged.buffer_slot,
                phase_groups: PhaseGroupCache::default(),
            });
            return true;
        }
        false
    }

    /// Takes the committed-slot spans that the last in-place commit changed.
    pub fn take_patch(&mut self) -> Vec<Range<usize>> {
        std::mem::take(&mut self.pending_patch)
    }

    #[must_use]
    pub const fn committed(&self) -> Option<&TransparentOrderedSnapshot> {
        self.committed.as_ref()
    }

    /// Refs already written to each GPU slot: the committed snapshot and any uploaded staged prefix.
    pub(in crate::chunk) fn resident_refs(
        &self,
    ) -> impl Iterator<Item = (u8, &[PackedTransparentDrawRef])> {
        let committed = self
            .committed
            .as_ref()
            .map(|snapshot| (snapshot.buffer_slot, &snapshot.refs[..]));
        let staged = self
            .staged
            .as_ref()
            .map(|snapshot| (snapshot.buffer_slot, &snapshot.refs[..snapshot.uploaded]));
        committed.into_iter().chain(staged)
    }

    #[must_use]
    pub fn staged_ref_count(&self) -> usize {
        self.staged
            .as_ref()
            .map_or(0, |snapshot| snapshot.refs.len())
    }

    pub(in crate::chunk) fn staged_generation(&self) -> Option<ViewSortGeneration> {
        self.staged.as_ref().map(|snapshot| snapshot.generation)
    }

    /// The key of the snapshot being uploaded into the inactive slot, if any.
    pub(in crate::chunk) fn staged_key(&self) -> Option<&ViewSortKey> {
        self.staged.as_ref().map(|snapshot| &snapshot.key)
    }

    /// Keys whose refs a frame may draw: the committed snapshot's, and the staged one's,
    /// which commits once its upload completes.
    pub(in crate::chunk) fn retained_keys(&self) -> impl Iterator<Item = &ViewSortKey> {
        self.committed
            .as_ref()
            .map(|snapshot| &snapshot.key)
            .into_iter()
            .chain(self.staged_key())
    }

    pub fn reset_preserving_generation(&mut self) {
        self.requested = None;
        self.committed = None;
        self.staged = None;
        self.pending_patch.clear();
    }
}

// Writing a few unchanged references is cheaper than another buffer write call.
const PATCH_MERGE_GAP: usize = 32;

pub(in crate::chunk) fn changed_ref_spans(
    old: &[PackedTransparentDrawRef],
    new: &[PackedTransparentDrawRef],
) -> Vec<Range<usize>> {
    let mut spans = Vec::<Range<usize>>::new();
    for (index, _) in old
        .iter()
        .zip(new)
        .enumerate()
        .filter(|(_, (old, new))| old != new)
    {
        match spans.last_mut() {
            Some(span) if index - span.end <= PATCH_MERGE_GAP => span.end = index + 1,
            _ => spans.push(index..index + 1),
        }
    }
    spans
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentSortWork {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) requested_at: Instant,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) camera: Vec3,
    /// In `key.sorted_allocations` order, which is the committed layout.
    pub(in crate::chunk) groups: TransparentGroups,
    /// Parallel to `groups`.
    pub(in crate::chunk) cached: Vec<Option<TransparentGroupOrder>>,
    /// The committed order when it has this layout, so the worker can diff against it.
    pub(in crate::chunk) base: Option<Arc<[PackedTransparentDrawRef]>>,
    pub(in crate::chunk) distinct_tint_count: usize,
}

#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentCandidateCache {
    pub(in crate::chunk) address_identity: TransparentAddressIdentity,
    pub(in crate::chunk) groups: TransparentGroups,
    pub(in crate::chunk) distinct_tint_count: usize,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentWorkerResult {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) requested_at: Instant,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Result<Arc<[PackedTransparentDrawRef]>, TransparentSortError>,
    pub(in crate::chunk) patch: Option<TransparentRefPatch>,
    pub(in crate::chunk) fresh: Vec<TransparentGroupOrder>,
    pub(in crate::chunk) cpu_duration: Duration,
    pub(in crate::chunk) distinct_tint_count: usize,
}

#[derive(Resource)]
pub(in crate::chunk) struct TransparentSortRuntime {
    pub(in crate::chunk) view_entity: Option<Entity>,
    pub(in crate::chunk) state: TransparentSortState,
    pub(in crate::chunk) gate: TransparentSortJobGate<TransparentSortWork>,
    pub(in crate::chunk) result_sender: SyncSender<TransparentWorkerResult>,
    pub(in crate::chunk) result_receiver: Mutex<Receiver<TransparentWorkerResult>>,
    pub(in crate::chunk) requested_at: HashMap<ViewSortGeneration, Instant>,
    pub(in crate::chunk) staged_distinct_tint_counts: HashMap<ViewSortGeneration, usize>,
    pub(in crate::chunk) committed_distinct_tint_count: usize,
    pub(in crate::chunk) last_indirect_identity: Option<(u8, usize)>,
    pub(in crate::chunk) candidate_cache: Option<TransparentCandidateCache>,
    pub(in crate::chunk) group_inputs: HashMap<SubChunkKey, Arc<TransparentGroupInput>>,
    pub(in crate::chunk) group_orders: HashMap<SubChunkKey, TransparentGroupOrder>,
    pub(in crate::chunk) manifest: Option<TransparentManifest>,
    pub(in crate::chunk) last_ceiling_log: Option<Instant>,
    /// Whether the active view draws order-independent water from its records instead of
    /// sorting it.
    pub(in crate::chunk) direct_order_independent: bool,
}
