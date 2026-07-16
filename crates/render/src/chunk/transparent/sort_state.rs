
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

    #[doc(hidden)]
    #[must_use]
    pub const fn for_test(value: u64) -> Self {
        Self(value)
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

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ViewSortKey {
    pub(in crate::chunk) camera_position_bits: [u32; 3],
    pub(in crate::chunk) camera_orientation_bits: [u32; 4],
    pub(in crate::chunk) visible_allocations: Arc<[TransparentAllocationIdentity]>,
    pub(in crate::chunk) asset_identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(in crate::chunk) struct TransparentAddressIdentity {
    pub(in crate::chunk) visible_allocations: Arc<[TransparentAllocationIdentity]>,
    pub(in crate::chunk) asset_identity: ChunkTextureAssetIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
}

impl ViewSortKey {
    pub fn try_new(
        camera_position: [f32; 3],
        camera_orientation: [f32; 4],
        mut visible_allocations: Vec<TransparentAllocationIdentity>,
        asset_identity: ChunkTextureAssetIdentity,
        tint_identity: ChunkBiomeTintIdentity,
    ) -> Result<Self, TransparentSortError> {
        if !camera_position.into_iter().all(f32::is_finite)
            || !camera_orientation.into_iter().all(f32::is_finite)
        {
            return Err(TransparentSortError::InvalidCameraTransform);
        }
        let norm_squared = camera_orientation
            .into_iter()
            .map(|value| value * value)
            .sum::<f32>();
        if !norm_squared.is_finite() || norm_squared == 0.0 {
            return Err(TransparentSortError::InvalidCameraTransform);
        }
        let inverse_norm = norm_squared.sqrt().recip();
        let mut orientation = camera_orientation.map(|value| value * inverse_norm);
        let sign_anchor = [
            orientation[3],
            orientation[2],
            orientation[1],
            orientation[0],
        ]
        .into_iter()
        .find(|value| *value != 0.0)
        .unwrap_or(1.0);
        if sign_anchor.is_sign_negative() {
            orientation = orientation.map(|value| -value);
        }
        let canonical_bits = |value: f32| if value == 0.0 { 0 } else { value.to_bits() };
        visible_allocations.sort_by_key(TransparentAllocationIdentity::canonical_tuple);
        visible_allocations.dedup();
        for pair in visible_allocations.windows(2) {
            if pair[0].key == pair[1].key {
                return Err(TransparentSortError::ConflictingAllocation { key: pair[0].key });
            }
        }
        Ok(Self {
            camera_position_bits: camera_position.map(canonical_bits),
            camera_orientation_bits: orientation.map(canonical_bits),
            visible_allocations: Arc::from(visible_allocations),
            asset_identity,
            tint_identity,
        })
    }

    pub(in crate::chunk) fn address_identity_eq(&self, other: &Self) -> bool {
        self.visible_allocations == other.visible_allocations
            && self.asset_identity == other.asset_identity
            && self.tint_identity == other.tint_identity
    }

    pub(in crate::chunk) fn address_identity(&self) -> TransparentAddressIdentity {
        TransparentAddressIdentity {
            visible_allocations: Arc::clone(&self.visible_allocations),
            asset_identity: self.asset_identity,
            tint_identity: self.tint_identity,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentSortResult {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Box<[PackedTransparentDrawRef]>,
}

impl TransparentSortResult {
    pub fn new(
        generation: ViewSortGeneration,
        key: ViewSortKey,
        refs: Vec<PackedTransparentDrawRef>,
    ) -> Result<Self, TransparentSortError> {
        validate_transparent_sort_ref_count(refs.len())?;
        Ok(Self {
            generation,
            key,
            refs: refs.into_boxed_slice(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransparentOrderedSnapshot {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Arc<[PackedTransparentDrawRef]>,
    pub(in crate::chunk) buffer_slot: u8,
}

impl TransparentOrderedSnapshot {
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
        }
    }

    pub fn request(&mut self, key: &ViewSortKey) -> ViewSortGeneration {
        self.request_retaining_resident_snapshot(key, false)
    }

    pub(in crate::chunk) fn request_retaining_resident_snapshot(
        &mut self,
        key: &ViewSortKey,
        committed_addresses_are_resident: bool,
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
        // Exact camera bits may change every frame. Finish a safe inactive-slot
        // upload before accepting another pose so bounded uploads cannot starve.
        if let Some(staged) = self
            .staged
            .as_ref()
            .filter(|snapshot| snapshot.key.address_identity_eq(key))
        {
            return staged.generation;
        }
        if self
            .staged
            .as_ref()
            .is_some_and(|snapshot| !snapshot.key.address_identity_eq(key))
        {
            self.staged = None;
        }
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
        if let Some(committed) = self.committed.as_mut()
            && committed.key.address_identity_eq(&result.key)
            && committed.refs.as_ref() == result.refs.as_ref()
        {
            committed.generation = result.generation;
            committed.key = result.key;
            self.staged = None;
            return Ok(true);
        }
        let buffer_slot = self
            .committed
            .as_ref()
            .map_or(0, |snapshot| 1 - snapshot.buffer_slot);
        if result.refs.is_empty() {
            self.committed = Some(TransparentOrderedSnapshot {
                generation: result.generation,
                key: result.key,
                refs: Arc::from(result.refs),
                buffer_slot,
            });
            self.staged = None;
            return Ok(true);
        }
        self.staged = Some(TransparentStagedSnapshot {
            generation: result.generation,
            key: result.key,
            refs: Arc::from(result.refs),
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
            });
            return true;
        }
        false
    }

    #[must_use]
    pub const fn committed(&self) -> Option<&TransparentOrderedSnapshot> {
        self.committed.as_ref()
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

    pub fn reset_preserving_generation(&mut self) {
        self.requested = None;
        self.committed = None;
        self.staged = None;
    }
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentSortRequest {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) requested_at: Instant,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) view_from_world: Mat4,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentSortWork {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) requested_at: Instant,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) view_from_world: Mat4,
    pub(in crate::chunk) candidates: Arc<[TransparentSortCandidate]>,
    pub(in crate::chunk) distinct_tint_count: usize,
}

#[derive(Debug, Clone)]
pub(in crate::chunk) struct TransparentCandidateCache {
    pub(in crate::chunk) address_identity: TransparentAddressIdentity,
    pub(in crate::chunk) candidates: Arc<[TransparentSortCandidate]>,
    pub(in crate::chunk) distinct_tint_count: usize,
}

#[derive(Debug)]
pub(in crate::chunk) struct TransparentWorkerResult {
    pub(in crate::chunk) generation: ViewSortGeneration,
    pub(in crate::chunk) requested_at: Instant,
    pub(in crate::chunk) key: ViewSortKey,
    pub(in crate::chunk) refs: Result<Vec<PackedTransparentDrawRef>, TransparentSortError>,
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
}
