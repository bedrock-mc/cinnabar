use crate::{
    Aabb, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    ProvenancedCollider, WorldQueryError,
};

use super::environment::SampledEnvironment;

/// How far below a scaffolding top the feet may sit and still stand on it.
const TOP_TOLERANCE: f32 = 1.0e-6;
/// Vertical speed of both the held-jump ascent and the sneak descent.
pub(super) const CLIMB_SPEED: f64 = 0.15;

/// Scaffolding facts of the body footprint at the feet layer and the layer below it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ScaffoldingContact {
    /// Scaffolding at the feet layer.
    pub inside: bool,
    /// Scaffolding at the layer below the feet.
    pub over: bool,
    /// Feet-layer scaffolding resting on a block other than air or water.
    pub ascending: bool,
    /// Lower-layer scaffolding resting on a block other than air or water.
    pub over_descending: bool,
}

/// Scans every footprint column at the feet layer and, while sneaking (the only
/// time it matters), the layer below. Fresh reads merge into the tick identity.
pub(super) fn sample_contact(
    world: &impl CollisionWorld,
    player: Aabb,
    sneaking: bool,
    sampled: &mut SampledEnvironment,
) -> Result<ScaffoldingContact, WorldQueryError> {
    let floor = |value: f64| (value as f32).floor() as i32;
    let feet = floor(player.min.y);
    let below = ((player.min.y as f32) - 1.0).floor() as i32;
    let mut contact = ScaffoldingContact::default();
    for x in floor(player.min.x)..=floor(player.max.x) {
        for z in floor(player.min.z)..=floor(player.max.z) {
            for (y, layer_over) in [(feet, false), (below, true)] {
                if layer_over && !sneaking {
                    continue;
                }
                if !primary(world, sampled, [x, y, z])?
                    .flags
                    .contains(BlockPhysicsFlags::SCAFFOLDING)
                {
                    continue;
                }
                let supported = rests_on_support(world, sampled, [x, y - 1, z])?;
                if layer_over {
                    contact.over = true;
                    contact.over_descending |= supported;
                } else {
                    contact.inside = true;
                    contact.ascending |= supported;
                }
            }
        }
    }
    Ok(contact)
}

fn primary(
    world: &impl CollisionWorld,
    sampled: &mut SampledEnvironment,
    block: [i32; 3],
) -> Result<crate::BlockPhysicsFacts, WorldQueryError> {
    let (facts, fresh) = sampled.primary(world, block)?;
    if let Some(fresh) = fresh {
        sampled.identity = sampled.identity.merge(&fresh)?;
    }
    Ok(facts)
}

/// Whether the block under a scaffold is anything but air or (flowing) water.
fn rests_on_support(
    world: &impl CollisionWorld,
    sampled: &mut SampledEnvironment,
    block: [i32; 3],
) -> Result<bool, WorldQueryError> {
    let facts = primary(world, sampled, block)?;
    let water = facts.flags.contains(BlockPhysicsFlags::WATER)
        && !matches!(
            facts.surface_response,
            crate::SurfaceResponse::BubbleUp | crate::SurfaceResponse::BubbleDown
        );
    let air = match world.primary_is_air(block)? {
        Some(air) => {
            sampled.identity = sampled.identity.merge(&air.identity)?;
            air.value
        }
        // Without material identity, air is the bare passable fact set.
        None => facts.flags == BlockPhysicsFlags::PASSABLE,
    };
    Ok(!air && !water)
}

/// Scaffolding is solid only under the feet of a player not descending through it.
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
