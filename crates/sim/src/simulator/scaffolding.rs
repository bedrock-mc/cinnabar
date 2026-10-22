use crate::{
    Aabb, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    ProvenancedCollider, WorldQueryError,
};

use super::environment::SampledEnvironment;

/// How far below a scaffolding top the feet may sit and still stand on it.
const TOP_TOLERANCE: f32 = 1.0e-6;
/// Vertical speed of both the held-jump ascent and the descent through a climbable block.
pub(super) const CLIMB_SPEED: f64 = 0.15_f32 as f64;

/// Block-climber flags of the body footprint, rewritten every tick from the
/// layer at the feet and the layer below it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ClimberContact {
    /// Scaffolding at the feet layer (`InScaffolding`).
    pub inside: bool,
    /// Scaffolding at the layer below the feet (`OverScaffolding`).
    pub over: bool,
    /// A climbable block at the feet layer (`InAscendingBlock`).
    pub ascending: bool,
    /// A climbable block at the layer below the feet (`OverDescendingBlock`),
    /// which a descent request sinks through.
    pub over_descending: bool,
}

/// Scaffolding and climbable-block facts of one footprint layer.
#[derive(Debug, Clone, Copy, Default)]
struct LayerContact {
    scaffolding: bool,
    climbable: bool,
}

/// Feet layer and the layer below it, from the box's float feet height.
fn layers(player: Aabb) -> (i32, i32) {
    let feet = player.min.y as f32;
    (feet.floor() as i32, (feet - 1.0).floor() as i32)
}

/// Scans the footprint at the feet layer and the layer below it, as the block
/// climber does every tick. A climbable block is scaffolding that rests on a
/// block other than air or water, or powder snow for a wearer who can stand on
/// it. Fresh reads merge into the tick identity.
pub(super) fn sample_climber(
    world: &(impl CollisionWorld + ?Sized),
    player: Aabb,
    can_stand_on_snow: bool,
    sampled: &mut SampledEnvironment,
) -> Result<ClimberContact, WorldQueryError> {
    let (feet, below) = layers(player);
    let at_feet = scan_layer(world, player, feet, Some(can_stand_on_snow), sampled)?;
    let under = scan_layer(world, player, below, Some(can_stand_on_snow), sampled)?;
    Ok(ClimberContact {
        inside: at_feet.scaffolding,
        over: under.scaffolding,
        ascending: at_feet.climbable,
        over_descending: under.climbable,
    })
}

/// Whether scaffolding occupies the feet layer, without the climbable-block
/// reads that only the movement tick needs.
pub(super) fn sample_feet_scaffolding(
    world: &(impl CollisionWorld + ?Sized),
    player: Aabb,
    sampled: &mut SampledEnvironment,
) -> Result<bool, WorldQueryError> {
    let (feet, _) = layers(player);
    Ok(scan_layer(world, player, feet, None, sampled)?.scaffolding)
}

/// Scans every footprint column of one layer until both facts are known.
/// `climber` carries the snow-standing capability when climbable blocks are
/// wanted; scaffolding support is read only for scaffold cells.
fn scan_layer(
    world: &(impl CollisionWorld + ?Sized),
    player: Aabb,
    y: i32,
    climber: Option<bool>,
    sampled: &mut SampledEnvironment,
) -> Result<LayerContact, WorldQueryError> {
    let floor = |value: f64| (value as f32).floor() as i32;
    let mut contact = LayerContact::default();
    for x in floor(player.min.x)..=floor(player.max.x) {
        for z in floor(player.min.z)..=floor(player.max.z) {
            let flags = primary(world, sampled, [x, y, z])?.flags;
            let scaffolding = flags.contains(BlockPhysicsFlags::SCAFFOLDING);
            contact.scaffolding |= scaffolding;
            if let Some(can_stand_on_snow) = climber
                && !contact.climbable
            {
                contact.climbable = (can_stand_on_snow
                    && flags.contains(BlockPhysicsFlags::POWDER_SNOW))
                    || (scaffolding && rests_on_support(world, sampled, [x, y - 1, z])?);
            }
            if contact.scaffolding && (contact.climbable || climber.is_none()) {
                return Ok(contact);
            }
        }
    }
    Ok(contact)
}

/// Reads a primary block through this tick's cache, merging a fresh read's identity.
pub(super) fn primary(
    world: &(impl CollisionWorld + ?Sized),
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
    world: &(impl CollisionWorld + ?Sized),
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
    fn registry_identity(&self) -> crate::CollisionRegistryIdentity {
        self.inner.registry_identity()
    }

    /// Preserves the underlying terrain availability requirement for movement origins.
    fn ensure_movement_origin(&self, feet: crate::Vec3) -> Result<(), WorldQueryError> {
        self.inner.ensure_movement_origin(feet)
    }

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
                let has_scaffolding = match collider.cell_has_scaffolding {
                    Some(value) => value,
                    None => {
                        let sample = self.inner.block_physics(block)?;
                        identity = identity.merge(&sample.identity)?;
                        sample
                            .layers
                            .iter()
                            .any(|facts| facts.flags.contains(BlockPhysicsFlags::SCAFFOLDING))
                    }
                };
                if has_scaffolding {
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
#[path = "scaffolding_tests.rs"]
mod tests;

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
                    cell_has_scaffolding: None,
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
