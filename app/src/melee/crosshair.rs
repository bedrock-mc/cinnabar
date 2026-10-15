use actor_animation::ActorLifetimeId;
use client_presentation::{
    aim_assist::{AimAssistTarget, TargetKind},
    local_player::InteractionOriginSnapshot,
};
use gameplay::melee::{ActorHit, Crosshair, classify, pick_actor_by, pick_distance};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use std::{collections::HashMap, ops::Range};

use super::MeleeContext;
use crate::{
    interaction_authority::{BlockRayUnavailable, observe_block_ray, ray_is_current},
    modding::interaction::{block_pick_reach, effective_reach},
};
use client_interaction::mining::hand_interaction_selection;
use gameplay::mining::{creative_reach, protocol_input_mode, survival_reach};

/// One actor's identity and its ordered boxes in the frame's geometry buffer.
struct FrameActor {
    identity: ActorLifetimeId,
    boxes: Range<usize>,
}

/// Published picking inputs contain no hit, classification or admission result.
#[derive(Default)]
struct Frame {
    origin: InteractionOriginSnapshot,
    actors: Vec<FrameActor>,
    index: HashMap<u64, usize>,
    boxes: Vec<([f32; 3], [f32; 3])>,
    assisted: Option<AimAssistTarget>,
}

impl Frame {
    /// Borrows the actor's ordered published boxes through the index derived from this frame.
    fn hit_boxes(&self, runtime_id: u64) -> impl Iterator<Item = ([f32; 3], [f32; 3])> {
        let actor = &self.actors[self.index[&runtime_id]];
        self.boxes[actor.boxes.clone()].iter().copied()
    }
}

/// Two reusable input buffers keep the preceding published frame coherent.
#[derive(Default)]
pub(crate) struct FrameInputs {
    previous: Frame,
    current: Frame,
}

impl FrameInputs {
    /// Publishes the current ray and frame-positioned actor boxes together, reusing storage.
    pub(super) fn publish(&mut self, context: &MeleeContext) {
        std::mem::swap(&mut self.previous, &mut self.current);
        let frame = &mut self.current;
        frame.origin.clone_from(&context.origin);
        frame.assisted = context.aim.target;
        frame.actors.clear();
        frame.index.clear();
        frame.boxes.clear();
        if let Some(stream) = context.client_world.stream.as_ref() {
            let world = stream.authority();
            for actor in world.remote_actors() {
                let start = frame.boxes.len();
                frame.boxes.extend(world.pick_hit_boxes(actor));
                frame.actors.push(FrameActor {
                    identity: world.actor_lifetime(actor.runtime_id).unwrap(),
                    boxes: start..frame.boxes.len(),
                });
            }
        }
        frame.index.extend(
            frame
                .actors
                .iter()
                .enumerate()
                .map(|(index, actor)| (actor.identity.runtime_id, index)),
        );
    }

    /// A first-frame press uses the first complete input snapshot.
    fn published(&self) -> &Frame {
        if self.previous.origin.outbound_ray().is_some() {
            &self.previous
        } else {
            &self.current
        }
    }
}

/// Picks once against published camera and actor geometry, using current admission policy.
pub(super) fn resolve_crosshair(
    player: &client_state::PlayerRuntime,
    context: &MeleeContext,
    frames: &FrameInputs,
    position_authority: PositionAuthorityGeneration,
) -> Option<Crosshair> {
    context.origin.outbound_ray()?;
    let frame = frames.published();
    let ray = frame.origin.outbound_ray()?;
    let stream = context.client_world.stream.as_ref()?;
    if !ray_is_current(ray, SessionGeneration::new(context.ui.session_id()), stream) {
        return None;
    }
    let world = stream.authority();
    let input = context.input.snapshot()?;
    let input_mode = protocol_input_mode(input.input_mode);
    let caps = player.facts.game_mode_capabilities();
    let reach = if caps.is_some_and(|caps| caps.creative_reach) {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let actor_reach = effective_reach(reach, context.mod_interaction.as_deref());
    let attack_reach = effective_reach(
        caps.map_or(
            client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH,
            |caps| caps.attack_reach,
        ),
        context.mod_interaction.as_deref(),
    );
    let observed =
        hand_interaction_selection(player).and_then(|selection| {
            match observe_block_ray(
                &frame.origin,
                &context.ui,
                &context.client_world,
                &context.collisions,
                selection,
                player.facts.player_game_mode(),
                (
                    input_mode,
                    block_pick_reach(reach, actor_reach),
                    (input.authority_generation, input.frame_sequence),
                    position_authority,
                ),
            ) {
                Ok(observed) => observed,
                Err(BlockRayUnavailable) => {
                    gameplay::movement::note_click_drop(
                        "attack",
                        "block_ray_unreadable_treated_as_clear",
                    );
                    None
                }
            }
        });
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
    let actor = match frame.assisted {
        Some(target) => match target.kind {
            TargetKind::Actor(identity) => eligible(identity)
                .filter(|actor| {
                    world.camera_aim_assist_eligible(actor) == Some(true)
                        && frame.actors.iter().any(|actor| actor.identity == identity)
                        && target.within_pick_range(ray.origin(), actor_reach as f32)
                })
                .map(|_| ActorHit {
                    runtime_id: identity.runtime_id,
                    distance: f64::from(pick_distance(origin, target.point.to_array())),
                    point: target.point.to_array(),
                })
                .or_else(normal),
            TargetKind::Block { .. } => None,
        },
        None => normal(),
    };
    Some(classify(
        actor,
        observed.map(|block| block.hit_distance()),
        attack_reach,
    ))
}

#[cfg(test)]
#[path = "crosshair/interaction_traits_tests.rs"]
mod interaction_traits_tests;
