use actor_animation::ActorLifetimeId;
use bevy::prelude::Quat;
use client_presentation::{
    aim_assist::{AimAssistTarget, TargetKind},
    local_player::InteractionOriginSnapshot,
};
use gameplay::melee::{ActorHit, Crosshair, classify, pick_actor_by, pick_distance};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::{collections::HashMap, ops::Range, sync::Arc};

use super::MeleeContext;
use crate::{
    interaction_authority::{BlockRayUnavailable, observe_block_ray_using, ray_is_current},
    interaction_override::{block_pick_reach, effective_reach},
};
use client_interaction::mining::hand_interaction_selection;
use gameplay::mining::{creative_reach, protocol_input_mode, survival_reach};

/// One actor's identity and its ordered boxes in the frame's geometry buffer.
#[derive(Clone)]
struct FrameActor {
    identity: ActorLifetimeId,
    boxes: Range<usize>,
}

/// Published picking inputs contain no hit, classification or admission result.
#[derive(Default, Clone)]
struct Frame {
    origin: InteractionOriginSnapshot,
    action_rotation: Option<Quat>,
    position_authority: PositionAuthorityGeneration,
    actors: Vec<FrameActor>,
    index: HashMap<u64, usize>,
    boxes: Vec<([f32; 3], [f32; 3])>,
    assisted: Option<AimAssistTarget>,
}

impl Frame {
    /// Refreshes a reusable geometry buffer from one coherent published action boundary.
    fn publish(&mut self, context: &MeleeContext, position_authority: PositionAuthorityGeneration) {
        self.origin.clone_from(&context.origin);
        self.action_rotation =
            client_interaction::camera::aim_assist::action_rotation(&context.aim, &context.camera);
        self.position_authority = position_authority;
        self.assisted = context.aim.target;
        self.actors.clear();
        self.index.clear();
        self.boxes.clear();
        if let Some(stream) = context.client_world.stream.as_ref() {
            let world = stream.authority();
            for actor in world.remote_actors() {
                let start = self.boxes.len();
                self.boxes.extend(world.pick_hit_boxes(actor));
                self.actors.push(FrameActor {
                    identity: world.actor_lifetime(actor.runtime_id).unwrap(),
                    boxes: start..self.boxes.len(),
                });
            }
        }
        self.index.extend(
            self.actors
                .iter()
                .enumerate()
                .map(|(index, actor)| (actor.identity.runtime_id, index)),
        );
    }

    /// Borrows the actor's ordered published boxes through the index derived from this frame.
    fn hit_boxes(&self, runtime_id: u64) -> impl Iterator<Item = ([f32; 3], [f32; 3])> {
        let actor = &self.actors[self.index[&runtime_id]];
        self.boxes[actor.boxes.clone()].iter().copied()
    }
}

/// Reusable buffers keep publication history and late dispatch geometry independent.
#[derive(Default)]
pub(crate) struct FrameInputs {
    previous: Arc<Frame>,
    current: Arc<Frame>,
    late: Arc<Frame>,
}

impl FrameInputs {
    /// Identifies a complete published frame without advancing the two frame buffers.
    pub(super) fn has_current(&self, position: PositionAuthorityGeneration) -> bool {
        self.current.position_authority == position && self.current.origin.outbound_ray().is_some()
    }

    /// Publishes the current ray and frame-positioned actor boxes together, reusing storage.
    pub(super) fn publish(
        &mut self,
        context: &MeleeContext,
        position_authority: PositionAuthorityGeneration,
    ) {
        std::mem::swap(&mut self.previous, &mut self.current);
        if Arc::get_mut(&mut self.current).is_none() {
            self.current = Arc::default();
        }
        Arc::get_mut(&mut self.current)
            .expect("unshared publication buffer")
            .publish(context, position_authority);
    }

    /// Captures late actions without advancing the regular two-frame publication history.
    pub(super) fn capture_current(
        &mut self,
        context: &MeleeContext,
        position_authority: PositionAuthorityGeneration,
    ) -> CapturedInputs {
        Arc::make_mut(&mut self.late).publish(context, position_authority);
        CapturedInputs(Arc::clone(&self.late))
    }

    /// A first-frame press or position reset uses the current complete input snapshot.
    fn published(&self, position_authority: PositionAuthorityGeneration) -> &Frame {
        if self.previous.position_authority == position_authority
            && self.previous.origin.outbound_ray().is_some()
        {
            &self.previous
        } else {
            &self.current
        }
    }
}

/// One frame's resolved press and its action-facing decision travel together.
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct ResolvedTarget {
    pub(crate) crosshair: Crosshair,
    pub(crate) hover: Option<gameplay::entity_hover::HoveredActor>,
    pub(crate) direction: [f32; 3],
    pub(crate) action_rotation: Option<Quat>,
    pub(super) reach: AttackReach,
}

/// Effective picking and attack distances that authorize one frozen actor action.
#[derive(Clone, Copy, PartialEq)]
pub(super) struct AttackReach {
    block: f64,
    actor: f64,
    attack: f64,
}

/// Resolves the current mode, input profile and local reach override in one place.
pub(super) fn attack_reach(
    player: &client_state::PlayerRuntime,
    context: &MeleeContext,
    mode: semantic_input::InputMode,
) -> AttackReach {
    let caps = player.facts.game_mode_capabilities();
    let block = if caps.is_some_and(|caps| caps.creative_reach) {
        creative_reach(protocol_input_mode(mode))
    } else {
        survival_reach(protocol_input_mode(mode))
    };
    AttackReach {
        block,
        actor: effective_reach(block, context.mod_interaction.as_deref()),
        attack: effective_reach(
            caps.map_or(
                client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH,
                |caps| caps.attack_reach,
            ),
            context.mod_interaction.as_deref(),
        ),
    }
}

/// Picks once against published camera and actor geometry, using current admission policy.
pub(super) fn resolve_crosshair(
    player: &client_state::PlayerRuntime,
    context: &MeleeContext,
    frames: &FrameInputs,
    position_authority: PositionAuthorityGeneration,
) -> Option<ResolvedTarget> {
    context.origin.outbound_ray()?;
    let input = context.input.snapshot()?;
    resolve_frame(
        player,
        context,
        frames.published(position_authority),
        input.input_mode,
        (input.authority_generation, input.frame_sequence),
    )
    .map(|(resolved, _)| resolved)
}

/// Immutable published geometry shared by a batch, with no cached hit or admission decision.
#[derive(Clone)]
pub(crate) struct CapturedInputs(Arc<Frame>);

/// Captures the early dispatch boundary from the normal published-frame history.
pub(super) fn capture_ordered_inputs(
    frames: &FrameInputs,
    position: PositionAuthorityGeneration,
    prepared_this_frame: bool,
) -> CapturedInputs {
    let frame = if prepared_this_frame
        && frames.previous.position_authority == position
        && frames.previous.origin.outbound_ray().is_some()
    {
        &frames.previous
    } else {
        &frames.current
    };
    CapturedInputs(Arc::clone(frame))
}

/// Resolves each ordered press separately against its published inputs and current policy.
pub(super) fn resolve_captured(
    player: &client_state::PlayerRuntime,
    context: &MeleeContext,
    captured: &CapturedInputs,
    input: &crate::ordered_interactions::InteractionInput,
) -> Option<(ResolvedTarget, Option<sim::WorldCollisionIdentity>)> {
    input.context.origin.outbound_ray()?;
    resolve_frame(
        player,
        context,
        &captured.0,
        input.context.mode,
        input.context.identity,
    )
}

/// Applies current admission policy to one coherent published camera and actor frame.
/// Defers hover and attack together while the selected hand awaits transport.
fn resolve_frame(
    player: &client_state::PlayerRuntime,
    context: &MeleeContext,
    frame: &Frame,
    mode: semantic_input::InputMode,
    identity: (
        semantic_input::identity::InputAuthorityGeneration,
        semantic_input::identity::InputSequence,
    ),
) -> Option<(ResolvedTarget, Option<sim::WorldCollisionIdentity>)> {
    let ray = frame.origin.outbound_ray()?;
    let stream = context.client_world.stream.as_ref()?;
    if !ray_is_current(ray, SessionGeneration::new(context.ui.session_id()), stream) {
        return None;
    }
    let world = stream.authority();
    let selection = hand_interaction_selection(player)?;
    let input_mode = protocol_input_mode(mode);
    let reach = attack_reach(player, context, mode);
    let actor_reach = reach.actor;
    let attack_reach = reach.attack;
    let origin = ray.origin().to_array();
    let mount = context.ui.gameplay_hud().mount_unique_id();
    let eligible = |identity: ActorLifetimeId| {
        world.actor(identity.runtime_id).filter(|actor| {
            world.actor_lifetime(identity.runtime_id) == Some(identity)
                && !actor.status.dead
                && identity.runtime_id != stream.local_player_runtime_id()
                && Some(actor.unique_id) != mount
                && gameplay::melee::actor_is_pickable(actor)
        })
    };
    let normal = || {
        pick_actor_by(
            frame
                .actors
                .iter()
                .filter_map(|actor| eligible(actor.identity)),
            |actor| frame.hit_boxes(actor.runtime_id),
            mount,
            origin,
            ray.direction().to_array(),
            actor_reach,
        )
    };
    let mut obstruction_origin = frame.origin.clone();
    let actor = match frame.assisted {
        Some(target) => match target.kind {
            TargetKind::Actor(identity) => eligible(identity)
                .filter(|actor| {
                    world.camera_aim_assist_eligible(actor) == Some(true)
                        && frame.actors.iter().any(|actor| actor.identity == identity)
                        && target.within_pick_range(ray.origin(), actor_reach as f32)
                })
                .map(|_| {
                    obstruction_origin.assist_direction(target.point - ray.origin());
                    ActorHit {
                        runtime_id: identity.runtime_id,
                        distance: f64::from(pick_distance(origin, target.point.to_array())),
                        point: target.point.to_array(),
                    }
                })
                .or_else(normal),
            TargetKind::Block { .. } => None,
        },
        None => normal(),
    };
    let observed = match observe_block_ray_using(
        &obstruction_origin,
        &context.ui,
        &context.client_world,
        &context.collisions,
        selection,
        player.facts.player_game_mode(),
        false,
        (
            input_mode,
            block_pick_reach(reach.block, actor_reach),
            identity,
            frame.position_authority,
        ),
        |world, origin, direction, reach| {
            world.block_interaction_ray_current(origin, direction, reach)
        },
    ) {
        Ok(observed) => Some(observed),
        Err(BlockRayUnavailable) => {
            gameplay::movement::note_click_drop("attack", "block_ray_unreadable_treated_as_clear");
            None
        }
    };
    let block_distance = observed
        .as_ref()
        .and_then(|observed| observed.value.as_ref())
        .map(|block| block.hit_distance());
    // Hover, attack and interaction share one target: the pick ray finds actors along the
    // whole pick range, but only a hit within the game-mode cap becomes the targeted actor.
    let crosshair = classify(actor, block_distance, attack_reach);
    let hover = match crosshair {
        Crosshair::Actor(hit) => Some(gameplay::entity_hover::HoveredActor {
            unique_id: world.actor(hit.runtime_id)?.unique_id,
            runtime_id: hit.runtime_id,
            point: hit.point,
        }),
        _ => None,
    };
    let value = ResolvedTarget {
        hover,
        crosshair,
        action_rotation: frame.action_rotation,
        direction: ray.direction().to_array(),
        reach,
    };
    Some((value, observed.map(|observed| observed.identity)))
}

#[cfg(test)]
#[path = "crosshair/interaction_traits_tests.rs"]
mod interaction_traits_tests;
