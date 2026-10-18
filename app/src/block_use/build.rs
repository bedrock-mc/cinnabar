//! Frozen build proposals share the same prediction and transport commit as held placement.

use super::*;
use gameplay::{
    mining::FrozenMiningSelection,
    movement::{MiningEffects, UnsentSampleView},
};

/// One press/repeat's observed target and pre-tick pose, retained across transport refusal.
pub(crate) struct FrozenBuild {
    observed: FrozenBlockObservation,
    plan: Option<sim::CollisionQuery<BuildPlan>>,
    pub state: UnsentSampleView,
    pub trigger: ItemUseTrigger,
    pub due: std::time::Duration,
    pub clock: RepeatClock,
    view: LocalViewPose,
}

/// A proposal freezes selection, placement and inventory effects before its first send.
struct BuildPlan {
    permissions: UsePermissions,
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

impl FrozenBuild {
    /// Validates the complete prepared world observation without changing the frozen target.
    pub(super) fn current(
        &self,
        player: &client_state::PlayerRuntime,
        context: &BlockUseContext,
    ) -> bool {
        let identity = self
            .plan
            .as_ref()
            .map_or(&self.observed.ray.world_identity, |plan| &plan.identity);
        crate::interaction_authority::world_identity_current(
            &context.client_world,
            &context.collisions,
            identity,
        ) && self.plan.as_ref().is_none_or(|plan| {
            UsePermissions::capture(player).as_ref() == Some(&plan.value.permissions)
                && self.placement_current(player, context, &plan.value)
        })
    }

    /// Checks the original placement against current actor boxes without replacing its proposal.
    fn placement_current(
        &self,
        player: &client_state::PlayerRuntime,
        context: &BlockUseContext,
        plan: &BuildPlan,
    ) -> bool {
        if plan.local_use != LocalUse::Place {
            return true;
        }
        let Some(stream) = context.client_world.stream.as_ref() else {
            return false;
        };
        let world = PaletteWorld::new(
            stream.collision_store(),
            context.collisions.registry(stream.network_id_mode()),
            stream.current_dimension(),
            Default::default(),
        );
        let surroundings = use_surroundings(
            context,
            &world,
            &self.observed,
            self.state.position,
            self.state.sneaking,
        );
        if let Some(placement) = predicted_placement(
            &context.collisions,
            stream,
            &world,
            self.observed.selection.item.block_runtime_id(),
            &self.observed,
            &surroundings,
            self.view,
        ) {
            plan.predicted.as_ref().is_some_and(|cells| {
                std::iter::once((placement.position, 0, placement.block))
                    .chain(
                        placement
                            .additional
                            .into_iter()
                            .map(|(position, block)| (position, 0, block)),
                    )
                    .eq(cells.iter().copied())
            })
        } else {
            plan.predicted.is_none()
                && player.facts.game_mode_capabilities().is_some_and(|caps| {
                    LocalUse::resolve(
                        &self.observed.selection.item,
                        self.observed.target.position,
                        self.observed.target.face,
                        &surroundings,
                        &caps,
                    ) == LocalUse::Place
                })
        }
    }

    /// Captures immutable press facts; packet and local prediction preparation happens once.
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
            plan: None,
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
    attempt: &mut FrozenBuild,
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
                .then(|| predicted_toggle(&context.collisions, stream, observed.target.runtime_id))
                .flatten()
                .map(|block| vec![(observed.target.position, 0, block)])
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
            permissions: UsePermissions::capture(player_runtime)?,
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

/// Sends one frozen proposal and commits its effects only after successful transport admission.
pub(crate) fn advance(
    player_runtime: &mut client_state::PlayerRuntime,
    context: &mut BlockUseContext,
    runtime: &mut BlockUseRuntime,
    item_use: &mut client_interaction::item_use::ItemUseRuntime,
    swings: &mut SwingTracker,
    movement: &MovementTicker,
    attempt: &mut FrozenBuild,
) -> bool {
    if attempt.plan.is_none() {
        attempt.plan = prepare(
            player_runtime,
            context,
            runtime,
            item_use,
            movement,
            attempt,
        );
    }
    let Some(plan) = attempt.plan.as_ref() else {
        return false;
    };
    if !attempt.current(player_runtime, context) {
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
