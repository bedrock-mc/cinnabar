//! Immediate build preparation shared by ordered presses and held placement.

use super::*;
use gameplay::{
    mining::FrozenMiningSelection,
    movement::{MiningEffects, UnsentSampleView},
};

/// Current target and movement facts for one immediate build call.
pub(crate) struct BuildInput {
    observed: FrozenBlockObservation,
    pub state: UnsentSampleView,
    pub trigger: ItemUseTrigger,
    pub due: std::time::Duration,
    pub clock: RepeatClock,
    view: LocalViewPose,
}

/// A proposal freezes selection, placement and inventory effects before its first send.
struct BuildPlan {
    server_selection: FrozenMiningSelection,
    inventory_revision: u64,
    local_use: LocalUse,
    destination: [i32; 3],
    predicted: Option<Vec<([i32; 3], u32, u32)>>,
    local_runtime_id: u64,
    swing_effects: MiningEffects,
    block_network_id: u32,
    change: Option<protocol::PredictedSlotChange>,
}

impl BuildInput {
    /// Groups the current press facts for shared prediction and transport admission.
    pub(super) fn new(
        observed: FrozenBlockObservation,
        state: UnsentSampleView,
        trigger: ItemUseTrigger,
        due: std::time::Duration,
        clock: RepeatClock,
        view: LocalViewPose,
    ) -> Self {
        Self {
            observed,
            state,
            trigger,
            due,
            clock,
            view,
        }
    }
}

/// Computes a proposal without sending packets, committing inventory or changing the world.
fn prepare(
    player_runtime: &client_state::PlayerRuntime,
    context: &BlockUseContext,
    runtime: &mut BlockUseRuntime,
    item_use: &mut client_interaction::item_use::ItemUseRuntime,
    movement: &MovementTicker,
    attempt: &mut BuildInput,
) -> Option<sim::CollisionQuery<BuildPlan>> {
    let mut observed = attempt.observed.clone();
    let state = &attempt.state;
    let tick = state.tick.saturating_add(1);
    let game_mode = player_runtime.facts.player_game_mode();
    let caps = player_runtime.facts.game_mode_capabilities()?;
    let stream = context.client_world.stream.as_ref()?;
    let server_selection = observed.selection.clone();
    let inventory_revision = context
        .ui
        .inventory_ledger(player_runtime)
        .authoritative_slot_revision(server_selection.slot)
        .unwrap_or(0);
    observed.selection = runtime
        .inventory
        .selection(&server_selection, inventory_revision);
    let caps = caps.for_edit(stream.authority().held_edit_item(
        &observed.selection.item,
        protocol::ItemBlockList::CanPlaceOn,
        observed.target.runtime_id,
    ));
    let prepared = PaletteWorld::new(
        stream.collision_store(),
        context.collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
        Default::default(),
    )
    .observe(|world| {
        let surroundings =
            use_surroundings(context, world, &observed, state.position, state.sneaking);
        let mut local_use = LocalUse::resolve(
            &observed.selection.item,
            observed.target.position,
            observed.target.face,
            &surroundings,
            &caps,
        );
        let placement = (caps.can_build && local_use != LocalUse::Interact)
            .then(|| {
                predicted_placement(
                    &context.collisions,
                    stream,
                    world,
                    observed.selection.item.block_runtime_id(),
                    &observed,
                    &surroundings,
                    attempt.view,
                )
            })
            .flatten();
        if placement.is_some() && observed.selection.item.count() > 0 {
            local_use = LocalUse::Place;
        }
        let destination = placement.as_ref().map_or_else(
            || {
                surroundings
                    .destination(observed.target.position, observed.target.face)
                    .0
            },
            |placement| placement.position,
        );
        let predicted = placement
            .filter(|_| local_use == LocalUse::Place)
            .map(|placement| {
                std::iter::once((placement.position, 0, placement.block))
                    .chain(
                        placement
                            .additional
                            .into_iter()
                            .map(|(position, block)| (position, 0, block)),
                    )
                    .collect::<Vec<_>>()
            });
        let predicted = predicted.or_else(|| {
            (local_use == LocalUse::Interact)
                .then(|| {
                    predicted_toggle(
                        &context.collisions,
                        stream,
                        world,
                        observed.target.position,
                        observed.target.runtime_id,
                    )
                })
                .flatten()
                .map(|(position, block)| vec![(position, 0, block)])
        });
        for (position, _, _) in predicted.iter().flatten() {
            world.primary_runtime_id(*position).ok()?;
        }
        Some((local_use, destination, predicted))
    })
    .ok()?;
    let (local_use, destination, predicted) = prepared.value?;
    let identity = observed.ray.world_identity.merge(&prepared.identity).ok()?;
    let local_runtime_id = stream.local_player_runtime_id();
    // The tick has not simulated yet, so its swing reads the effects in force before it.
    let swing_effects = context
        .effects
        .mining_tick(tick, movement.completed_tick())
        .0;
    let block_network_id = stream.block_network_id(observed.target.runtime_id)?;
    item_use.synchronize(context.ui.session_id());
    let legacy_request_id = protocol::next_legacy_request_id()?;
    let change = (local_use == LocalUse::Place && game_mode != Some(PlayerGameMode::Creative))
        .then(|| {
            runtime
                .inventory
                .prepare_change(&observed.selection, legacy_request_id)
        });
    attempt.observed = observed;
    Some(sim::CollisionQuery {
        identity,
        value: BuildPlan {
            server_selection,
            inventory_revision,
            local_use,
            destination,
            predicted,
            local_runtime_id,
            swing_effects,
            block_network_id,
            change,
        },
    })
}

/// Prepares one build and commits its effects only after successful transport admission.
pub(crate) fn advance(
    player_runtime: &mut client_state::PlayerRuntime,
    context: &mut BlockUseContext,
    runtime: &mut BlockUseRuntime,
    item_use: &mut client_interaction::item_use::ItemUseRuntime,
    swings: &mut SwingTracker,
    movement: &MovementTicker,
    attempt: &mut BuildInput,
) -> bool {
    let Some(plan) = prepare(
        player_runtime,
        context,
        runtime,
        item_use,
        movement,
        attempt,
    ) else {
        return false;
    };
    if !crate::interaction_authority::world_identity_current(
        &context.client_world,
        &context.collisions,
        &plan.identity,
    ) {
        return false;
    }
    let plan = &plan.value;
    let observed = &attempt.observed;
    let state = &attempt.state;
    let trigger = attempt.trigger;
    let due = attempt.due;
    let clock = attempt.clock;
    let tick = state.tick.saturating_add(1);
    let Some(stream) = context.client_world.stream.as_ref() else {
        return false;
    };
    let bucket = stream
        .authority()
        .item_identifier(observed.selection.item.network_id())
        .as_deref()
        .is_some_and(|identifier| identifier.ends_with("bucket"));
    if trigger == ItemUseTrigger::SimulationTick
        && !gameplay::block_use::repeats_while_held(
            &observed.selection.item,
            bucket,
            plan.local_use,
        )
    {
        runtime.record(trigger, due, tick, LocalUse::Nothing, clock);
        return false;
    }
    if !runtime.may_attempt(tick, plan.local_use, swings) {
        return false;
    }
    let start_destination = runtime
        .last_success_destination()
        .is_none()
        .then_some(plan.destination);
    let mut before_swing = swings.clone();
    let packets = use_packets(
        (observed, plan.block_network_id),
        state.position,
        trigger,
        plan.local_use,
        start_destination,
        plan.change.clone(),
        plan.local_runtime_id,
        |tick| swings.try_swing_before_tick(tick, plan.swing_effects),
        tick,
    );
    let result = (!packets.is_empty()).then(|| context.network.send_inventory_packets(packets));
    let sent = matches!(result, Some(Ok(())));
    if sent {
        runtime.intention.record(
            trigger == ItemUseTrigger::SimulationTick,
            plan.destination,
            plan.local_use,
            plan.local_use != LocalUse::Interact
                && held_block_store_id(stream, observed.selection.item.block_runtime_id())
                    .is_some_and(|block| {
                        context
                            .collisions
                            .block_has_build_intention(stream.network_id_mode(), block)
                    }),
            state.sneaking,
            observed.target.hit_world,
        );
    }
    if !runtime.admit(trigger, due, tick, plan.local_use, clock, sent) {
        runtime.refuse_transport(
            tick,
            plan.local_use,
            matches!(result, Some(Err(client_session::BatchSendError::Full))),
        );
        before_swing.defer_unadmitted_attempt(swings);
        *swings = before_swing;
        return false;
    }
    if let Some(change) = &plan.change {
        player_runtime
            .inventory
            .ledger_mut()
            .settle_use_slot_change(plan.server_selection.slot, plan.inventory_revision, change);
        runtime.inventory.commit(
            plan.server_selection.clone(),
            plan.inventory_revision,
            change.clone(),
        );
    }
    if plan.local_use == LocalUse::Place {
        let position = plan.destination;
        context
            .audio_cues
            .write(client_audio::audio::LocalBlockCue::Place {
                position,
                block_runtime_id: stream.resolve_block_network_id(u32::from_ne_bytes(
                    observed.selection.item.block_runtime_id().to_ne_bytes(),
                )) as i32,
            });
    }
    // Java re-equips the held item after each placement.
    if plan.local_use == LocalUse::Place
        && let Some(stream) = context.client_world.stream.as_mut()
    {
        stream.reset_local_java_equip();
    }
    // Vanilla places locally as it sends; a correction replaces the prediction.
    if let (true, Some(cells), Some(stream)) = (
        sent,
        plan.predicted.as_ref(),
        context.client_world.stream.as_mut(),
    ) {
        let applied = stream.predict_blocks(cells);
        if applied {
            runtime.prediction_frames.committed(
                context.frame.0,
                cells
                    .iter()
                    .filter_map(|cell| stream.prediction_generation(cell.0)),
            );
        }
        bevy::log::debug!(?cells, applied, "local block prediction committed");
    }
    true
}
