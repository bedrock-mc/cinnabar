//! Every resident sub-chunk with transparent water, kept current as uploads and removals land.
//!
//! The sort manifest is drawn from here rather than from the frustum, so a camera turn
//! never changes what is sorted; the frustum only selects which sorted groups are drawn.
use crate::chunk::*;

/// One resident allocation's transparent water.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentLiquidResident {
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) identity: TransparentAllocationIdentity,
    pub(in crate::chunk) tint_identity: ChunkBiomeTintIdentity,
    /// Transparent faces; each becomes one ref when sorted.
    pub(in crate::chunk) refs: usize,
    /// Whether the faces blend the same in any order, so they draw straight from the records.
    pub(in crate::chunk) order_independent: bool,
}

impl TransparentLiquidResident {
    /// The transparent water `allocation` uploaded, if it has any addressable water.
    fn from_allocation(entity: Entity, allocation: &GpuChunkAllocation) -> Option<Self> {
        if !allocation.has_transparent_liquid {
            return None;
        }
        let identity = TransparentAllocationIdentity::new(
            allocation.key,
            allocation.generation,
            allocation.liquid_range.clone()?,
            allocation.liquid_lighting_range.clone()?,
            allocation.metadata_index,
        );
        let refs = transparent_liquid_direct_draw_command(allocation)
            .map_or(0, |command| command.instance_count as usize);
        Some(Self {
            entity,
            identity,
            tint_identity: allocation.tint_identity,
            refs,
            order_independent: allocation.order_independent_liquid,
        })
    }

    /// Squared distance in sub-chunks from `camera_chunk`, the order the ref ceiling admits.
    fn chunk_distance_squared(&self, camera_chunk: [i32; 3]) -> i64 {
        let key = self.identity.key;
        [key.x, key.y, key.z]
            .into_iter()
            .zip(camera_chunk)
            .map(|(chunk, camera)| (i64::from(chunk) - i64::from(camera)).pow(2))
            .sum()
    }
}

/// Resident transparent water by sub-chunk, in key order.
#[derive(Debug, Default)]
pub(in crate::chunk) struct TransparentLiquidResidents {
    entries: BTreeMap<SubChunkKey, TransparentLiquidResident>,
    /// Keys whose faces must be sorted.
    order_dependent: BTreeSet<SubChunkKey>,
    /// Bumped by every change.
    revision: u64,
    /// Bumped by changes that add, replace or remove order-dependent water.
    order_dependent_revision: u64,
}

impl TransparentLiquidResidents {
    /// Records `entity`'s newly uploaded `allocation` as the resident water for its key.
    pub(in crate::chunk) fn record(&mut self, entity: Entity, allocation: &GpuChunkAllocation) {
        let next = TransparentLiquidResident::from_allocation(entity, allocation);
        let previous = match &next {
            Some(resident) if self.entries.get(&allocation.key) == Some(resident) => return,
            Some(resident) => self.entries.insert(allocation.key, resident.clone()),
            None => match self.entries.remove(&allocation.key) {
                Some(previous) => Some(previous),
                None => return,
            },
        };
        self.changed(allocation.key, previous.as_ref(), next.as_ref());
    }

    /// Forgets `key`'s water once `entity`, its resident, has been removed.
    pub(in crate::chunk) fn forget(&mut self, entity: Entity, key: SubChunkKey) {
        if self
            .entries
            .get(&key)
            .is_none_or(|resident| resident.entity != entity)
        {
            return;
        }
        let previous = self.entries.remove(&key);
        self.changed(key, previous.as_ref(), None);
    }

    /// Rebuilds every entry from a whole arena, as when resource geometry replaces it.
    pub(in crate::chunk) fn rebuild<'a>(
        &mut self,
        allocations: impl IntoIterator<Item = (Entity, &'a GpuChunkAllocation)>,
    ) {
        self.entries.clear();
        self.order_dependent.clear();
        for (entity, allocation) in allocations {
            if let Some(resident) = TransparentLiquidResident::from_allocation(entity, allocation) {
                if !resident.order_independent {
                    self.order_dependent.insert(allocation.key);
                }
                self.entries.insert(allocation.key, resident);
            }
        }
        self.revision = self.revision.wrapping_add(1);
        self.order_dependent_revision = self.order_dependent_revision.wrapping_add(1);
    }

    /// Keeps the order-dependent key set and both revisions in step with one entry change.
    fn changed(
        &mut self,
        key: SubChunkKey,
        previous: Option<&TransparentLiquidResident>,
        next: Option<&TransparentLiquidResident>,
    ) {
        let dependent = |resident: Option<&TransparentLiquidResident>| {
            resident.is_some_and(|resident| !resident.order_independent)
        };
        if dependent(next) {
            self.order_dependent.insert(key);
        } else {
            self.order_dependent.remove(&key);
        }
        self.revision = self.revision.wrapping_add(1);
        if dependent(previous) || dependent(next) {
            self.order_dependent_revision = self.order_dependent_revision.wrapping_add(1);
        }
    }

    /// The resident water of `key`, when its newest upload has any.
    pub(in crate::chunk) fn get(&self, key: SubChunkKey) -> Option<&TransparentLiquidResident> {
        self.entries.get(&key)
    }

    /// Changes whenever the residents [`Self::sortable`] yields for `include_order_independent`
    /// may differ.
    pub(in crate::chunk) const fn revision(&self, include_order_independent: bool) -> u64 {
        if include_order_independent {
            self.revision
        } else {
            self.order_dependent_revision
        }
    }

    /// Residents that need sorting in key order, or every resident when views displace water.
    pub(in crate::chunk) fn sortable(
        &self,
        include_order_independent: bool,
    ) -> Box<dyn Iterator<Item = &TransparentLiquidResident> + '_> {
        if include_order_independent {
            Box::new(self.entries.values())
        } else {
            Box::new(
                self.order_dependent
                    .iter()
                    .filter_map(|key| self.entries.get(key)),
            )
        }
    }
}

/// The residents one snapshot sorts.
#[derive(Debug, Default, PartialEq, Eq)]
pub(in crate::chunk) struct TransparentResidentSelection {
    /// In key order.
    pub(in crate::chunk) residents: Vec<TransparentLiquidResident>,
    /// Residents the ref ceiling left out; they are drawn unsorted.
    pub(in crate::chunk) excluded: usize,
    /// Whether the selection depends on the camera's sub-chunk.
    pub(in crate::chunk) camera_dependent: bool,
}

/// Selects the residents to sort: all of them under `ceiling` refs, otherwise the nearest
/// to `camera_chunk` that fit, so distant water is the part left unsorted.
pub(in crate::chunk) fn select_sorted_residents<'a>(
    residents: impl IntoIterator<Item = &'a TransparentLiquidResident>,
    tint_identity: ChunkBiomeTintIdentity,
    camera_chunk: [i32; 3],
    ceiling: usize,
) -> TransparentResidentSelection {
    let mut selected = residents
        .into_iter()
        .filter(|resident| resident.tint_identity == tint_identity)
        .cloned()
        .collect::<Vec<_>>();
    if selected.iter().map(|resident| resident.refs).sum::<usize>() <= ceiling {
        return TransparentResidentSelection {
            residents: selected,
            excluded: 0,
            camera_dependent: false,
        };
    }
    selected.sort_by_cached_key(|resident| {
        (
            resident.chunk_distance_squared(camera_chunk),
            resident.identity.key,
        )
    });
    let mut refs = 0_usize;
    let admitted = selected
        .iter()
        .take_while(|resident| {
            refs = refs.saturating_add(resident.refs);
            refs <= ceiling
        })
        .count();
    let excluded = selected.len() - admitted;
    selected.truncate(admitted);
    selected.sort_unstable_by_key(|resident| resident.identity.key);
    TransparentResidentSelection {
        residents: selected,
        excluded,
        camera_dependent: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allocation(key: SubChunkKey, generation: u64, independent: bool) -> GpuChunkAllocation {
        let start = (key.x.unsigned_abs() + key.z.unsigned_abs() * 64) * 64;
        GpuChunkAllocation {
            cube_layout: CubeQuadLayout::default(),
            key,
            generation,
            tint_identity: ChunkBiomeTintIdentity::new(1, 1),
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(start..start + 16),
            liquid_lighting_range: Some(start + 16..start + 24),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_range: None,
            order_independent_liquid: independent,
            metadata_index: key.x.unsigned_abs(),
        }
    }

    #[test]
    fn order_independent_changes_leave_the_sorted_revision_alone() {
        let mut residents = TransparentLiquidResidents::default();
        let flat = SubChunkKey::new(0, 1, 0, 0);
        let shore = SubChunkKey::new(0, 2, 0, 0);
        residents.record(Entity::from_bits(1), &allocation(flat, 1, true));
        let sorted = residents.revision(false);
        residents.record(Entity::from_bits(1), &allocation(flat, 2, true));
        assert_eq!(residents.revision(false), sorted);
        residents.record(Entity::from_bits(2), &allocation(shore, 1, false));
        assert_ne!(residents.revision(false), sorted);
        assert_eq!(
            residents
                .sortable(false)
                .map(|resident| resident.identity.key)
                .collect::<Vec<_>>(),
            [shore]
        );
        assert_eq!(residents.sortable(true).count(), 2);

        // Re-recording the identical upload is not a change at all.
        let all = residents.revision(true);
        residents.record(Entity::from_bits(2), &allocation(shore, 1, false));
        assert_eq!(residents.revision(true), all);

        // A shore that settles flat leaves the sorted set.
        residents.record(Entity::from_bits(2), &allocation(shore, 2, true));
        assert_eq!(residents.sortable(false).count(), 0);
    }

    #[test]
    fn removal_forgets_only_the_current_resident_of_a_key() {
        let mut residents = TransparentLiquidResidents::default();
        let key = SubChunkKey::new(0, 1, 0, 0);
        residents.record(Entity::from_bits(1), &allocation(key, 1, false));
        // A reloaded chunk uploads under a new entity before the old one is removed.
        residents.record(Entity::from_bits(2), &allocation(key, 1, false));
        residents.forget(Entity::from_bits(1), key);
        assert_eq!(residents.get(key).unwrap().entity, Entity::from_bits(2));
        residents.forget(Entity::from_bits(2), key);
        assert!(residents.get(key).is_none());

        let mut dry = allocation(key, 2, false);
        dry.has_transparent_liquid = false;
        residents.record(Entity::from_bits(3), &allocation(key, 2, false));
        residents.record(Entity::from_bits(3), &dry);
        assert!(residents.get(key).is_none());
        assert_eq!(residents.sortable(false).count(), 0);
    }

    #[test]
    fn ceiling_admits_the_nearest_water_and_reports_the_rest() {
        let mut residents = TransparentLiquidResidents::default();
        for x in -3..=3 {
            let key = SubChunkKey::new(0, x, 0, 0);
            residents.record(
                Entity::from_bits((x + 20) as u64),
                &allocation(key, 1, false),
            );
        }
        let tint = ChunkBiomeTintIdentity::new(1, 1);
        let all = select_sorted_residents(residents.sortable(false), tint, [0, 0, 0], 28);
        assert_eq!((all.residents.len(), all.excluded), (7, 0));
        assert!(!all.camera_dependent);

        // Each resident has four faces, so a twelve-ref ceiling admits three sub-chunks.
        let near = select_sorted_residents(residents.sortable(false), tint, [2, 0, 0], 12);
        assert_eq!(
            near.residents
                .iter()
                .map(|resident| resident.identity.key.x)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        assert_eq!(near.excluded, 4);
        assert!(near.camera_dependent);

        let other_tint = ChunkBiomeTintIdentity::new(9, 9);
        assert!(
            select_sorted_residents(residents.sortable(false), other_tint, [0; 3], 12)
                .residents
                .is_empty()
        );
    }
}
