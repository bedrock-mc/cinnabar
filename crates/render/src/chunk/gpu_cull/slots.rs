//! The slot-indexed record mirror and the records built from resident allocations.

use crate::chunk::*;

use super::model::{CullRecord, CullRecordSource};

const SIDE: i32 = world::SUB_CHUNK_SIDE as i32;
/// Models may overhang their sub-chunk by one block.
const MODEL_BOUNDS: [[i32; 3]; 2] = [[-1; 3], [SIDE + 1; 3]];
const FULL_BOUNDS: [[i32; 3]; 2] = [[0; 3], [SIDE; 3]];

#[derive(Clone, Copy)]
struct SlotOwner {
    entity: Entity,
    tint: ChunkBiomeTintIdentity,
}

/// Slot-indexed record mirror and enabled bits, tracking what the GPU copy still needs.
#[derive(Default)]
pub(in crate::chunk) struct CullSlots {
    records: Vec<CullRecord>,
    owners: Vec<Option<SlotOwner>>,
    slots: HashMap<Entity, u32>,
    enabled: Vec<u32>,
    enabled_dirty: bool,
    dirty: Vec<u32>,
    tint_identity: Option<ChunkBiomeTintIdentity>,
}

impl CullSlots {
    pub(in crate::chunk) fn slot_count(&self) -> u32 {
        self.records.len() as u32
    }

    pub(in crate::chunk) fn records(&self) -> &[CullRecord] {
        &self.records
    }

    pub(in crate::chunk) fn enabled(&self) -> &[u32] {
        &self.enabled
    }

    pub(in crate::chunk) fn contains(&self, entity: Entity) -> bool {
        self.slots.contains_key(&entity)
    }

    /// Frees `entity`'s slot after its allocation component is gone.
    pub(in crate::chunk) fn remove(&mut self, entity: Entity) {
        if let Some(slot) = self.slots.remove(&entity) {
            self.clear_slot(slot, entity);
        }
    }

    /// Writes `entity`'s record at its (possibly new) metadata slot.
    pub(in crate::chunk) fn update(
        &mut self,
        entity: Entity,
        slot: u32,
        tint: ChunkBiomeTintIdentity,
        record: CullRecord,
        hidden: &HashSet<Entity>,
    ) {
        if let Some(previous) = self.slots.insert(entity, slot)
            && previous != slot
        {
            self.clear_slot(previous, entity);
        }
        let index = slot as usize;
        if self.records.len() <= index {
            // Fresh slots may still hold records a trim left on the GPU.
            self.dirty.extend(self.records.len() as u32..slot);
            self.records.resize(index + 1, CullRecord::default());
            self.owners.resize(index + 1, None);
        }
        self.records[index] = record;
        self.owners[index] = Some(SlotOwner { entity, tint });
        self.dirty.push(slot);
        self.refresh(slot, hidden);
    }

    /// Re-derives `entity`'s enabled bit after its cave visibility flipped.
    pub(in crate::chunk) fn refresh_entity(&mut self, entity: Entity, hidden: &HashSet<Entity>) {
        if let Some(&slot) = self.slots.get(&entity) {
            self.refresh(slot, hidden);
        }
    }

    /// Disables every slot whose mesh predates the active biome-tint table.
    pub(in crate::chunk) fn set_tint(
        &mut self,
        tint: ChunkBiomeTintIdentity,
        hidden: &HashSet<Entity>,
    ) {
        if self.tint_identity == Some(tint) {
            return;
        }
        self.tint_identity = Some(tint);
        for slot in 0..self.slot_count() {
            self.refresh(slot, hidden);
        }
    }

    /// Drops trailing free slots so the cull dispatch covers only the live watermark.
    pub(in crate::chunk) fn trim(&mut self) {
        while self.owners.last().is_some_and(Option::is_none) {
            self.owners.pop();
            self.records.pop();
        }
        let len = self.records.len();
        self.dirty.retain(|&slot| (slot as usize) < len);
    }

    /// Sorted, deduplicated dirty slots; the caller uploads them.
    pub(in crate::chunk) fn take_dirty(&mut self) -> Vec<u32> {
        let mut dirty = std::mem::take(&mut self.dirty);
        dirty.sort_unstable();
        dirty.dedup();
        dirty
    }

    pub(in crate::chunk) fn take_enabled_dirty(&mut self) -> bool {
        std::mem::take(&mut self.enabled_dirty)
    }

    pub(super) fn mark_all_dirty(&mut self) {
        self.dirty = (0..self.slot_count()).collect();
        self.enabled_dirty = true;
    }

    fn refresh(&mut self, slot: u32, hidden: &HashSet<Entity>) {
        let tint = self.tint_identity;
        let enabled = self.owners[slot as usize].is_some_and(|owner| {
            !hidden.contains(&owner.entity)
                && tint.is_some_and(|tint| chunk_tint_identity_is_active(owner.tint, tint))
        });
        self.set_enabled(slot, enabled);
    }

    fn set_enabled(&mut self, slot: u32, value: bool) {
        let (word, bit) = (slot as usize / 32, 1 << (slot % 32));
        if self.enabled.len() <= word {
            self.enabled.resize(word + 1, 0);
        }
        let old = self.enabled[word];
        self.enabled[word] = if value { old | bit } else { old & !bit };
        self.enabled_dirty |= old != self.enabled[word];
    }

    fn clear_slot(&mut self, slot: u32, entity: Entity) {
        if self.owners[slot as usize].is_some_and(|owner| owner.entity == entity) {
            self.owners[slot as usize] = None;
            self.records[slot as usize] = CullRecord::default();
            self.dirty.push(slot);
            self.set_enabled(slot, false);
        }
    }
}

/// Builds a slot's record from the same validated ranges the CPU draw path uses.
pub(in crate::chunk) fn cull_record(
    allocation: &GpuChunkAllocation,
    instance: Option<&ChunkRenderInstance>,
) -> CullRecord {
    let Some(base_vertex) = metadata_base_vertex(allocation.metadata_index) else {
        return CullRecord::default();
    };
    let mut source = CullRecordSource {
        origin: chunk_origin(allocation.key),
        base_vertex,
        ..default()
    };
    let mut bounds: Option<[[i32; 3]; 2]> = None;
    let mut include = |extent: [[i32; 3]; 2]| {
        bounds = Some(bounds.map_or(extent, |[low, high]| {
            [
                std::array::from_fn(|axis| low[axis].min(extent[0][axis])),
                std::array::from_fn(|axis| high[axis].max(extent[1][axis])),
            ]
        }));
    };
    if let Some((cube, layout, _)) = cube_draw_base(allocation) {
        let quads = instance
            .map(|instance| &*instance.cube_quads)
            .filter(|quads| quads.len() as u32 == cube.end - cube.start);
        match quads {
            Some(quads) => quads.iter().for_each(|quad| include(quad_bounds(quad))),
            None => include(FULL_BOUNDS),
        }
        source.solid_ends =
            CubeQuadLayout::SOLID_FACE_ORDER.map(|face| layout.solid_range(face).end);
        source.cube = cube;
    }
    if let Some(draw) = model_mdi_draw_command(allocation) {
        source.model = draw.first_instance..draw.first_instance + draw.instance_count;
        include(MODEL_BOUNDS);
    }
    if let Some(draw) = depth_liquid_mdi_draw_command(allocation) {
        source.liquid = draw.first_instance..draw.first_instance + draw.instance_count;
        include(FULL_BOUNDS);
    }
    source.bounds = bounds.unwrap_or(FULL_BOUNDS);
    CullRecord::new(&source).unwrap_or_default()
}

/// A box containing the quad: it starts at its origin and spans its larger extent on each axis.
pub(super) fn quad_bounds(quad: &PackedQuad) -> [[i32; 3]; 2] {
    let origin = quad.origin().map(i32::from);
    let extent = i32::from(quad.width().max(quad.height()));
    [origin, origin.map(|value| (value + extent).min(SIDE))]
}
