use actor_animation::ActorLifetimeId;
use bevy::prelude::Vec3;
use client_presentation::{
    aim_assist::{AimAssistTarget, TargetKind},
    local_player::FrozenInteractionOrigin,
};
use client_world::{ActorSnapshot, WorldAuthority};
use gameplay::melee::{ActorHit, Crosshair, classify, pick_actor_by, pick_distance};
use protocol::PlayerInputMode;
use semantic_input::identity::{InputAuthorityGeneration, InputSequence};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

use super::MeleeContext;
use crate::{
    interaction_authority::{BlockRayUnavailable, observe_block_ray, ray_is_current},
    modding::interaction::{block_pick_reach, effective_reach},
};
use client_interaction::mining::hand_interaction_selection;
use gameplay::mining::{creative_reach, protocol_input_mode, survival_reach};

/// Geometry is a proposal until the current press validates it.
#[derive(Clone, Copy)]
pub(super) struct FrameTarget {
    authority: (SessionGeneration, PositionAuthorityGeneration),
    geometry: Geometry,
}

/// Ordinary intercepts and selected assisted points retain their original owners.
#[derive(Clone, Copy)]
enum Geometry {
    Ray {
        actor: ActorLifetimeId,
        point: [f32; 3],
    },
    Assisted(AimAssistTarget),
}

impl Geometry {
    /// Returns the lifetime captured with the geometry, without looking up a reusable ID.
    fn actor(self) -> Option<ActorLifetimeId> {
        match self {
            Self::Ray { actor, .. } => Some(actor),
            Self::Assisted(target) => match target.kind {
                TargetKind::Actor(actor) => Some(actor),
                TargetKind::Block { .. } => None,
            },
        }
    }

    /// Keeps the selected world point independent of current actor motion.
    fn point(self) -> [f32; 3] {
        match self {
            Self::Ray { point, .. } => point,
            Self::Assisted(target) => target.point.to_array(),
        }
    }
}

/// All acquisition paths use the same current policy and validation owner.
struct Picker<'a, 'w, 's> {
    player: &'a client_state::PlayerRuntime,
    context: &'a MeleeContext<'w, 's>,
    world: &'a WorldAuthority,
    ray: &'a FrozenInteractionOrigin,
    authority: (SessionGeneration, PositionAuthorityGeneration),
    input_authority: (InputAuthorityGeneration, InputSequence),
    input_mode: PlayerInputMode,
    block_reach: f64,
    actor_reach: f64,
    attack_reach: f64,
}

impl Picker<'_, '_, '_> {
    /// Binds newly acquired geometry to this press's movement authority.
    fn proposal(&self, geometry: Geometry) -> FrameTarget {
        FrameTarget {
            authority: self.authority,
            geometry,
        }
    }

    /// Recomputes payload distance while preserving the proposed world point.
    fn hit(&self, geometry: Geometry) -> Option<ActorHit> {
        let point = geometry.point();
        point.into_iter().all(f32::is_finite).then_some(ActorHit {
            runtime_id: geometry.actor()?.runtime_id,
            distance: f64::from(pick_distance(self.ray.origin().to_array(), point)),
            point,
        })
    }

    /// Observes current selectable obstruction along a proposed intercept.
    fn block_distance(&self, point: Option<[f32; 3]>) -> Option<f64> {
        let mut origin = (*self.context.origin).clone();
        if let Some(point) = point {
            origin.assist_direction(Vec3::from_array(point) - self.ray.origin());
        }
        let selection = hand_interaction_selection(self.player)?;
        match observe_block_ray(
            &origin,
            &self.context.ui,
            &self.context.client_world,
            &self.context.collisions,
            selection,
            self.player.facts.player_game_mode(),
            (
                self.input_mode,
                self.block_reach,
                self.input_authority,
                self.authority.1,
            ),
        ) {
            Ok(observed) => observed.map(|hit| hit.hit_distance()),
            Err(BlockRayUnavailable) => {
                gameplay::movement::note_click_drop(
                    "attack",
                    "block_ray_unreadable_treated_as_clear",
                );
                None
            }
        }
    }

    /// Finds a current box witness without replacing the proposed packet coordinates.
    fn current_hit(&self, actor: &ActorSnapshot, geometry: Geometry) -> Option<ActorHit> {
        let origin = self.ray.origin();
        let proposed_direction = (Vec3::from_array(geometry.point()) - origin)
            .try_normalize()
            .unwrap_or(self.ray.direction());
        self.world
            .pick_hit_boxes(actor)
            .filter_map(|(minimum, maximum)| {
                let minimum_vec = Vec3::from_array(minimum);
                let maximum_vec = Vec3::from_array(maximum);
                if !minimum_vec.is_finite()
                    || !maximum_vec.is_finite()
                    || minimum_vec.cmpgt(maximum_vec).any()
                {
                    return None;
                }
                if let Geometry::Assisted(target) = geometry {
                    let current = AimAssistTarget {
                        minimum: minimum_vec,
                        maximum: maximum_vec,
                        ..target
                    };
                    if !current.within_pick_range(origin, self.actor_reach as f32) {
                        return None;
                    }
                }
                let direction = (origin.clamp(minimum_vec, maximum_vec) - origin)
                    .try_normalize()
                    .unwrap_or(self.ray.direction());
                let pick = |direction: Vec3| {
                    pick_actor_by(
                        std::iter::once(actor),
                        |_| [(minimum, maximum)],
                        None,
                        origin.to_array(),
                        direction.to_array(),
                        self.actor_reach,
                    )
                };
                pick(proposed_direction).or_else(|| pick(direction))
            })
            .min_by(|left, right| left.distance.total_cmp(&right.distance))
    }

    /// Validates every fresh or retained proposal before it can own an attack or its vetoes.
    fn validate(&self, proposal: FrameTarget) -> Option<Crosshair> {
        if proposal.authority != self.authority {
            return None;
        }
        let lifetime = proposal.geometry.actor()?;
        if self.world.actor_lifetime(lifetime.runtime_id) != Some(lifetime) {
            return None;
        }
        let actor = self.world.actor(lifetime.runtime_id)?;
        if actor.status.dead
            || lifetime.runtime_id == self.world.local_player_runtime_id()
            || Some(actor.unique_id) == self.context.ui.gameplay_hud().mount_unique_id()
            || !gameplay::melee::actor_is_pickable(actor)
        {
            return None;
        }
        if let Geometry::Assisted(target) = proposal.geometry
            && (self.world.camera_aim_assist_eligible(actor) != Some(true)
                || self.context.aim.target.map(|current| current.kind) != Some(target.kind)
                || !target.within_pick_range(self.ray.origin(), self.actor_reach as f32))
        {
            return None;
        }
        let hit = self.hit(proposal.geometry)?;
        if matches!(proposal.geometry, Geometry::Ray { .. }) && hit.distance > self.actor_reach {
            return None;
        }
        let current = self.current_hit(actor, proposal.geometry)?;
        let current = classify(
            Some(current),
            self.block_distance(Some(current.point)),
            self.attack_reach,
        );
        if !matches!(current, Crosshair::Actor(_)) {
            return Some(current);
        }
        Some(classify(
            Some(hit),
            self.block_distance(Some(hit.point)),
            self.attack_reach,
        ))
    }

    /// Acquires the nearest ordinary proposal that passes the shared current validator.
    fn normal_pick(&self) -> Option<(FrameTarget, Crosshair)> {
        self.world
            .remote_actors()
            .filter_map(|actor| {
                let hit = pick_actor_by(
                    std::iter::once(actor),
                    |actor| self.world.pick_hit_boxes(actor),
                    None,
                    self.ray.origin().to_array(),
                    self.ray.direction().to_array(),
                    self.actor_reach,
                )?;
                let proposal = self.proposal(Geometry::Ray {
                    actor: self.world.actor_lifetime(actor.runtime_id)?,
                    point: hit.point,
                });
                Some((proposal, self.validate(proposal)?))
            })
            .min_by(|(left, _), (right, _)| {
                pick_distance(self.ray.origin().to_array(), left.geometry.point()).total_cmp(
                    &pick_distance(self.ray.origin().to_array(), right.geometry.point()),
                )
            })
    }
}

/// Publishes fresh geometry and validates the retained proposal against this press's facts.
pub(super) fn resolve_crosshair(
    player: &client_state::PlayerRuntime,
    context: &MeleeContext,
    authority: (SessionGeneration, PositionAuthorityGeneration),
    retained: Option<FrameTarget>,
) -> Option<(Option<FrameTarget>, Crosshair)> {
    let ray = context.origin.outbound_ray()?;
    let stream = context.client_world.stream.as_ref()?;
    if !ray_is_current(ray, SessionGeneration::new(context.ui.session_id()), stream) {
        return None;
    }
    let input = context.input.snapshot()?;
    let input_mode = protocol_input_mode(input.input_mode);
    let caps = player.facts.game_mode_capabilities();
    let pick_reach = if caps.is_some_and(|caps| caps.creative_reach) {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let actor_reach = effective_reach(pick_reach, context.mod_interaction.as_deref());
    let picker = Picker {
        player,
        context,
        world: stream.authority(),
        ray,
        authority,
        input_authority: (input.authority_generation, input.frame_sequence),
        input_mode,
        block_reach: block_pick_reach(pick_reach, actor_reach),
        actor_reach,
        attack_reach: effective_reach(
            caps.map_or(
                client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH,
                |caps| caps.attack_reach,
            ),
            context.mod_interaction.as_deref(),
        ),
    };
    let fresh = context
        .aim
        .target
        .and_then(|target| {
            let proposal = picker.proposal(Geometry::Assisted(target));
            let target = picker.validate(proposal)?;
            matches!(target, Crosshair::Actor(_)).then_some((proposal, target))
        })
        .or_else(|| picker.normal_pick());
    let published = fresh
        .filter(|(_, target)| matches!(target, Crosshair::Actor(_)))
        .map(|(proposal, _)| proposal);
    let target = retained
        .and_then(|proposal| picker.validate(proposal))
        .filter(|target| matches!(target, Crosshair::Actor(_)))
        .or_else(|| fresh.map(|(_, target)| target))
        .unwrap_or_else(|| classify(None, picker.block_distance(None), picker.attack_reach));
    Some((published, target))
}

#[cfg(test)]
#[path = "crosshair/interaction_traits_tests.rs"]
mod interaction_traits_tests;
