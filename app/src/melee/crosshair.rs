use crate::modding::interaction::{block_pick_reach, effective_reach};
use protocol::PlayerInputMode;
use semantic_input::identity::{InputAuthorityGeneration, InputSequence};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use {
    super::MeleeContext,
    gameplay::melee::{Crosshair, classify},
};
use {
    crate::interaction_authority::{BlockRayUnavailable, observe_block_ray, ray_is_current},
    client_interaction::mining::hand_interaction_selection,
    gameplay::mining::{creative_reach, survival_reach},
};

/// A completed pick retains its intercept independently from later actor motion and look input.
#[derive(Clone, Copy)]
pub(super) struct FrameTarget {
    authority: (SessionGeneration, PositionAuthorityGeneration),
    actor_session_id: u64,
    target: Crosshair,
    spawn_revision: Option<u64>,
}

impl FrameTarget {
    /// Binds the exact hit point to its movement authority and actor lifetime.
    pub(super) fn new(
        target: Crosshair,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        world: &client_world::WorldAuthority,
    ) -> Self {
        Self {
            authority,
            actor_session_id: world.actor_session_id(),
            target,
            spawn_revision: match target {
                Crosshair::Actor(hit) => world
                    .actor(hit.runtime_id)
                    .map(|actor| actor.spawn_revision),
                _ => None,
            },
        }
    }

    /// Rejects replaced authority; an actor removed or replaced since the pick becomes a miss.
    pub(super) fn crosshair(
        self,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        world: &client_world::WorldAuthority,
    ) -> Option<Crosshair> {
        if self.authority != authority || self.actor_session_id != world.actor_session_id() {
            return None;
        }
        Some(match self.target {
            Crosshair::Actor(hit)
                if self.spawn_revision.is_none()
                    || world
                        .actor(hit.runtime_id)
                        .map(|actor| actor.spawn_revision)
                        != self.spawn_revision =>
            {
                Crosshair::Miss
            }
            target => target,
        })
    }
}

/// Assisted actor identity is preserved while ordinary reach and world obstruction remain authoritative.
pub(super) fn resolve_crosshair(
    player_runtime: &client_state::PlayerRuntime,
    context: &MeleeContext,
    input_mode: PlayerInputMode,
    attack_reach: f64,
    creative_pick_reach: bool,
    input_authority: (InputAuthorityGeneration, InputSequence),
    position_authority_generation: PositionAuthorityGeneration,
) -> Option<Crosshair> {
    let ray = context.origin.outbound_ray()?;
    let stream = context.client_world.stream.as_ref()?;
    if !ray_is_current(ray, SessionGeneration::new(context.ui.session_id()), stream) {
        return None;
    }
    let reach = if creative_pick_reach {
        creative_reach(input_mode)
    } else {
        survival_reach(input_mode)
    };
    let actor_reach = effective_reach(reach, context.mod_interaction.as_deref());

    let reach = block_pick_reach(reach, actor_reach);
    let origin = ray.origin().to_array();
    // Vanilla picks against the world it holds, where unreadable space is empty; an
    // unreadable block ray therefore neither blocks the swing nor occludes a target.
    let observed = hand_interaction_selection(player_runtime).and_then(|selection| {
        match observe_block_ray(
            &context.origin,
            &context.ui,
            &context.client_world,
            &context.collisions,
            selection,
            player_runtime.facts.player_game_mode(),
            (
                input_mode,
                reach,
                input_authority,
                position_authority_generation,
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
    let block_distance = observed.map(|observed| observed.hit_distance());
    let authority = stream.authority();
    let mount = context.ui.gameplay_hud().mount_unique_id();
    let actor = match context.aim.target {
        Some(target) => match target.kind {
            client_presentation::aim_assist::TargetKind::Actor(runtime_id) => authority
                .actor(runtime_id)
                .filter(|actor| {
                    runtime_id != stream.local_player_runtime_id()
                        && Some(actor.unique_id) != mount
                        && gameplay::melee::actor_is_pickable(actor)
                        && target.within_pick_range(
                            bevy::prelude::Vec3::from_array(origin),
                            actor_reach as f32,
                        )
                })
                .map(|_| gameplay::melee::ActorHit {
                    runtime_id,
                    distance: f64::from(gameplay::melee::pick_distance(
                        origin,
                        target.point.to_array(),
                    )),
                    point: target.point.to_array(),
                }),
            client_presentation::aim_assist::TargetKind::Block { .. } => None,
        },
        None => gameplay::melee::pick_actor_by(
            authority.remote_actors(),
            |actor| authority.pick_hit_boxes(actor),
            mount,
            origin,
            ray.direction().to_array(),
            actor_reach,
        ),
    };
    Some(classify(actor, block_distance, attack_reach))
}

#[cfg(test)]
#[path = "crosshair/interaction_traits_tests.rs"]
mod interaction_traits_tests;
