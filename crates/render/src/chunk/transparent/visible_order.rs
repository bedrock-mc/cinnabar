//! Retains visible water in deterministic key order, merging only newly visible keys each frame.
//! Equal phase distances preserve insertion order, so the unordered Bevy list cannot choose draw order.
use crate::chunk::*;
use bevy::ecs::entity::EntityHashMap;

/// One visible sub-chunk with transparent water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::chunk) struct VisibleWater {
    pub(in crate::chunk) key: SubChunkKey,
    pub(in crate::chunk) entity: Entity,
    pub(in crate::chunk) main: MainEntity,
    pub(in crate::chunk) order_independent: bool,
}

/// Visible water ordered by key, then entity, updated by each frame's changes.
#[derive(Default)]
pub(in crate::chunk) struct VisibleWaterOrder {
    ordered: Vec<VisibleWater>,
    current: EntityHashMap<VisibleWater>,
    added: Vec<VisibleWater>,
    merged: Vec<VisibleWater>,
}

impl VisibleWaterOrder {
    /// Orders this frame's `visible` water, sorting only entries new since the last frame.
    pub(in crate::chunk) fn update(
        &mut self,
        visible: impl IntoIterator<Item = VisibleWater>,
    ) -> &[VisibleWater] {
        let Self {
            ordered,
            current,
            added,
            merged,
        } = self;
        current.clear();
        current.extend(visible.into_iter().map(|water| (water.entity, water)));
        // Last frame's entries keep their place while visible, with this frame's data.
        ordered.retain_mut(|water| match current.remove(&water.entity) {
            Some(now) if now.key == water.key => {
                *water = now;
                true
            }
            Some(now) => {
                added.push(now);
                false
            }
            None => false,
        });
        added.extend(current.drain().map(|(_, water)| water));
        if !added.is_empty() {
            added.sort_unstable_by_key(|water| (water.key, water.entity));
            merged.clear();
            merged.reserve(ordered.len() + added.len());
            let (mut old, mut new) = (ordered.iter().peekable(), added.iter().peekable());
            while let (Some(left), Some(right)) = (old.peek(), new.peek()) {
                if (left.key, left.entity) <= (right.key, right.entity) {
                    merged.push(**left);
                    old.next();
                } else {
                    merged.push(**right);
                    new.next();
                }
            }
            merged.extend(old.copied());
            merged.extend(new.copied());
            added.clear();
            std::mem::swap(ordered, merged);
        }
        ordered
    }
}

/// Visits `ordered` water once per key, choosing among duplicates the entity that
/// `residents` holds as the key's current upload, else the first.
pub(in crate::chunk) fn each_visible_key(
    ordered: &[VisibleWater],
    residents: &TransparentLiquidResidents,
) -> impl Iterator<Item = VisibleWater> {
    ordered
        .chunk_by(|left, right| left.key == right.key)
        .map(move |same_key| match same_key {
            [only] => *only,
            duplicates => {
                let resident = residents
                    .get(duplicates[0].key)
                    .map(|resident| resident.entity);
                *duplicates
                    .iter()
                    .find(|water| Some(water.entity) == resident)
                    .unwrap_or(&duplicates[0])
            }
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a visible water fixture, alternating its order dependence by key.
    fn water(x: i32, entity: u64) -> VisibleWater {
        VisibleWater {
            key: SubChunkKey::new(0, x, 0, 0),
            entity: Entity::from_bits(entity),
            main: MainEntity::from(Entity::from_bits(entity)),
            order_independent: x % 2 == 0,
        }
    }

    /// Produces the full-sort reference order for retained-order comparisons.
    fn sorted(mut water: Vec<VisibleWater>) -> Vec<VisibleWater> {
        water.sort_unstable_by_key(|water| (water.key, water.entity));
        water
    }

    #[test]
    fn churning_visible_sets_in_any_order_match_a_full_sort() {
        let mut order = VisibleWaterOrder::default();
        let mut seed = 0x1234_5678_u32;
        let mut next = move |bound: u32| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            seed % bound
        };
        for _ in 0..200 {
            // Each frame keeps most entries and arrives shuffled, as Bevy's list does.
            let mut visible = (0..64)
                .filter(|_| next(8) != 0)
                .map(|x| water(x, u64::from(x as u32) + 1))
                .collect::<Vec<_>>();
            for index in (1..visible.len()).rev() {
                visible.swap(index, next(index as u32 + 1) as usize);
            }
            assert_eq!(order.update(visible.clone()), sorted(visible));
        }
    }

    #[test]
    fn retained_water_takes_this_frames_data() {
        let mut order = VisibleWaterOrder::default();
        order.update([water(1, 1), water(2, 2)]);
        let mut remeshed = water(1, 1);
        remeshed.order_independent = !remeshed.order_independent;
        assert_eq!(
            order.update([water(2, 2), remeshed]),
            [remeshed, water(2, 2)]
        );
    }

    #[test]
    fn duplicate_keys_draw_the_indexed_resident() {
        let key = SubChunkKey::new(0, 1, 0, 0);
        let (older, newer) = (water(1, 1), water(1, 7));
        let mut residents = TransparentLiquidResidents::default();
        let upload = GpuChunkAllocation {
            cube_layout: CubeQuadLayout::default(),
            key,
            generation: 2,
            tint_identity: ChunkBiomeTintIdentity::default(),
            quad_range: 0..0,
            cube_lighting_range: None,
            model_range: None,
            model_lighting_range: None,
            model_draw_range: None,
            transparent_model_draw_range: None,
            liquid_range: Some(0..16),
            liquid_lighting_range: Some(16..24),
            has_depth_liquid: false,
            has_transparent_liquid: true,
            depth_liquid_range: None,
            order_independent_liquid: true,
            metadata_index: 1,
        };
        residents.record(newer.entity, &upload);
        let mut order = VisibleWaterOrder::default();
        let ordered = order.update([newer, older, water(3, 3)]).to_vec();
        assert_eq!(
            each_visible_key(&ordered, &residents).collect::<Vec<_>>(),
            [newer, water(3, 3)]
        );
    }
}
