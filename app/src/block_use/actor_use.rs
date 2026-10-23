//! Actor use shares the pre-physics pick and press admission with block use.
use client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH;
use gameplay::melee::{Crosshair, classify_block_hit, pick_actor};
use simulation_clock::identity::SessionGeneration;
use {
    super::*,
    gameplay::block_use::{LocalUse, RepeatClock},
    gameplay::mining::{FrozenMiningSelection, creative_reach, survival_reach},
};

/// Current actor-use packet and the local effects committed on successful admission.
pub(crate) struct ActorUse {
    packet: protocol::Packet,
    local_use: LocalUse,
    tick: u64,
    due: std::time::Duration,
    clock: RepeatClock,
}

/// Resolves the actor/block competition with the same eligibility as ordinary block use.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare(
    context: &BlockUseContext,
    origin: &InteractionOriginSnapshot,
    selection: &FrozenMiningSelection,
    player: &client_state::PlayerRuntime,
    pick: FramePick,
    state: &gameplay::movement::UnsentSampleView,
    input_mode: PlayerInputMode,
    due: std::time::Duration,
    clock: RepeatClock,
) -> Option<ActorUse> {
    let game_mode = player.facts.player_game_mode();
    let stream = context.client_world.stream.as_ref()?;
    let ray = origin.outbound_ray()?;
    if !crate::interaction_authority::ray_is_current(
        ray,
        SessionGeneration::new(context.ui.session_id()),
        stream,
    ) || pick.session_generation != ray.session_generation()
        || pick.actor_session_id != ray.actor_session_id()
    {
        return None;
    }
    let reach = if clock.survival {
        survival_reach(input_mode)
    } else {
        creative_reach(input_mode)
    };
    let actor = pick_actor(
        stream.authority().remote_actors(),
        context.ui.gameplay_hud().mount_unique_id(),
        pick.origin.to_array(),
        pick.direction.to_array(),
        reach,
    );
    let world = PaletteWorld::new(
        stream.collision_store(),
        context.collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
        crate::interaction_authority::pick_context(
            stream,
            &context.collisions,
            game_mode,
            Some(selection.item.block_runtime_id()),
        ),
    );
    let vector =
        |v: bevy::prelude::Vec3| sim::Vec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z));
    let obstruction = world
        .observe(|world| {
            world.block_interaction_ray_current(vector(pick.origin), vector(pick.direction), reach)
        })
        .ok()
        .and_then(|query| query.value.ok().map(|block| (block, query.identity)));
    let block = obstruction.as_ref().and_then(|(block, _)| block.as_ref());
    // Like attacks, an actor is the use target only within the game-mode cap.
    let cap = player
        .facts
        .game_mode_capabilities()
        .map_or(SURVIVAL_ATTACK_REACH, |caps| caps.attack_reach);
    let Crosshair::Actor(hit) = classify_block_hit(actor, pick.origin.to_array(), block, cap)
    else {
        return None;
    };
    if (0..3)
        .map(|axis| (f64::from(hit.point[axis]) - f64::from(state.position[axis])).powi(2))
        .sum::<f64>()
        > reach * reach
    {
        return None;
    }
    let packet = protocol::use_actor_packet(protocol::ActorUseRequest {
        actor_runtime_id: hit.runtime_id,
        action: protocol::ActorUseAction::Interact,
        selected_slot: selection.slot,
        selected_item: selection.item.clone(),
        player_position: state.position,
        hit_position: hit.point,
    })
    .ok()?;
    let local_use = stream
        .authority()
        .actor(hit.runtime_id)
        .map_or(LocalUse::Interact, |actor| {
            LocalUse::for_actor(&actor.kind, context.ui.gameplay_hud().has_interact_text())
        });
    Some(ActorUse {
        packet,
        local_use,
        tick: state.tick.saturating_add(1),
        due,
        clock,
    })
}

/// Commits the current actor use only after its packet enters the session FIFO.
pub(crate) fn advance(
    context: &BlockUseContext,
    runtime: &mut BlockUseRuntime,
    attempt: &mut ActorUse,
) -> bool {
    match context
        .network
        .send_inventory_packets(vec![attempt.packet.clone()])
    {
        Ok(()) => {
            runtime.admit(
                ItemUseTrigger::PlayerInput,
                attempt.due,
                attempt.tick,
                attempt.local_use,
                attempt.clock,
                true,
            );
            true
        }
        Err(client_session::BatchSendError::Full) => false,
        Err(client_session::BatchSendError::Closed) => {
            runtime.clear_press();
            true
        }
    }
}

/// Handles an actor press through the shared immediate admission boundary.
#[allow(clippy::too_many_arguments)]
pub(super) fn produce(
    context: &BlockUseContext,
    runtime: &mut BlockUseRuntime,
    selection: &FrozenMiningSelection,
    player: &client_state::PlayerRuntime,
    pick: FramePick,
    state: &gameplay::movement::UnsentSampleView,
    input_mode: PlayerInputMode,
    trigger: ItemUseTrigger,
    due: std::time::Duration,
    clock: RepeatClock,
) -> bool {
    let Some(mut attempt) = prepare(
        context,
        &context.origin,
        selection,
        player,
        pick,
        state,
        input_mode,
        due,
        clock,
    ) else {
        return false;
    };
    if trigger != ItemUseTrigger::SimulationTick {
        advance(context, runtime, &mut attempt);
    }
    true
}
