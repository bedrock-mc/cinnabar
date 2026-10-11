use crate::chunk::*;
use meshing::liquid::TRANSPARENT_WATER_DRAW_FLAG;

/// Hard 16 MiB ceiling for one committed transparent indirection snapshot.
pub const MAX_TRANSPARENT_DRAW_REFS: usize = 2_097_152;
pub const MAX_TRANSPARENT_VIEWS: usize = 1;
/// The largest one slot of the double-buffered ref buffer grows to.
pub const TRANSPARENT_REF_SLOT_BYTES: usize =
    MAX_TRANSPARENT_DRAW_REFS * std::mem::size_of::<PackedTransparentDrawRef>();
pub const TRANSPARENT_REF_BUFFER_BYTES: usize = TRANSPARENT_REF_SLOT_BYTES * 2;
/// Refs per slot before the first growth; slots double up to the ceiling as snapshots need.
pub(in crate::chunk) const INITIAL_TRANSPARENT_SLOT_REFS: usize = 16_384;
pub const DEFAULT_TRANSPARENT_UPLOAD_REFS_PER_FRAME: usize = 131_072;
pub const MAX_TRANSPARENT_WITNESS_KEYS: usize = 64;
pub const MAX_MODEL_WITNESS_KEYS: usize = 64;
pub(in crate::chunk) const MAX_TRANSPARENT_RETIRED_ALLOCATIONS: usize = 16_384;
pub(in crate::chunk) const MAX_TRANSPARENT_RETIRED_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TransparentDrawArgs {
    pub index_count: u32,
    pub instance_count: u32,
    pub first_index: u32,
    pub base_vertex: i32,
    pub first_instance: u32,
}

pub(in crate::chunk) fn transparent_draw_args(
    buffer_slot: u8,
    slot_refs: usize,
    ref_count: usize,
) -> Option<TransparentDrawArgs> {
    transparent_draw_range_args(buffer_slot, slot_refs, 0..u32::try_from(ref_count).ok()?)
}

/// `slot_refs` is the arena's current per-slot capacity, the stride between the two slots. The
/// first instance carries [`TRANSPARENT_WATER_DRAW_FLAG`], which the transparent pipeline reads.
pub(in crate::chunk) fn transparent_draw_range_args(
    buffer_slot: u8,
    slot_refs: usize,
    ref_range: Range<u32>,
) -> Option<TransparentDrawArgs> {
    if ref_range.start > ref_range.end
        || slot_refs > MAX_TRANSPARENT_DRAW_REFS
        || usize::try_from(ref_range.end).ok()? > slot_refs
    {
        return None;
    }
    let instance_count = ref_range.end - ref_range.start;
    let first_instance = u32::from(buffer_slot)
        .checked_mul(u32::try_from(slot_refs).ok()?)?
        .checked_add(ref_range.start)?;
    if first_instance & TRANSPARENT_WATER_DRAW_FLAG != 0 {
        return None;
    }
    Some(TransparentDrawArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex: 0,
        first_instance: first_instance | TRANSPARENT_WATER_DRAW_FLAG,
    })
}

/// Byte offset of `start` within `buffer_slot` at the arena's current slot stride.
pub(in crate::chunk) fn transparent_ref_offset(
    buffer_slot: u8,
    slot_refs: usize,
    start: usize,
) -> u64 {
    ((usize::from(buffer_slot) * slot_refs + start)
        * std::mem::size_of::<PackedTransparentDrawRef>()) as u64
}

pub(in crate::chunk) fn transparent_ref_buffer(device: &RenderDevice, slot_refs: usize) -> Buffer {
    create_storage_buffer(
        device,
        "double-buffered transparent draw refs",
        transparent_ref_offset(2, slot_refs, 0),
    )
}

/// One resident slot's refs moved from the old slot stride to the new one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentRefCopy {
    pub(in crate::chunk) source: u64,
    pub(in crate::chunk) destination: u64,
    pub(in crate::chunk) bytes: u64,
}

/// The GPU copies that carry every resident ref across a stride change.
pub(in crate::chunk) fn transparent_ref_growth_copies(
    state: &TransparentSortState,
    old_slot_refs: usize,
    new_slot_refs: usize,
) -> Vec<TransparentRefCopy> {
    state
        .resident_refs()
        .filter(|(_, refs)| !refs.is_empty())
        .map(|(slot, refs)| TransparentRefCopy {
            source: transparent_ref_offset(slot, old_slot_refs, 0),
            destination: transparent_ref_offset(slot, new_slot_refs, 0),
            bytes: transparent_ref_offset(0, 0, refs.len()),
        })
        .collect()
}

/// Grows both slots to hold `refs`, copying resident refs on the GPU so growth uploads nothing.
/// Returns whether the buffer was replaced, which invalidates written indirect args.
pub(in crate::chunk) fn ensure_transparent_ref_capacity(
    arena: &mut ChunkGpuArena,
    device: &RenderDevice,
    queue: &RenderQueue,
    refs: usize,
    state: &TransparentSortState,
) -> bool {
    if refs <= arena.transparent_slot_refs {
        return false;
    }
    let slot_refs = refs
        .min(MAX_TRANSPARENT_DRAW_REFS)
        .next_power_of_two()
        .clamp(INITIAL_TRANSPARENT_SLOT_REFS, MAX_TRANSPARENT_DRAW_REFS);
    let copies = transparent_ref_growth_copies(state, arena.transparent_slot_refs, slot_refs);
    let grown = transparent_ref_buffer(device, slot_refs);
    if !copies.is_empty() {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("transparent ref growth"),
        });
        for copy in &copies {
            encoder.copy_buffer_to_buffer(
                &arena.transparent_ref_buffer,
                copy.source,
                &grown,
                copy.destination,
                copy.bytes,
            );
        }
        // The old buffer is released once this submission no longer needs it.
        let command = encoder.finish();
        #[cfg(feature = "tracy")]
        let _span = bevy::log::info_span!(
            "terrain.transparent_growth_submit",
            copies = copies.len(),
            bytes = copies.iter().map(|copy| copy.bytes).sum::<u64>(),
        )
        .entered();
        queue.submit([command]);
    }
    arena.transparent_ref_buffer = grown;
    arena.transparent_slot_refs = slot_refs;
    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentLiquidPhaseGroup {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) ref_range: Range<u32>,
}

pub(in crate::chunk) fn transparent_liquid_phase_groups(
    snapshot: &TransparentOrderedSnapshot,
) -> Option<Vec<TransparentLiquidPhaseGroup>> {
    let mut identities = HashMap::with_capacity(snapshot.key.sorted_allocations.len());
    for identity in snapshot.key.sorted_allocations.iter() {
        if !identity.liquid_range.start.is_multiple_of(4)
            || !identity.liquid_range.end.is_multiple_of(4)
            || identities
                .insert(identity.metadata_index, identity)
                .is_some()
        {
            return None;
        }
    }

    let mut groups = Vec::<TransparentLiquidPhaseGroup>::new();
    let mut closed_metadata = HashSet::new();
    let refs = snapshot.refs();
    let mut start = 0;
    while start < refs.len() {
        let metadata = refs[start].metadata_index();
        let run = refs[start..]
            .iter()
            .position(|draw_ref| draw_ref.metadata_index() != metadata)
            .map_or(refs.len(), |length| start + length);
        let identity = identities.get(&metadata)?;
        let record_range = identity.liquid_range.start / 4..identity.liquid_range.end / 4;
        if !closed_metadata.insert(metadata)
            || !refs[start..run]
                .iter()
                .all(|draw_ref| record_range.contains(&draw_ref.liquid_record_index()))
        {
            return None;
        }
        groups.push(TransparentLiquidPhaseGroup {
            key: identity.key,
            ref_range: u32::try_from(start).ok()?..u32::try_from(run).ok()?,
        });
        start = run;
    }
    Some(groups)
}

pub(in crate::chunk) fn transparent_indirect_args(
    snapshot: &TransparentOrderedSnapshot,
    slot_refs: usize,
) -> Option<DrawIndexedIndirectArgs> {
    let args = transparent_draw_args(snapshot.buffer_slot(), slot_refs, snapshot.refs().len())?;
    Some(DrawIndexedIndirectArgs {
        index_count: args.index_count,
        instance_count: args.instance_count,
        first_index: args.first_index,
        base_vertex: args.base_vertex,
        first_instance: args.first_instance,
    })
}

/// One absolute liquid-record/chunk-metadata pair in committed back-to-front order.
///
/// The liquid record carries its absolute lighting address in word 3. These
/// references belong to a committed per-view snapshot, never to `ChunkMesh`.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PackedTransparentDrawRef {
    pub(in crate::chunk) liquid_record_index: u32,
    pub(in crate::chunk) metadata_index: u32,
}

impl PackedTransparentDrawRef {
    #[must_use]
    pub const fn new(liquid_record_index: u32, metadata_index: u32) -> Self {
        Self {
            liquid_record_index,
            metadata_index,
        }
    }

    #[must_use]
    pub const fn liquid_record_index(self) -> u32 {
        self.liquid_record_index
    }

    #[must_use]
    pub const fn metadata_index(self) -> u32 {
        self.metadata_index
    }
}

const _: () = assert!(std::mem::size_of::<PackedTransparentDrawRef>() == 8);

mod groups;
mod layout;
mod manifest;
mod prepare;
mod state;

pub(in crate::chunk) use groups::{
    TransparentGroupInput, TransparentGroups, build_transparent_group, distinct_tint_count,
    sort_group, spawn_transparent_sort,
};
pub(in crate::chunk) use layout::{
    TransparentLayoutBase, TransparentRefPatch, TransparentSnapshotLayout, TransparentSortOutput,
    plan_transparent_slot,
};
pub(in crate::chunk) use manifest::view_displaces_water;
pub(in crate::chunk) use prepare::{
    prepare_transparent_sorts, transparent_snapshot_addresses_are_resident,
};
pub use state::{
    TransparentAllocationIdentity, TransparentOrderedSnapshot, TransparentSortError,
    TransparentSortJobGate, TransparentSortResult, TransparentSortState, TransparentUploadBatch,
    ViewSortGeneration, ViewSortKey, validate_transparent_sort_ref_count,
};
pub(in crate::chunk) use state::{
    TransparentSortRuntime, TransparentSortWork, TransparentStagedSnapshot,
    TransparentWorkerResult, changed_ref_spans,
};

#[cfg(test)]
mod growth_tests {
    use super::*;

    /// The ref buffer starts small and grows only when a staged snapshot needs it.
    #[test]
    fn transparent_ref_buffer_grows_on_demand_and_keeps_resident_refs() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let mut arena = ChunkGpuArena::new(&device);
        let initial = transparent_ref_offset(2, INITIAL_TRANSPARENT_SLOT_REFS, 0);
        assert_eq!(arena.transparent_ref_buffer.size(), initial);
        assert!(initial < TRANSPARENT_REF_BUFFER_BYTES as u64 / 64);

        let mut state = TransparentSortState::with_upload_cap(usize::MAX);
        let key = ViewSortKey::try_new(
            [0.0; 3],
            Vec::new(),
            ChunkTextureAssetIdentity::new(1, 1),
            ChunkBiomeTintIdentity::new(2, 2),
        )
        .unwrap();
        let generation = state.request(&key);
        let refs = vec![PackedTransparentDrawRef::new(1, 2); 3];
        state
            .complete(TransparentSortResult::new(generation, key, refs).unwrap())
            .unwrap();
        assert!(state.acknowledge_upload());
        assert_eq!(state.resident_refs().count(), 1);

        assert!(!ensure_transparent_ref_capacity(
            &mut arena,
            &device,
            &queue,
            INITIAL_TRANSPARENT_SLOT_REFS,
            &state,
        ));
        let before = arena.transparent_ref_buffer.id();
        // Growth moves the three committed refs on the GPU: one copy, nothing uploaded.
        assert_eq!(
            transparent_ref_growth_copies(
                &state,
                INITIAL_TRANSPARENT_SLOT_REFS,
                INITIAL_TRANSPARENT_SLOT_REFS * 2
            ),
            [TransparentRefCopy {
                source: 0,
                destination: 0,
                bytes: 3 * std::mem::size_of::<PackedTransparentDrawRef>() as u64,
            }]
        );
        assert!(ensure_transparent_ref_capacity(
            &mut arena,
            &device,
            &queue,
            INITIAL_TRANSPARENT_SLOT_REFS + 1,
            &state,
        ));
        assert_ne!(arena.transparent_ref_buffer.id(), before);
        assert_eq!(
            arena.transparent_slot_refs,
            INITIAL_TRANSPARENT_SLOT_REFS * 2
        );
        assert_eq!(arena.transparent_ref_buffer.size(), initial * 2);
        let args = transparent_draw_args(1, arena.transparent_slot_refs, 3).unwrap();
        assert_eq!(
            args.first_instance,
            (INITIAL_TRANSPARENT_SLOT_REFS as u32 * 2) | TRANSPARENT_WATER_DRAW_FLAG
        );

        ensure_transparent_ref_capacity(&mut arena, &device, &queue, usize::MAX, &state);
        assert_eq!(arena.transparent_slot_refs, MAX_TRANSPARENT_DRAW_REFS);
        assert_eq!(
            arena.transparent_ref_buffer.size(),
            TRANSPARENT_REF_BUFFER_BYTES as u64
        );
    }

    /// A half-uploaded staged slot moves with the committed one; nothing is re-uploaded.
    #[test]
    fn transparent_ref_growth_copies_each_resident_slot_at_its_new_stride() {
        let key = |x| {
            ViewSortKey::try_new(
                [x, 0.0, 0.0],
                Vec::new(),
                ChunkTextureAssetIdentity::new(1, 1),
                ChunkBiomeTintIdentity::new(2, 2),
            )
            .unwrap()
        };
        let mut state = TransparentSortState::with_upload_cap(2);
        let refs = |count| vec![PackedTransparentDrawRef::new(1, 2); count];
        let generation = state.request(&key(0.0));
        state
            .complete(TransparentSortResult::new(generation, key(0.0), refs(3)).unwrap())
            .unwrap();
        assert!(!state.acknowledge_upload());
        assert!(state.acknowledge_upload());
        let generation = state.request(&key(16.0));
        state
            .complete(TransparentSortResult::new(generation, key(16.0), refs(4)).unwrap())
            .unwrap();
        assert!(!state.acknowledge_upload());
        let size = std::mem::size_of::<PackedTransparentDrawRef>() as u64;
        assert_eq!(
            transparent_ref_growth_copies(&state, 8, 32),
            [
                TransparentRefCopy {
                    source: 0,
                    destination: 0,
                    bytes: 3 * size,
                },
                TransparentRefCopy {
                    source: 8 * size,
                    destination: 32 * size,
                    bytes: 2 * size,
                },
            ]
        );
    }
}
