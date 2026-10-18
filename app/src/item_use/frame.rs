//! Shared immutable item-use proposals and post-admission effects.
use super::*;
use client_presentation::local_player::LocalViewPose;

/// Current item-use facts paired with the action's movement pose.
#[derive(Clone)]
pub(crate) struct FrozenAirUse {
    pub frame: UseFrame,
    local_runtime_id: u64,
    duration: i32,
    aim_rotation: Option<bevy::prelude::Quat>,
}

/// Input policy supplied once by the edge or held-use caller.
pub(super) struct FramePolicy {
    pub held: bool,
    pub before_flush: bool,
    pub delay_fix_enabled: bool,
    pub aim_rotation: Option<bevy::prelude::Quat>,
    pub release_facing: gameplay::item_use::RiptideFacing,
}

/// Observes one proposal without executing a held-use tick or sending a packet.
pub(super) fn observe(
    player_runtime: &client_state::PlayerRuntime,
    context: &ItemUseContext,
    runtime: &mut ItemUseRuntime,
    movement: &MovementTicker,
    view: &mut LocalViewPose,
    policy: FramePolicy,
) -> Option<FrozenAirUse> {
    let FramePolicy {
        held,
        before_flush,
        delay_fix_enabled,
        aim_rotation,
        release_facing,
    } = policy;
    let stream = context.client_world.stream.as_ref()?;
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let air_use = selected_air_use_with_projectile(
        player_runtime,
        stream,
        runtime.selected_projectile(player_runtime, &context.ui),
    );
    let creative = player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative);
    let inventory_revision = player_runtime.selected_hotbar_slot().and_then(|slot| {
        context
            .ui
            .inventory_ledger(player_runtime)
            .authoritative_slot_revision(slot)
    });
    let selection = verified_use_selection(player_runtime, &context.ui).map(|server| {
        context
            .block_use
            .inventory
            .predicted_selection(&server, inventory_revision.unwrap_or(0))
            .unwrap_or(server)
    });
    let riptide_at = |position| {
        selected_riptide_use(
            player_runtime,
            stream,
            context
                .physics
                .as_deref()
                .is_some_and(|physics| physics.in_water())
                || riptide::in_rain(
                    stream,
                    position,
                    context
                        .weather
                        .as_deref()
                        .map_or(0.0, |weather| weather.current_rain_level),
                ),
        )
    };
    // The completed local pose remains available while its movement input waits in the FIFO.
    if runtime.is_releasing(held)
        && let Some(edge) = movement.build_action_state()
    {
        let first = runtime.observe_riptide_release(
            edge.tick.saturating_add(1),
            held,
            riptide_at(edge.position),
            selection.as_ref(),
            release_facing,
        );
        if first && let Some(rotation) = aim_rotation {
            view.set_rotation(rotation);
        }
    }
    if movement.has_held_release() {
        return None;
    }
    let sample = if before_flush {
        movement
            .newest_unsent_sample()
            .map(Into::into)
            .or_else(|| movement.between_ticks_sample())
    } else {
        runtime.frame_sample(movement, held, true)
    };
    let sample = sample?;
    // Pending presses suppress new starts while accepted uses keep advancing.
    let press_pending = context.block_use.press_pending()
        || (!delay_fix_enabled && context.melee.press_pending())
        || (context.ordered.is_some() && context.melee.has_queued_work(movement))
        || crate::ordered_interactions::is_blocked(context.ordered.as_deref());
    let frame = UseFrame {
        riptide: riptide_at(sample.position),
        riptide_facing: release_facing,
        tick: sample.tick,
        now_millis,
        position: sample.position,
        held,
        selection,
        air_use,
        ready: match air_use {
            Some(AirUse::Hold { needs, .. }) => {
                creative || needs_met(player_runtime, stream, &context.ui, needs)
            }
            Some(AirUse::GlideRocket) => context.physics.as_deref().is_some_and(|physics| {
                physics.is_active() && physics.mode() == sim::MovementMode::Gliding
            }),
            _ => false,
        },
        creative,
        inventory_revision,
        charge_projectile: loading_projectile(player_runtime, stream, &context.ui, creative),
        press_consumed: before_flush
            || press_pending
            || (!delay_fix_enabled && context.melee.blocks_use_at(now_millis))
            || context.block_use.press_interacted(),
    };
    if let Some(reason) = runtime.press_drop_reason(&frame) {
        gameplay::movement::note_click_drop("use", reason);
    }
    let duration = swing_duration(
        context
            .effects
            .mining_tick(sample.tick, movement.completed_tick())
            .0,
    );
    Some(FrozenAirUse {
        frame,
        local_runtime_id: stream.local_player_runtime_id(),
        duration,
        aim_rotation,
    })
}

/// Commits one proposal through the existing item-use admission and release-fence owners.
pub(crate) fn admit(
    player_runtime: &mut client_state::PlayerRuntime,
    context: &mut ItemUseContext,
    runtime: &mut ItemUseRuntime,
    movement: &mut MovementTicker,
    swings: &mut SwingTracker,
    view: &mut LocalViewPose,
    frozen: &FrozenAirUse,
) {
    admit_with_action_aim(
        runtime,
        swings,
        movement,
        view,
        &frozen.frame,
        frozen.local_runtime_id,
        frozen.duration,
        frozen.aim_rotation,
        &context.network,
    );
    if let Some(launch) = runtime.take_riptide_launch()
        && let Some(physics) = context.physics.as_deref_mut()
    {
        movement.request_riptide(physics, launch.level, launch.rotation);
    }
    if let Some((slot, revision, change)) = runtime.take_inventory_change() {
        player_runtime
            .inventory
            .ledger_mut()
            .settle_use_slot_change(slot, revision, &change);
    }
}
