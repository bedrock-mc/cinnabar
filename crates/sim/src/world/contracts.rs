//! Collision query contract shared by live palettes and synthetic worlds.

use super::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery,
    CollisionRegistryIdentity, CollisionSnapshot, DEFAULT_SURFACE_FRICTION, LenientCollisionBoxes,
    LenientSkipCounts, ProvenancedCollider, SurfaceResponse, Vec3, WorldQueryError,
};

pub trait CollisionWorld {
    /// Registry provenance for ticks that read no terrain. Bare shape adapters remain neutral.
    fn registry_identity(&self) -> CollisionRegistryIdentity {
        CollisionQuery::synthetic(()).identity.registry
    }

    /// Native liquid velocity impulse for the preceding collision pose. Worlds
    /// without liquid-state authority may omit it; palette snapshots retain the
    /// same neighbor sampling and immutable identity as live prediction.
    fn liquid_current(&self, _aabb: Aabb) -> Result<Option<CollisionQuery<Vec3>>, WorldQueryError> {
        Ok(None)
    }

    /// Primary air material, when the adapter retains palette material identity.
    fn primary_is_air(
        &self,
        _block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        Ok(None)
    }

    /// Retains the world read by this tick when the adapter supports historical replay.
    fn snapshot(&self) -> Option<CollisionSnapshot> {
        None
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError>;

    /// Camera-only lenient companion to [`Self::collision_boxes`]: a cell with an
    /// unregistered runtime id or an unloaded chunk is skipped and tallied, not
    /// fatal, so a third-person boom still stops at known solids instead of
    /// collapsing near unknown blocks or chunk edges. Framing/bounds errors stay
    /// fatal; the strict authority contract of [`Self::collision_boxes`] is
    /// untouched. The default gives whole-region leniency for bare-AABB worlds;
    /// the palette adapter overrides it to skip per cell.
    fn collision_boxes_camera_lenient(
        &self,
        query: Aabb,
    ) -> Result<LenientCollisionBoxes, WorldQueryError> {
        match self.collision_boxes(query) {
            Ok(boxes) => Ok(LenientCollisionBoxes {
                value: boxes.value,
                skipped: LenientSkipCounts::default(),
            }),
            Err(WorldQueryError::UnknownRuntimeId { .. }) => Ok(LenientCollisionBoxes {
                skipped: LenientSkipCounts {
                    unknown_runtime_id: 1,
                    unloaded_chunk: 0,
                },
                ..Default::default()
            }),
            Err(WorldQueryError::UnloadedChunk(_)) => Ok(LenientCollisionBoxes {
                skipped: LenientSkipCounts {
                    unknown_runtime_id: 0,
                    unloaded_chunk: 1,
                },
                ..Default::default()
            }),
            Err(other) => Err(other),
        }
    }

    /// Visits camera colliders in scan order; live palettes borrow shapes without allocating.
    fn visit_collision_boxes_camera_lenient(
        &self,
        query: Aabb,
        visitor: &mut dyn FnMut(Aabb),
    ) -> Result<LenientSkipCounts, WorldQueryError> {
        let boxes = self.collision_boxes_camera_lenient(query)?;
        for shape in boxes.value {
            visitor(shape);
        }
        Ok(boxes.skipped)
    }

    /// Earliest fraction of the closed segment `origin..origin + delta` inside a camera collider,
    /// with the same leniency as [`Self::visit_collision_boxes_camera_lenient`].
    /// Live palettes walk only the cells along the segment, so cost is linear in its length.
    fn camera_segment_entry(
        &self,
        origin: Vec3,
        delta: Vec3,
    ) -> Result<(Option<f64>, LenientSkipCounts), WorldQueryError> {
        let end = origin + delta;
        let mut entry: Option<f64> = None;
        let skipped = self.visit_collision_boxes_camera_lenient(
            Aabb::new(origin.component_min(end), origin.component_max(end)),
            &mut |shape| {
                if let Some(hit) = shape.segment_entry(origin, delta) {
                    entry = Some(entry.map_or(hit, |best| best.min(hit)));
                }
            },
        )?;
        Ok((entry, skipped))
    }

    /// Per-collider provenance companion to [`Self::collision_boxes`].
    ///
    /// The default derives every entry from [`Self::collision_boxes`] with
    /// both provenance facts absent; adapters that can attribute each emitted
    /// instance to its source palette cell override this method. Bounds,
    /// ordering, identity, and the error contract stay exactly those of the
    /// box-only surface either way.
    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        let boxes = self.collision_boxes(query)?;
        Ok(CollisionQuery {
            identity: boxes.identity,
            value: boxes
                .value
                .into_iter()
                .map(|aabb| ProvenancedCollider {
                    aabb,
                    block: None,
                    runtime_id: None,
                })
                .collect(),
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let origin = Vec3::new(
            f64::from(block[0]),
            f64::from(block[1]),
            f64::from(block[2]),
        );
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: DEFAULT_SURFACE_FRICTION,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::default(),
                surface_response: SurfaceResponse::None,
            }]),
            identity: self
                .collision_boxes(Aabb::new(origin, origin + Vec3::ONE))?
                .identity,
        })
    }
}
