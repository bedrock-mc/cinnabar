//! The working state of one merge: the sections it holds expanded, the stored sections
//! it read for their face planes, and the per-level work it schedules.

use std::collections::{BTreeMap, HashMap};

use super::{MergeStats, Merged, SectionSource};
use crate::{
    classify::MaterialClasses,
    exposure::{FacePlane, Neighbourhood},
    geometry::{Face, SectionKey, SectionPos, VoxelBox},
    mip::{mip_into_parent, unknown_section},
    sampler::SampledSubChunk,
    section::{ChildMask, ChildState, Section},
    storage::{DenseSection, PackScratch},
};

/// What a section needs at its level.
#[derive(Debug, Clone, Copy, Default)]
pub(super) struct LevelWork {
    /// Voxels whose content changed.
    changed: Option<VoxelBox>,
    /// Voxels to mip again because a neighbour plane changed.
    remip: Option<VoxelBox>,
    /// The section's child mask changed, so its parent's mask may too.
    mask_changed: bool,
}

impl LevelWork {
    /// Records changed content in `region`.
    pub(super) fn change(&mut self, region: VoxelBox) {
        self.changed = Some(self.changed.map_or(region, |changed| changed.union(region)));
    }

    /// Records a neighbour change across the slab `region`.
    fn remip(&mut self, region: VoxelBox) {
        self.remip = Some(self.remip.map_or(region, |remip| remip.union(region)));
    }

    /// The voxels to mip into the parent.
    fn dirty(self) -> Option<VoxelBox> {
        match (self.changed, self.remip) {
            (Some(changed), Some(remip)) => Some(changed.union(remip)),
            (changed, remip) => changed.or(remip),
        }
    }
}

/// A section the merge holds expanded.
struct Working {
    dense: DenseSection,
    mask: ChildMask,
    /// No section was stored before this merge.
    created: bool,
    /// The merge changed the section and returns it.
    write: bool,
}

impl Working {
    /// True when the section exists once the merge is stored: it was stored before or
    /// the merge writes it.
    fn present(&self) -> bool {
        !self.created || self.write
    }
}

pub(super) struct Merge<'s, S, C> {
    source: &'s mut S,
    classes: &'s C,
    working: HashMap<SectionKey, Working>,
    /// Stored sections read only for their face planes.
    stored: HashMap<SectionKey, Option<Section>>,
    stats: MergeStats,
}

impl<'s, S: SectionSource, C: MaterialClasses> Merge<'s, S, C> {
    /// A merge reading from `source` with `classes`, holding nothing yet.
    pub(super) fn new(source: &'s mut S, classes: &'s C) -> Self {
        Self {
            source,
            classes,
            working: HashMap::new(),
            stored: HashMap::new(),
            stats: MergeStats::default(),
        }
    }

    /// Writes `sub`'s octant into its level-0 section.
    pub(super) fn write_sub_chunk(&mut self, sub: &SampledSubChunk) -> Result<(), S::Error> {
        let section = self.working(sub.section())?;
        sub.write_into(&mut section.dense);
        section.write = true;
        self.stats.sub_chunks += 1;
        Ok(())
    }

    /// The section at `pos` expanded, loading it or starting it unknown.
    fn working(&mut self, pos: SectionPos) -> Result<&mut Working, S::Error> {
        let key = pos.key();
        if !self.working.contains_key(&key) {
            let stored = match self.stored.remove(&key) {
                Some(stored) => stored,
                None => self.source.load(pos)?,
            };
            let mut dense = unknown_section();
            let working = match stored {
                Some(section) => {
                    section.expand_into(&mut dense);
                    Working {
                        dense,
                        mask: section.children(),
                        created: false,
                        write: false,
                    }
                }
                None => Working {
                    dense,
                    mask: ChildMask::UNKNOWN,
                    created: true,
                    write: false,
                },
            };
            self.working.insert(key, working);
        }
        Ok(self.working.get_mut(&key).expect("inserted above"))
    }

    /// The stored section at `pos` for a plane, read at most once.
    fn stored(&mut self, pos: SectionPos) -> Result<Option<&Section>, S::Error> {
        let key = pos.key();
        if !self.stored.contains_key(&key) {
            let section = self.source.load(pos)?;
            self.stored.insert(key, section);
        }
        Ok(self.stored[&key].as_ref())
    }

    /// True when the section at `pos` exists once the merge is stored.
    fn present(&mut self, pos: SectionPos) -> Result<bool, S::Error> {
        match self.working.get(&pos.key()) {
            Some(working) => Ok(working.present()),
            None => Ok(self.stored(pos)?.is_some()),
        }
    }

    /// The planes across `pos`'s faces from present neighbours at its level.
    fn neighbourhood(&mut self, pos: SectionPos) -> Result<Neighbourhood, S::Error> {
        let classes = self.classes;
        let mut neighbourhood = Neighbourhood::UNKNOWN;
        for face in Face::ALL {
            let Some(neighbour) = pos.neighbour(face) else {
                continue;
            };
            let plane = match self.working.get(&neighbour.key()) {
                Some(working) if working.present() => {
                    Some(FacePlane::across(face, &working.dense, classes))
                }
                Some(_) => None,
                None => self
                    .stored(neighbour)?
                    .map(|section| FacePlane::across_section(face, section, classes)),
            };
            neighbourhood.set(face, plane);
        }
        Ok(neighbourhood)
    }

    /// Schedules the facing slab of every present neighbour of a section in `level` that
    /// was created or changed on that face.
    pub(super) fn schedule_neighbour_slabs(
        &mut self,
        level: &mut BTreeMap<SectionKey, LevelWork>,
    ) -> Result<(), S::Error> {
        let changed: Vec<(SectionKey, VoxelBox)> = level
            .iter()
            .filter_map(|(&key, work)| Some((key, work.changed?)))
            .collect();
        for (key, region) in changed {
            let created = self
                .working
                .get(&key)
                .is_some_and(|working| working.created);
            for face in Face::ALL {
                if !created && region.intersection(face.slab()).is_none() {
                    continue;
                }
                let Some(neighbour) = key.pos().neighbour(face) else {
                    continue;
                };
                if self.present(neighbour)? {
                    level
                        .entry(neighbour.key())
                        .or_default()
                        .remip(face.opposite().slab());
                    self.stats.neighbour_slabs += 1;
                }
            }
        }
        Ok(())
    }

    /// Mips `work` of the section at `pos` into its parent and records what that changed
    /// in `parents`.
    pub(super) fn mip_into_parent(
        &mut self,
        pos: SectionPos,
        work: LevelWork,
        parents: &mut BTreeMap<SectionKey, LevelWork>,
    ) -> Result<(), S::Error> {
        let parent_pos = pos.parent().expect("local levels have parents");
        let neighbours = self.neighbourhood(pos)?;
        self.working(pos)?;
        self.working(parent_pos)?;
        let mut parent = self
            .working
            .remove(&parent_pos.key())
            .expect("loaded above");
        let child = &self.working[&pos.key()];
        let state = if pos.level() == 0 {
            ChildState::of_level_zero(&child.dense)
        } else {
            child.mask.summary()
        };
        let octant = pos.octant_in_parent();
        let mask_changed = parent.mask.set(octant, state);
        let parent_work = parents.entry(parent_pos.key()).or_default();
        if let Some(dirty) = work.dirty() {
            let step = mip_into_parent(
                &child.dense,
                &neighbours,
                octant,
                dirty,
                self.classes,
                &mut parent.dense,
            );
            self.stats.hidden_fallbacks += step.hidden_fallbacks;
            self.stats.unresolved_hidden += step.unresolved_hidden;
            if let Some(changed) = step.changed {
                parent_work.change(changed);
                parent.write = true;
            }
        }
        if mask_changed {
            parent_work.mask_changed = true;
            parent.write = true;
        }
        if parent_work.dirty().is_none() && !parent_work.mask_changed {
            parents.remove(&parent_pos.key());
        }
        self.working.insert(parent_pos.key(), parent);
        Ok(())
    }

    /// Packs every rewritten section.
    pub(super) fn finish(self) -> Merged {
        let mut scratch = PackScratch::default();
        let mut written: Vec<_> = self
            .working
            .into_iter()
            .filter(|(_, working)| working.write)
            .collect();
        written.sort_unstable_by_key(|(key, _)| *key);
        let sections: Vec<Section> = written
            .into_iter()
            .map(|(key, working)| {
                Section::from_dense(key.pos(), &working.dense, working.mask, &mut scratch)
                    .expect("level-0 sections keep the unknown mask")
            })
            .collect();
        Merged {
            stats: MergeStats {
                sections: sections.len() as u32,
                ..self.stats
            },
            sections,
        }
    }
}
