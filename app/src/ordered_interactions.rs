//! Dispatches each ordered input edge once in its own frame.

use bevy::prelude::*;
use client_interaction::{
    block_use::BlockUseRuntime, item_use::ItemUseRuntime, melee::MeleeRuntime,
    movement::MovementTicker,
};
use client_presentation::local_player::{InteractionOriginSnapshot, LocalViewPose};
use client_state::SemanticInputSnapshot;
use gameplay::ordered_interactions::{InteractionScope, Invalidation, OrderedInteractions};
use semantic_input::{Action, ActionEvent, ActionSnapshot};

/// The resolved edge and the frame geometry used during its immediate dispatch.
#[derive(Clone)]
pub(crate) struct InteractionInput {
    pub edge: ActionEvent,
    pub context: CapturedContext,
}

/// Input authority, pose and assistance captured together before action dispatch.
#[derive(Clone)]
pub(crate) struct CapturedContext {
    pub mode: semantic_input::InputMode,
    pub identity: (
        semantic_input::identity::InputAuthorityGeneration,
        semantic_input::identity::InputSequence,
    ),
    pub origin: InteractionOriginSnapshot,
    pub view: LocalViewPose,
    pub aim: Option<Quat>,
}

/// Tracks consumed frame edges and overload recovery without retaining action attempts.
#[derive(Resource, Default)]
pub(crate) struct InteractionRuntime {
    frames: OrderedInteractions,
    ingested: usize,
    overflowed_batches: u64,
}

/// Reports whether an incomplete physical sample still suppresses held input.
pub(crate) fn is_blocked(runtime: Option<&InteractionRuntime>) -> bool {
    runtime.is_some_and(|runtime| runtime.frames.quarantined())
}

/// Keeps held placement behind accepted mining, item-use and movement work.
pub(crate) fn pending(world: &World) -> bool {
    let Some(runtime) = world.get_resource::<InteractionRuntime>() else {
        return false;
    };
    is_blocked(Some(runtime))
        || world
            .get_resource::<MeleeRuntime>()
            .is_some_and(|runtime| runtime.has_queued_work())
        || world
            .get_resource::<MovementTicker>()
            .is_some_and(|movement| movement.has_held_release())
        || world
            .get_resource::<ItemUseRuntime>()
            .is_some_and(|runtime| runtime.has_pending_release())
}

/// Reads current held state, suppressing it until an incomplete sample returns to neutral.
pub(crate) fn held(
    runtime: Option<&InteractionRuntime>,
    _action: Action,
    final_held: bool,
) -> bool {
    !is_blocked(runtime) && final_held
}

/// Cancels unadmitted gestures without deleting gameplay-owned accepted carriers.
fn cancel_pending_owners(world: &mut World) {
    world.resource_mut::<MeleeRuntime>().cancel();
    world.resource_mut::<BlockUseRuntime>().clear_press();
    world
        .resource_mut::<ItemUseRuntime>()
        .cancel_pending_input();
}

/// Captures geometry after UI authority at the existing published picking phase.
fn capture(world: &mut World, input: &ActionSnapshot) -> CapturedContext {
    let position = world
        .resource::<MovementTicker>()
        .interaction_authority_identity()
        .1;
    if !world
        .resource::<crate::melee::MeleeTarget>()
        .has_published_inputs(position)
    {
        world
            .run_system_cached(crate::melee::prepare_melee)
            .expect("ordered pick preparation");
    }
    let aim = world
        .get_resource::<client_presentation::aim_assist::AimAssistFrame>()
        .zip(world.get_resource::<client_presentation::camera::ServerCameraView>())
        .and_then(|(aim, camera)| {
            client_interaction::camera::aim_assist::action_rotation(aim, camera)
        });
    CapturedContext {
        mode: input.input_mode,
        identity: (input.authority_generation, input.frame_sequence),
        origin: world.resource::<InteractionOriginSnapshot>().clone(),
        view: *world.resource::<LocalViewPose>(),
        aim,
    }
}

/// Consumes each frame's edges once; unavailable actions never become later-frame input.
pub(crate) fn drain(world: &mut World) {
    let snapshot = world
        .resource::<SemanticInputSnapshot>()
        .snapshot()
        .cloned();
    let Some(snapshot) = snapshot.filter(|input| input.ordered.is_some()) else {
        if world.remove_resource::<InteractionRuntime>().is_some() {
            cancel_pending_owners(world);
        }
        return;
    };
    let ordered = snapshot.ordered.as_ref().expect("ordered snapshot");
    let mut runtime = world
        .remove_resource::<InteractionRuntime>()
        .unwrap_or_default();
    let authority = world
        .resource::<MovementTicker>()
        .interaction_authority_identity();
    let scope = InteractionScope {
        session: authority.0,
        position: authority.1,
        dimension: world
            .resource::<crate::runtime::world::ClientWorld>()
            .stream
            .as_ref()
            .map_or(0, |stream| stream.current_dimension()),
        input: snapshot.authority_generation,
    };
    let new_frame = runtime.frames.observe_frame(scope, snapshot.frame_sequence);
    if new_frame {
        runtime.ingested = 0;
    }
    let focused = world
        .query_filtered::<&Window, With<bevy::window::PrimaryWindow>>()
        .single(world)
        .is_ok_and(|window| window.focused)
        && !world.resource::<client_state::ScreenFocus>().menu_visible
        && !world.resource::<client_state::ScreenFocus>().ui_focused
        && !world.resource::<client_state::ScreenFocus>().absorbs_input;
    let active = focused
        && world
            .resource::<client_state::PlayerRuntime>()
            .facts
            .player_game_mode()
            != Some(protocol::PlayerGameMode::Spectator)
        && !world
            .resource::<crate::runtime::world::ClientWorld>()
            .respawn
            .input_held();
    let invalidation = runtime
        .frames
        .synchronize(scope, (new_frame && snapshot.authority_released) || !active);
    if invalidation != Invalidation::None {
        cancel_pending_owners(world);
        if invalidation == Invalidation::World {
            world.resource_mut::<MeleeRuntime>().synchronize(authority);
            world
                .resource_mut::<BlockUseRuntime>()
                .synchronize(authority);
            world
                .resource_mut::<client_interaction::survival_mining::SurvivalMiningRuntime>()
                .synchronize(authority);
            world.resource_mut::<ItemUseRuntime>().cancel_for_respawn();
        }
    }
    let tail = ordered.events().get(runtime.ingested..).unwrap_or_default();
    runtime.ingested = runtime.ingested.max(ordered.events().len());
    if ordered.overflowed() {
        if new_frame {
            runtime.overflowed_batches = runtime.overflowed_batches.saturating_add(1);
            bevy::log::warn!(
                batches = runtime.overflowed_batches,
                "ordered action burst lost input; waiting for a neutral sample"
            );
        }
        runtime.frames.quarantine();
        cancel_pending_owners(world);
    } else {
        runtime.frames.observe_neutral(ordered.neutral());
    }
    world
        .run_system_cached(crate::item_use::ordered::flush_release)
        .expect("ordered release carrier");
    let stop_ready = world
        .run_system_cached_with(
            crate::block_use::ordered::fence,
            !active || snapshot.authority_released || runtime.frames.quarantined(),
        )
        .expect("accepted block stop carrier");
    let selection_ready = world
        .run_system_cached(crate::hotbar::ordered::flush_existing)
        .expect("older selection carrier");
    if active
        && !snapshot.authority_released
        && !runtime.frames.quarantined()
        && stop_ready
        && selection_ready
        && !tail.is_empty()
    {
        let context = capture(world, &snapshot);
        for edge in tail.iter().copied() {
            if world.resource::<MovementTicker>().has_held_release()
                || world.resource::<ItemUseRuntime>().has_pending_release()
                || !world
                    .resource::<MovementTicker>()
                    .accepts_block_interactions()
                || world.resource::<MeleeRuntime>().has_queued_work()
            {
                break;
            }
            dispatch(
                world,
                InteractionInput {
                    edge,
                    context: context.clone(),
                },
            );
        }
    }
    world.insert_resource(runtime);
}

/// Dispatches one edge without rerunning a frame producer or retaining a failed attempt.
fn dispatch(world: &mut World, input: InteractionInput) {
    let edge = input.edge;
    if edge.action != Action::Use
        && !world
            .run_system_cached_with(
                crate::block_use::ordered::fence,
                edge.action == Action::Attack && edge.pressed,
            )
            .expect("older block stop carrier")
    {
        return;
    }
    match edge.action {
        Action::Attack if edge.pressed => {
            world
                .run_system_cached_with(crate::melee::ordered::press, input)
                .expect("ordered attack admission");
        }
        Action::Use => {
            if world
                .run_system_cached_with(crate::block_use::ordered::press, input.clone())
                .expect("ordered block admission")
            {
                world
                    .run_system_cached_with(crate::item_use::ordered::press, input)
                    .expect("ordered item admission");
            }
        }
        Action::PickItem if edge.pressed => {
            world
                .run_system_cached_with(crate::pick_block::ordered_pick, input)
                .expect("ordered pick admission");
        }
        action
            if edge.pressed && client_interaction::hotbar::slot_for_action(action, 0).is_some() =>
        {
            if world
                .resource::<MovementTicker>()
                .between_ticks_sample()
                .is_some()
            {
                world
                    .run_system_cached_with(crate::hotbar::ordered::select, action)
                    .expect("ordered selection admission");
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests;
