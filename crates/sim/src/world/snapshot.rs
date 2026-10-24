use std::sync::Arc;

use super::*;

/// An immutable collision world retained for one prediction frame.
#[derive(Debug, Clone)]
pub struct CollisionSnapshot(Arc<SnapshotData>);

#[derive(Debug)]
struct SnapshotData {
    store: Arc<ChunkStore>,
    registry: CollisionRegistry,
    dimension: i32,
    unloaded_collision_barriers: bool,
    actor_colliders: Option<Arc<[Aabb]>>,
}

impl PartialEq for CollisionSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl CollisionSnapshot {
    /// Keeps the original palette pages alive after updates or chunk unloads.
    pub(super) fn capture(world: &PaletteWorld<'_>) -> Self {
        Self(Arc::new(SnapshotData {
            store: world.store.collision_snapshot(),
            registry: world.registry.clone(),
            dimension: world.dimension,
            unloaded_collision_barriers: world.unloaded_collision_barriers,
            actor_colliders: world.actor_colliders.clone(),
        }))
    }

    /// Borrows the ordinary palette adapter so replay uses the live query implementation.
    fn world(&self) -> PaletteWorld<'_> {
        let mut world = PaletteWorld::new(
            &self.0.store,
            &self.0.registry,
            self.0.dimension,
            Default::default(),
        );
        world.unloaded_collision_barriers = self.0.unloaded_collision_barriers;
        world.actor_colliders = self.0.actor_colliders.clone();
        world
    }
}

impl CollisionWorld for CollisionSnapshot {
    /// Checks the origin against the terrain retained by the prediction frame.
    fn ensure_movement_origin(&self, feet: Vec3) -> Result<(), WorldQueryError> {
        self.world().ensure_movement_origin(feet)
    }

    fn registry_identity(&self) -> CollisionRegistryIdentity {
        self.0.registry.identity()
    }

    fn liquid_current(
        &self,
        previous_pose: Aabb,
    ) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
        self.world().liquid_current(previous_pose)
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        self.world().collision_boxes(query)
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        self.world().collision_boxes_with_provenance(query)
    }

    fn collision_boxes_for_mover(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<MoverColliders>, WorldQueryError> {
        self.world().collision_boxes_for_mover(query)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.world().block_physics(block)
    }

    /// Uses the retained palette pages without constructing material-sample identities.
    fn visit_block_materials(
        &self,
        block: [i32; 3],
        visitor: &mut dyn FnMut(BlockPhysicsFacts),
    ) -> Result<(), WorldQueryError> {
        self.world().visit_block_materials(block, visitor)
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        self.world().primary_is_air(block)
    }

    fn snapshot(&self) -> Option<CollisionSnapshot> {
        Some(self.clone())
    }
}
