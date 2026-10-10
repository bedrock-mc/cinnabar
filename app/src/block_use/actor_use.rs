//! Actor use shares the pre-physics pick and press admission with block use.
use super::*;
use gameplay::melee::{Crosshair, classify, pick_actor};

/// Resolves an admitted press against the fresh actor/block pick before block use.
#[allow(clippy::too_many_arguments)]
pub(super) fn produce(
    context: &BlockUseContext,
    runtime: &mut BlockUseRuntime,
    selection: &FrozenMiningSelection,
    pick: FramePick,
    state: &gameplay::movement::UnsentSampleView,
    input_mode: PlayerInputMode,
    trigger: ItemUseTrigger,
    due: u64,
    clock: RepeatClock,
) -> bool {
    let Some(stream) = context.client_world.stream.as_ref() else {
        return false;
    };
    let Some(ray) = context.origin.outbound_ray() else {
        return false;
    };
    if !crate::interaction_authority::ray_is_current(ray, context.ui.session_id(), stream)
        || pick.session_generation != ray.session_generation()
        || pick.actor_session_id != ray.actor_session_id()
    {
        return false;
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
    );
    let vector =
        |v: bevy::prelude::Vec3| sim::Vec3::new(f64::from(v.x), f64::from(v.y), f64::from(v.z));
    let block_distance = world
        .block_interaction_ray_current(vector(pick.origin), vector(pick.direction), reach)
        .ok()
        .flatten()
        .map(|hit| hit.distance);
    let Crosshair::Actor(hit) = classify(actor, block_distance, reach) else {
        return false;
    };
    if (0..3)
        .map(|axis| (f64::from(hit.point[axis]) - f64::from(state.position[axis])).powi(2))
        .sum::<f64>()
        > reach * reach
    {
        return false;
    }
    // Actor interactions are presses, rather than the held block-placement repeat.
    if trigger == ItemUseTrigger::SimulationTick {
        return true;
    }
    let packet = protocol::use_actor_packet(
        protocol::ActorUseRequest {
            actor_runtime_id: hit.runtime_id,
            action: protocol::ActorUseAction::Interact,
            selected_slot: selection.slot,
            selected_item: selection.item.clone(),
            player_position: state.position,
            hit_position: hit.point,
        },
        &protocol::BedrockSession { shield_item_id: 0 },
    );
    let admitted = packet
        .ok()
        .is_some_and(|packet| context.network.send_inventory_packets(vec![packet]).is_ok());
    let local_use = stream
        .authority()
        .actor(hit.runtime_id)
        .map_or(LocalUse::Interact, |actor| {
            LocalUse::for_actor(&actor.kind, context.ui.gameplay_hud().has_interact_text())
        });
    runtime.admit(
        trigger,
        due,
        state.tick.saturating_add(1),
        local_use,
        clock,
        admitted,
    );
    true
}
