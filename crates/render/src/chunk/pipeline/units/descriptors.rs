//! Stable geometry slots; visibility changes only the ordered references to them.

use super::sources::SectionUnits;
use crate::chunk::*;
use render_model::terrain_units::{TerrainUnit, VisibleTerrainUnit};

struct SectionSlots {
    source: Arc<SectionUnits>,
    slots: Vec<u32>,
}

/// Slots are reused after a section leaves; live sections never move for another section's edit.
#[derive(Default)]
pub(super) struct Descriptors {
    sections: HashMap<Entity, SectionSlots>,
    pub(super) records: Vec<VisibleTerrainUnit>,
    free: Vec<u32>,
    pub(super) dirty: Vec<u32>,
}

impl Descriptors {
    /// Updates changed sections within a single binding; oversized streams use ordered paging.
    pub(super) fn synchronize(
        &mut self,
        sources: &[Arc<SectionUnits>],
        lookup: &HashMap<Entity, usize>,
        capacity: usize,
    ) -> bool {
        self.dirty.clear();
        let count = sources
            .iter()
            .map(|source| source.units.len())
            .sum::<usize>();
        if count > capacity || count == 0 {
            *self = Self::default();
            return count == 0;
        }
        self.sections.retain(|entity, section| {
            if lookup.contains_key(entity) {
                return true;
            }
            self.free.extend(section.slots.iter().copied());
            false
        });
        // Return every shrinking section's excess before growing any other section.
        for source in sources {
            if let Some(section) = self.sections.get_mut(&source.entity)
                && section.slots.len() > source.units.len()
            {
                self.free.extend(section.slots.drain(source.units.len()..));
            }
        }
        for source in sources {
            let mut section =
                self.sections
                    .remove(&source.entity)
                    .unwrap_or_else(|| SectionSlots {
                        source: Arc::clone(source),
                        slots: Vec::new(),
                    });
            if section.slots.len() == source.units.len() && Arc::ptr_eq(&section.source, source) {
                self.sections.insert(source.entity, section);
                continue;
            }
            for (index, unit) in source.units.iter().enumerate() {
                let record = unit.descriptor();
                if index == section.slots.len() {
                    let slot = self.free.pop().unwrap_or_else(|| {
                        let slot = self.records.len() as u32;
                        self.records.push(record);
                        slot
                    });
                    section.slots.push(slot);
                    // A reused slot may contain equal bytes; publishing them is still bounded
                    // by the changed section and also initializes newly allocated slots.
                    self.records[slot as usize] = record;
                    self.dirty.push(slot);
                } else {
                    let slot = section.slots[index];
                    if self.records[slot as usize] != record {
                        self.records[slot as usize] = record;
                        self.dirty.push(slot);
                    }
                }
            }
            section.source = Arc::clone(source);
            self.sections.insert(source.entity, section);
        }
        self.dirty.sort_unstable();
        self.dirty.dedup();
        debug_assert!(self.records.len() <= capacity);
        true
    }

    /// Emits references in the selected pass order while retaining hidden section descriptors.
    pub(super) fn select(
        &self,
        selected: &[Entity],
        camera: Option<[f64; 3]>,
        references: &mut Vec<u32>,
    ) -> u64 {
        references.clear();
        let mut padded = 0;
        for entity in selected {
            let Some(section) = self.sections.get(entity) else {
                continue;
            };
            let mask = facing(section.source.key, camera);
            for (unit, &slot) in section.source.units.iter().zip(&section.slots) {
                if faces(unit, mask) {
                    references.push(slot);
                    padded +=
                        u64::from(render_model::terrain_units::QUADS_PER_UNIT - unit.quad_count);
                }
            }
        }
        padded
    }
}

/// Conservative directional selection shared by retained and oversized streams.
pub(super) fn facing(key: SubChunkKey, camera: Option<[f64; 3]>) -> meshing::FaceMask {
    camera.map_or(meshing::FaceMask::ALL, |camera| {
        meshing::sub_chunk_facing_faces(chunk_origin(key), camera)
    })
}

/// Nondirectional model and liquid units are never removed by cube face selection.
pub(super) fn faces(unit: &TerrainUnit, mask: meshing::FaceMask) -> bool {
    unit.facing
        .is_none_or(|face| mask.contains(meshing::Face::ALL[face as usize]))
}

#[cfg(test)]
mod tests;
