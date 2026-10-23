use crate::{
    Aabb, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    ProvenancedCollider, WorldQueryError,
};

use serde::{Deserialize, Serialize};

use super::{
    MovementEnvironment, MovementInput, MovementMode, PlayerState, TickResult,
    environment::SampledEnvironment,
};

/// How far below a scaffolding top the feet may sit and still stand on it.
const TOP_TOLERANCE: f32 = 1.0e-6;
/// Vertical speed of both the held-jump ascent and the descent through a climbable block.
pub(super) const CLIMB_SPEED: f64 = 0.15_f32 as f64;

/// Block-climber flags of the body footprint, measured after every tick's
/// move from the layer at the feet and the layer below it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
    /// The first climbable block found below the feet is powder snow, which
    /// keeps the fall distance the climber otherwise clears.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub over_descending_snow: bool,
}

impl ClimberContact {
    /// Whether the descent action sinks through this contact's descending block.
    pub(super) fn descends(self, input: &MovementInput) -> bool {
        descends_through_block(input, self.over_descending)
    }

    /// Whether any of the four contact flags is set.
    fn any(self) -> bool {
        self.inside || self.over || self.ascending || self.over_descending
    }

    /// Publishes the four contact flags as measured.
    fn publish(self, environment: &mut MovementEnvironment) {
        environment.in_scaffolding = self.inside;
        environment.over_scaffolding = self.over;
        environment.in_ascending_block = self.ascending;
        environment.over_descending_block = self.over_descending;
        environment.climber_unsampled = false;
    }
}

/// Vanilla's descent action: the input mode's descent request over a
/// climbable block (`OverDescendingBlock`) sinks through it, except while
/// riding. Inputs without a captured request use the sneak state.
#[must_use]
pub fn descends_through_block(input: &MovementInput, over_descending_block: bool) -> bool {
    input.mode != MovementMode::Riding
        && input.descent_request.unwrap_or(input.sneaking)
        && over_descending_block
}

/// Runs the block climber after the tick's move, as vanilla does, and retains
/// its contact for the next tick's descent action, climb jump and gravity.
/// `start` is the contact the tick began with; an immobile tick's descent
/// action reads it. Cells without usable facts count as neither scaffolding
/// nor support. As an untraced recovery policy, unavailable origins and failed
/// scans publish the retained contact; a spectator, or such a tick without
/// retained contact, is unsampled. Identity conflicts still reject the tick,
/// including conflicts discovered after movement.
pub(super) fn finish_tick(
    world: &(impl CollisionWorld + ?Sized),
    state: &mut PlayerState,
    input: &MovementInput,
    start: Option<ClimberContact>,
    result: &mut TickResult,
) -> Result<(), WorldQueryError> {
    result.environment.climber_unsampled = true;
    if input.spectator {
        state.climber = None;
        return Ok(());
    }
    // The climber reads the resolved collision faces, not a box rebuilt
    // around the rounded feet.
    let measured = measure(
        world,
        state.position,
        state.collision_box(state.hitbox_height(*input)),
        input,
        &mut result.world_identity,
    )?;
    if input.immobile {
        result.environment.descending_through_block = start
            .or(measured)
            .is_some_and(|contact| contact.descends(input));
    }
    if let Some(contact) = measured {
        state.climber = Some(contact);
    }
    if let Some(contact) = state.climber {
        contact.publish(&mut result.environment);
        // Climbing contact clears the fall distance, except over powder snow.
        if contact.any() && !contact.over_descending_snow {
            state.fall_distance = 0.0;
        }
    }
    Ok(())
}

/// Measures climber contact at `feet`, merging the reads into `identity`.
/// Unreadable terrain leaves contact unmeasured; identity conflicts propagate.
fn measure(
    world: &(impl CollisionWorld + ?Sized),
    feet: crate::Vec3,
    player: Aabb,
    input: &MovementInput,
    identity: &mut crate::WorldCollisionIdentity,
) -> Result<Option<ClimberContact>, WorldQueryError> {
    let mut sampled = SampledEnvironment::unsampled(identity.clone());
    let measured = world
        .ensure_movement_origin(feet)
        .and_then(|()| sample_climber(world, player, input.can_stand_on_snow, &mut sampled));
    match measured {
        Ok(contact) => {
            *identity = sampled.identity;
            Ok(Some(contact))
        }
        Err(
            error @ (WorldQueryError::RegistryIdentityMismatch
            | WorldQueryError::ChunkRevisionConflict { .. }),
        ) => Err(error),
        Err(_) => Ok(None),
    }
}

/// Scaffolding and climbable-block facts of one footprint layer.
#[derive(Debug, Clone, Copy, Default)]
struct LayerContact {
    scaffolding: bool,
    climbable: bool,
    /// The first climbable block found is powder snow.
    snow: bool,
}

/// Feet layer and the layer below it, from the box's float feet height.
fn layers(player: Aabb) -> (i32, i32) {
    let feet = player.min.y as f32;
    (feet.floor() as i32, (feet - 1.0).floor() as i32)
}

/// Scans the footprint at the feet layer and the layer below it, as the block
/// climber does every tick. A climbable block is scaffolding that rests on a
/// block other than air or water, or powder snow for a wearer who can stand on
/// it. A cell without usable facts counts as neither; fresh reads merge into
/// the tick identity, and identity conflicts fail the scan.
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
        over_descending_snow: under.snow,
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
            let Some(facts) = readable(primary(world, sampled, [x, y, z]))? else {
                continue;
            };
            let flags = facts.flags;
            let scaffolding = flags.contains(BlockPhysicsFlags::SCAFFOLDING);
            contact.scaffolding |= scaffolding;
            if let Some(can_stand_on_snow) = climber
                && !contact.climbable
            {
                contact.snow = can_stand_on_snow && flags.contains(BlockPhysicsFlags::POWDER_SNOW);
                contact.climbable = contact.snow
                    || (scaffolding
                        && readable(rests_on_support(world, sampled, [x, y - 1, z]))?
                            .unwrap_or(false));
            }
            if contact.scaffolding && (contact.climbable || climber.is_none()) {
                return Ok(contact);
            }
        }
    }
    Ok(contact)
}

/// Treats a cell without usable facts (an unknown runtime ID or an unloaded
/// chunk) as absent; identity conflicts and other failures still propagate.
fn readable<T>(result: Result<T, WorldQueryError>) -> Result<Option<T>, WorldQueryError> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(WorldQueryError::UnknownRuntimeId { .. } | WorldQueryError::UnloadedChunk(_)) => {
            Ok(None)
        }
        Err(error) => Err(error),
    }
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

/// A mover's contextual collision: scaffolding is solid only under the feet
/// of a player not descending through it, and powder snow collides as
/// [`super::powder_snow::collision_shape`] decides.
pub(super) struct ScaffoldingView<'a, W> {
    inner: &'a W,
    player: Aabb,
    descending: bool,
    powder_snow: super::powder_snow::PowderSnowStance,
}

impl<'a, W: CollisionWorld> ScaffoldingView<'a, W> {
    /// Retains the pre-move box used by the native contextual collision query.
    /// Powder snow stays passable until [`Self::with_powder_snow`] supplies
    /// the mover's fall distance and snow-standing capability.
    pub(super) const fn new(inner: &'a W, player: Aabb, descending: bool) -> Self {
        Self {
            inner,
            player,
            descending,
            powder_snow: super::powder_snow::PowderSnowStance {
                feet_y: player.min.y,
                descending,
                fall_distance: 0.0,
                can_stand_on_snow: false,
            },
        }
    }

    /// Adds the mover facts powder snow's collision reads.
    pub(super) const fn with_powder_snow(
        mut self,
        fall_distance: f32,
        can_stand_on_snow: bool,
    ) -> Self {
        self.powder_snow.fall_distance = fall_distance;
        self.powder_snow.can_stand_on_snow = can_stand_on_snow;
        self
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
        let colliders = self.inner.collision_boxes_for_mover(query)?;
        let crate::MoverColliders {
            colliders: candidates,
            powder_snow,
        } = colliders.value;
        let mut kept = Vec::with_capacity(candidates.len() + powder_snow.len());
        let mut identity = colliders.identity;
        for collider in candidates {
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
        for block in powder_snow {
            if let Some(aabb) = super::powder_snow::collision_shape(block, self.powder_snow)
                && aabb.intersects(query)
            {
                kept.push(ProvenancedCollider {
                    aabb,
                    block: Some(block),
                    runtime_id: None,
                    cell_has_scaffolding: Some(false),
                });
            }
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
