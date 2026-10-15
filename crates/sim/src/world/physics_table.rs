//! Runtime-ID keyed block physics with indexed lookups for sequential ID spaces.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use super::{Aabb, BlockPhysics, CollisionIdSpace};

/// Sequential runtime IDs below this bound are stored in a directly indexed slot.
/// Bedrock's sequential palettes stay far below it; larger IDs still fall back to the map.
const DENSE_RUNTIME_ID_LIMIT: u32 = 1 << 16;

/// Block physics keyed by runtime ID.
///
/// Sequential registries index their IDs directly, so the per-block lookup in collision
/// and ray scans is one bounds-checked load. Hashed IDs are spread over the whole `u32`
/// range and stay in the ordered map. Bit-identical collision shape lists are shared, so
/// the many states with the same geometry read one allocation.
#[derive(Debug, Clone)]
pub(super) struct PhysicsTable {
    dense: Vec<Option<BlockPhysics>>,
    sparse: BTreeMap<u32, BlockPhysics>,
    dense_limit: u32,
    shape_sets: BTreeSet<InternedShapes>,
}

/// One registered collision shape list, compared by the exact bits of its coordinates.
#[derive(Debug, Clone)]
struct InternedShapes(Arc<[Aabb]>);

impl InternedShapes {
    /// Every coordinate's bit pattern, shape by shape, minimum corner first.
    fn bits(&self) -> impl Iterator<Item = u64> + '_ {
        self.0.iter().flat_map(|shape| {
            [
                shape.min.x,
                shape.min.y,
                shape.min.z,
                shape.max.x,
                shape.max.y,
                shape.max.z,
            ]
            .map(f64::to_bits)
        })
    }
}

impl PartialEq for InternedShapes {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for InternedShapes {}

impl PartialOrd for InternedShapes {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for InternedShapes {
    /// Lexicographic over the coordinate bits, so only bit-identical lists are equal.
    fn cmp(&self, other: &Self) -> Ordering {
        self.bits().cmp(other.bits())
    }
}

impl PhysicsTable {
    /// An empty table laid out for `id_space`.
    pub(super) fn new(id_space: CollisionIdSpace) -> Self {
        Self {
            dense: Vec::new(),
            sparse: BTreeMap::new(),
            dense_limit: match id_space {
                CollisionIdSpace::Sequential => DENSE_RUNTIME_ID_LIMIT,
                CollisionIdSpace::Hashed => 0,
            },
            shape_sets: BTreeSet::new(),
        }
    }

    /// Shares one allocation among every registration of a bit-identical shape list.
    pub(super) fn intern_shapes(&mut self, shapes: Vec<Aabb>) -> Arc<[Aabb]> {
        let candidate = InternedShapes(shapes.into());
        if let Some(existing) = self.shape_sets.get(&candidate) {
            return Arc::clone(&existing.0);
        }
        self.shape_sets.insert(candidate.clone());
        candidate.0
    }

    pub(super) fn get(&self, runtime_id: u32) -> Option<&BlockPhysics> {
        if runtime_id < self.dense_limit {
            self.dense.get(runtime_id as usize)?.as_ref()
        } else {
            self.sparse.get(&runtime_id)
        }
    }

    pub(super) fn get_mut(&mut self, runtime_id: u32) -> Option<&mut BlockPhysics> {
        if runtime_id < self.dense_limit {
            self.dense.get_mut(runtime_id as usize)?.as_mut()
        } else {
            self.sparse.get_mut(&runtime_id)
        }
    }

    pub(super) fn contains(&self, runtime_id: u32) -> bool {
        self.get(runtime_id).is_some()
    }

    /// Stores `physics`, replacing any earlier registration of the same ID.
    pub(super) fn insert(&mut self, runtime_id: u32, physics: BlockPhysics) {
        if runtime_id < self.dense_limit {
            let index = runtime_id as usize;
            if index >= self.dense.len() {
                self.dense.resize_with(index + 1, || None);
            }
            self.dense[index] = Some(physics);
        } else {
            self.sparse.insert(runtime_id, physics);
        }
    }

    /// Drops one registration; returns whether it existed.
    pub(super) fn remove(&mut self, runtime_id: u32) -> bool {
        if runtime_id < self.dense_limit {
            self.dense
                .get_mut(runtime_id as usize)
                .and_then(Option::take)
                .is_some()
        } else {
            self.sparse.remove(&runtime_id).is_some()
        }
    }

    /// Drops every registration at or above `first_runtime_id`.
    pub(super) fn remove_from(&mut self, first_runtime_id: u32) {
        if first_runtime_id < self.dense_limit {
            self.dense.truncate(first_runtime_id as usize);
        }
        self.sparse.split_off(&first_runtime_id);
    }
}

#[cfg(test)]
mod tests {
    use super::super::{CollisionRegistry, CollisionRegistryIdentity, Vec3};
    use super::*;

    fn registry(id_space: CollisionIdSpace) -> CollisionRegistry {
        CollisionRegistry::with_identity(CollisionRegistryIdentity {
            protocol: 1001,
            id_space,
            preg_sha256: [0; 32],
        })
    }

    fn cube(top: f64) -> Aabb {
        Aabb::new(Vec3::ZERO, Vec3::new(1.0, top, 1.0))
    }

    #[test]
    fn membership_matches_an_ordered_map_in_both_id_spaces() {
        for id_space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
            let mut registry = registry(id_space);
            let ids = [
                0,
                7,
                DENSE_RUNTIME_ID_LIMIT - 1,
                DENSE_RUNTIME_ID_LIMIT,
                u32::MAX,
            ];
            for id in ids {
                registry.register(id, [cube(1.0)]).unwrap();
            }
            assert!(ids.iter().all(|&id| registry.contains_runtime_id(id)));
            assert!(!registry.contains_runtime_id(3));
            assert!(registry.remove_runtime_id(7));
            assert!(!registry.remove_runtime_id(7));
            assert!(!registry.contains_runtime_id(7));
            registry.remove_runtime_ids_from(DENSE_RUNTIME_ID_LIMIT - 1);
            assert!(registry.contains_runtime_id(0));
            assert!(!registry.contains_runtime_id(DENSE_RUNTIME_ID_LIMIT - 1));
            assert!(!registry.contains_runtime_id(DENSE_RUNTIME_ID_LIMIT));
            assert!(!registry.contains_runtime_id(u32::MAX));
            registry.register(u32::MAX, [cube(1.0)]).unwrap();
            registry.remove_runtime_ids_from(u32::MAX);
            assert!(registry.contains_runtime_id(0));
            assert!(!registry.contains_runtime_id(u32::MAX));
        }
    }

    #[test]
    fn only_bit_identical_shape_lists_share_storage() {
        let mut registry = registry(CollisionIdSpace::Sequential);
        registry.register(1, [cube(1.0)]).unwrap();
        registry.register(2, [cube(1.0)]).unwrap();
        registry.register(3, [cube(0.5)]).unwrap();
        registry.register(4, [cube(1.0), cube(0.5)]).unwrap();
        let shapes = |id| registry.collision_shapes(id).unwrap();
        assert!(std::ptr::eq(shapes(1), shapes(2)));
        assert!(!std::ptr::eq(shapes(1), shapes(3)));
        assert_eq!(shapes(3), &[cube(0.5)]);
        assert_eq!(shapes(4), &[cube(1.0), cube(0.5)]);
    }
}
