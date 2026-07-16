use crate::chunk::*;

/// Hard 16 MiB ceiling for one committed transparent indirection snapshot.
pub const MAX_TRANSPARENT_DRAW_REFS: usize = 2_097_152;
pub const MAX_TRANSPARENT_VIEWS: usize = 1;
pub const TRANSPARENT_REF_SLOT_BYTES: usize =
    MAX_TRANSPARENT_DRAW_REFS * std::mem::size_of::<PackedTransparentDrawRef>();
pub const TRANSPARENT_REF_BUFFER_BYTES: usize = TRANSPARENT_REF_SLOT_BYTES * 2;
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

#[derive(Debug, Clone, PartialEq)]
pub struct TransparentSortCandidate {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) local_quad_index: u32,
    pub(in crate::chunk) liquid_record_index: u32,
    pub(in crate::chunk) metadata_index: u32,
    pub(in crate::chunk) subchunk_center: [f32; 3],
    pub(in crate::chunk) quad_centroid: [f32; 3],
}

impl TransparentSortCandidate {
    #[must_use]
    pub const fn new(
        key: SubChunkKey,
        local_quad_index: u32,
        liquid_record_index: u32,
        metadata_index: u32,
        subchunk_center: [f32; 3],
        quad_centroid: [f32; 3],
    ) -> Self {
        Self {
            key,
            local_quad_index,
            liquid_record_index,
            metadata_index,
            subchunk_center,
            quad_centroid,
        }
    }
}

pub(in crate::chunk) fn sort_transparent_candidates(
    view_from_world: Mat4,
    candidates: Arc<[TransparentSortCandidate]>,
) -> Vec<PackedTransparentDrawRef> {
    let mut grouped = BTreeMap::<SubChunkKey, Vec<usize>>::new();
    for (index, candidate) in candidates.iter().enumerate() {
        grouped.entry(candidate.key).or_default().push(index);
    }
    let mut groups = grouped.into_iter().collect::<Vec<_>>();
    groups.sort_by(|(left_key, left), (right_key, right)| {
        let left_center = Vec3::from_array(candidates[left[0]].subchunk_center);
        let right_center = Vec3::from_array(candidates[right[0]].subchunk_center);
        view_from_world
            .transform_point3(left_center)
            .z
            .total_cmp(&view_from_world.transform_point3(right_center).z)
            .then_with(|| left_key.cmp(right_key))
    });
    let mut refs = Vec::new();
    for (_key, mut group) in groups {
        group.sort_by(|&left, &right| {
            let left = &candidates[left];
            let right = &candidates[right];
            view_from_world
                .transform_point3(Vec3::from_array(left.quad_centroid))
                .z
                .total_cmp(
                    &view_from_world
                        .transform_point3(Vec3::from_array(right.quad_centroid))
                        .z,
                )
                .then_with(|| left.key.cmp(&right.key))
                .then_with(|| left.local_quad_index.cmp(&right.local_quad_index))
        });
        refs.extend(group.into_iter().map(|index| {
            let candidate = &candidates[index];
            PackedTransparentDrawRef::new(candidate.liquid_record_index, candidate.metadata_index)
        }));
    }
    refs
}

#[doc(hidden)]
pub fn sort_transparent_candidates_for_test(
    view_from_world: Mat4,
    candidates: Vec<TransparentSortCandidate>,
) -> Vec<PackedTransparentDrawRef> {
    sort_transparent_candidates(view_from_world, Arc::from(candidates))
}

pub(in crate::chunk) fn transparent_draw_args(
    buffer_slot: u8,
    ref_count: usize,
) -> Option<TransparentDrawArgs> {
    transparent_draw_range_args(buffer_slot, 0..u32::try_from(ref_count).ok()?)
}

pub(in crate::chunk) fn transparent_draw_range_args(
    buffer_slot: u8,
    ref_range: Range<u32>,
) -> Option<TransparentDrawArgs> {
    if ref_range.start > ref_range.end
        || usize::try_from(ref_range.end).ok()? > MAX_TRANSPARENT_DRAW_REFS
    {
        return None;
    }
    let instance_count = ref_range.end - ref_range.start;
    let first_instance = u32::from(buffer_slot)
        .checked_mul(u32::try_from(MAX_TRANSPARENT_DRAW_REFS).ok()?)?
        .checked_add(ref_range.start)?;
    Some(TransparentDrawArgs {
        index_count: STATIC_QUAD_INDICES.len() as u32,
        instance_count,
        first_index: 0,
        base_vertex: 0,
        first_instance,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentLiquidPhaseGroup {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) ref_range: Range<u32>,
}

pub(in crate::chunk) fn transparent_liquid_phase_groups(
    snapshot: &TransparentOrderedSnapshot,
) -> Option<Vec<TransparentLiquidPhaseGroup>> {
    let mut identities = BTreeMap::new();
    for identity in snapshot.key.visible_allocations.iter() {
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
    for (index, draw_ref) in snapshot.refs().iter().copied().enumerate() {
        let identity = identities.get(&draw_ref.metadata_index())?;
        let record_range = identity.liquid_range.start / 4..identity.liquid_range.end / 4;
        if !record_range.contains(&draw_ref.liquid_record_index()) {
            return None;
        }
        let index = u32::try_from(index).ok()?;
        if let Some(group) = groups.last_mut()
            && group.key == identity.key
        {
            group.ref_range.end = index.checked_add(1)?;
            continue;
        }
        if !closed_metadata.insert(identity.metadata_index) {
            return None;
        }
        groups.push(TransparentLiquidPhaseGroup {
            key: identity.key,
            ref_range: index..index.checked_add(1)?,
        });
    }
    Some(groups)
}

pub(in crate::chunk) fn transparent_indirect_args(
    snapshot: &TransparentOrderedSnapshot,
) -> Option<DrawIndexedIndirectArgs> {
    let args = transparent_draw_args(snapshot.buffer_slot(), snapshot.refs().len())?;
    Some(DrawIndexedIndirectArgs {
        index_count: args.index_count,
        instance_count: args.instance_count,
        first_index: args.first_index,
        base_vertex: args.base_vertex,
        first_instance: args.first_instance,
    })
}

#[doc(hidden)]
pub fn direct_transparent_draw_args_for_test(
    buffer_slot: u8,
    ref_count: usize,
) -> Option<TransparentDrawArgs> {
    transparent_draw_args(buffer_slot, ref_count)
}

#[doc(hidden)]
pub fn mdi_transparent_draw_args_for_test(
    buffer_slot: u8,
    ref_count: usize,
) -> Option<TransparentDrawArgs> {
    transparent_draw_args(buffer_slot, ref_count)
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

pub(in crate::chunk) const _: () = assert!(std::mem::size_of::<PackedTransparentDrawRef>() == 8);

include!("sort_state.rs");
include!("sort_prepare.rs");
