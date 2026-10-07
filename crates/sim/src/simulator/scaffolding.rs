use crate::{
    Aabb, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    ProvenancedCollider, WorldQueryError,
};

/// How far below a scaffolding top the feet may sit and still stand on it.
const TOP_TOLERANCE: f32 = 1.0e-6;

/// Scaffolding is solid only under the feet of a player who is not descending.
pub(super) struct ScaffoldingView<'a, W> {
    inner: &'a W,
    player: Aabb,
    descending: bool,
}

impl<'a, W: CollisionWorld> ScaffoldingView<'a, W> {
    /// Retains the pre-move box used by the native contextual collision query.
    pub(super) const fn new(inner: &'a W, player: Aabb, descending: bool) -> Self {
        Self {
            inner,
            player,
            descending,
        }
    }
}

impl<W: CollisionWorld> CollisionWorld for ScaffoldingView<'_, W> {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let colliders = self.collision_boxes_with_provenance(query)?;
        Ok(CollisionQuery {
            value: colliders
                .value
                .into_iter()
                .map(|collider| collider.aabb)
                .collect(),
            identity: colliders.identity,
        })
    }

    /// Keeps source cells attached to the contextual collision shapes.
    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        let colliders = self.inner.collision_boxes_with_provenance(query)?;
        let mut kept = Vec::with_capacity(colliders.value.len());
        let mut identity = colliders.identity;
        for collider in colliders.value {
            let bounds = collider.aabb;
            let solid_top = !self.descending
                && self.player.min.y as f32 >= bounds.max.y as f32 - TOP_TOLERANCE
                && self.player.max.x > bounds.min.x
                && self.player.min.x < bounds.max.x
                && self.player.max.z > bounds.min.z
                && self.player.min.z < bounds.max.z;
            if !solid_top && let Some(block) = collider.block {
                let sample = self.inner.block_physics(block)?;
                identity = identity.merge(&sample.identity)?;
                if sample
                    .layers
                    .iter()
                    .any(|facts| facts.flags.contains(BlockPhysicsFlags::SCAFFOLDING))
                {
                    continue;
                }
            }
            kept.push(collider);
        }
        Ok(CollisionQuery {
            value: kept,
            identity,
        })
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        self.inner.block_physics(block)
    }

    fn primary_is_air(
        &self,
        block: [i32; 3],
    ) -> Result<Option<CollisionQuery<bool>>, WorldQueryError> {
        self.inner.primary_is_air(block)
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    struct Conflicting;
    impl CollisionWorld for Conflicting {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(vec![]))
        }
        fn collision_boxes_with_provenance(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<crate::ProvenancedCollider>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(vec![
                crate::ProvenancedCollider {
                    aabb: Aabb::new(crate::Vec3::ZERO, crate::Vec3::ONE),
                    block: Some([0; 3]),
                    runtime_id: None,
                },
            ]))
        }
        fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            let mut identity = CollisionQuery::synthetic(()).identity;
            identity.registry.preg_sha256 = [1; 32];
            Ok(BlockPhysicsSample {
                identity,
                layers: Box::new([crate::BlockPhysicsFacts {
                    friction: 0.6,
                    horizontal_speed_factor: 1.0,
                    vertical_speed_factor: 1.0,
                    fluid_height_blocks: 0.0,
                    flags: BlockPhysicsFlags::SCAFFOLDING,
                    surface_response: crate::SurfaceResponse::None,
                }]),
            })
        }
    }
    #[test]
    fn review_scaffolding_classification_must_share_collision_identity() {
        let player = Aabb::player_at(crate::Vec3::new(0.5, 0.0, 0.5));
        let view = ScaffoldingView::new(&Conflicting, player, true);
        assert!(matches!(
            view.collision_boxes(player),
            Err(WorldQueryError::RegistryIdentityMismatch)
        ));
    }
}
