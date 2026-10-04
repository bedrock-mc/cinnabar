//! Hold-to-mine for every game mode: one destroy state-machine step per completed
//! physics tick. Creative and zero-hardness destroys complete on their start tick.
//!
//! Completion, timed from the provisional destroy table, removes the block locally
//! as vanilla's local destroy does; inbound block updates stay authoritative and
//! replace the prediction. Unknown blocks keep cracking until the server breaks them.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use semantic_input::Action;
use sim::{DestroyConditions, HeldTool, PaletteWorld};

use crate::{
    game_mode_capabilities::GameModeCapabilities,
    interaction_authority::{observe_block, within_pick_range},
    local_player::InteractionOriginSnapshot,
    melee::{MeleeRuntime, SwingTracker, swing_duration},
    menu::MenuRuntime,
    mining::{creative_reach, hand_interaction_selection, protocol_input_mode, survival_reach},
    movement::{
        LocalMovementEffectTimeline, LocalPhysicsController, MovementTicker,
        PhysicsCollisionRegistries,
    },
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

pub(crate) use gameplay::survival_mining::{
    BlockBreakingAuthority, DestroyInput, DestroyTarget, ToolWear, blocked_mining_reason,
    destroys_in_creative, exempt_from_airborne_penalty, eyes_in_water, mining_active, percent_roll,
    tool_break_damage, unbreaking_keeps_damage,
};

/// Bevy resource adapter for the gameplay survival_mining owner.
#[derive(Resource, Debug, Default)]
pub(crate) struct SurvivalMiningRuntime(gameplay::survival_mining::SurvivalMiningRuntime);
impl std::ops::Deref for SurvivalMiningRuntime {
    type Target = gameplay::survival_mining::SurvivalMiningRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for SurvivalMiningRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
#[derive(SystemParam)]
pub(crate) struct SurvivalMiningContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    origin: Res<'w, InteractionOriginSnapshot>,
    ui: ResMut<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: ResMut<'w, ClientWorld>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    physics: Option<Res<'w, LocalPhysicsController>>,
    melee: Res<'w, MeleeRuntime>,
    network: Res<'w, NetworkHandle>,
    time: Res<'w, Time<Real>>,
    block_cues: bevy::prelude::MessageWriter<'w, crate::audio::LocalBlockCue>,
}

/// Runs after committed world publication and before the movement flush.
pub(crate) fn produce_survival_mining(
    mut player_runtime: bevy::prelude::ResMut<crate::player_runtime::PlayerRuntime>,
    mut context: SurvivalMiningContext,
    mut runtime: ResMut<SurvivalMiningRuntime>,
    mut swings: ResMut<SwingTracker>,
    mut movement: ResMut<MovementTicker>,
) {
    // Wire sequencing only, defaulted to server-authoritative when the server
    // never negotiated it. This decides HOW a break travels, never WHETHER one
    // may happen; the capability gate below owns that.
    let authority = context
        .ui
        .server_authoritative_block_breaking(&player_runtime)
        .map(BlockBreakingAuthority::from_negotiation);
    let caps = context.ui.game_mode_capabilities(&player_runtime);
    let focused =
        !context.menu.is_visible() && context.windows.single().is_ok_and(|window| window.focused);
    let attack = context.input.phase(Action::Attack);
    let snapshot = context.input.snapshot();
    let snapshot_present = snapshot.is_some();
    let actor_in_front = context.melee.actor_in_front();
    let active = mining_active(caps, focused, snapshot_present);
    let target = match snapshot.filter(|_| active) {
        Some(input) => {
            runtime.latched_press |= attack.pressed;
            (attack.held || runtime.latched_press).then(|| {
                // An actor in front owns the press; the block behind it is not a target.
                (!actor_in_front)
                    .then(|| {
                        observe_destroy_target(
                            &player_runtime,
                            &context,
                            caps,
                            input.input_mode,
                            (input.authority_generation, input.frame_sequence),
                            movement.interaction_authority_identity().1,
                        )
                    })
                    .flatten()
            })
        }
        None => {
            runtime.latched_press = false;
            None
        }
    };
    if (attack.pressed || attack.held)
        && let Some(reason) = blocked_mining_reason(
            caps,
            focused,
            snapshot_present,
            actor_in_front,
            matches!(target, Some(Some(_))),
        )
    {
        let now = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
        runtime.log_blocked_mining(now, reason, caps, authority);
    }
    let input = target.as_ref().map_or(DestroyInput::Released, |target| {
        DestroyInput::Held(target.as_ref())
    });
    let duration = swing_duration(context.effects.mining_effects());
    let local_runtime_id = context
        .client_world
        .stream
        .as_ref()
        .map(|stream| stream.local_player_runtime_id());
    let network = &context.network;
    let ui = &mut context.ui;
    let client_world = &mut context.client_world;
    let unsent = runtime.step_ticks(
        &mut movement,
        input,
        authority.unwrap_or(BlockBreakingAuthority::Server),
        |tick| {
            if let Some(local_runtime_id) = local_runtime_id
                && swings.try_swing(tick, duration)
            {
                let _ = network.send_inventory_packet(protocol::swing_arm_packet(
                    local_runtime_id,
                    protocol::SwingSource::Mine,
                ));
            }
        },
        |slot, damage| ui.begin_mining_request(&mut player_runtime, slot, damage),
        |position| {
            if let Some(stream) = client_world.stream.as_mut() {
                let air = stream.air_block_id();
                stream.predict_block(position, 0, air);
            }
        },
    );
    for request_id in unsent {
        ui.cancel_mining_request(&mut player_runtime, request_id);
    }
    for cue in runtime.take_break_cues() {
        let gameplay::survival_mining::BlockBreakCue::Break {
            position,
            block_runtime_id,
        } = cue;
        context
            .block_cues
            .write(crate::audio::LocalBlockCue::Break {
                position,
                block_runtime_id,
            });
    }
}

fn observe_destroy_target(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    context: &SurvivalMiningContext,
    caps: Option<GameModeCapabilities>,
    input_mode: semantic_input::InputMode,
    input_authority: (std::num::NonZeroU64, u64),
    position_authority_generation: u64,
) -> Option<DestroyTarget> {
    let ui = &context.ui;
    // The capability gate in the producer already confirmed this mode edits;
    // here only an open UI blocks the pick.
    let caps = caps?;
    if ui.ui_focused(player_runtime) {
        return None;
    }
    let selection = hand_interaction_selection(player_runtime, ui)?;
    let input_mode = protocol_input_mode(input_mode);
    let observed = observe_block(
        &context.origin,
        ui,
        &context.client_world,
        &context.collisions,
        selection,
        (
            input_mode,
            if caps.creative_reach {
                creative_reach(input_mode)
            } else {
                survival_reach(input_mode)
            },
            input_authority,
            position_authority_generation,
        ),
    )?;
    if !within_pick_range(&observed) {
        return None;
    }
    let stream = context.client_world.stream.as_ref()?;
    let mode = stream.network_id_mode();
    let block = context
        .collisions
        .block_identifier(mode, observed.target.runtime_id)
        .and_then(sim::block_destroy_info);
    let item = &observed.selection.item;
    let identifier = (item.network_id() != 0)
        .then(|| {
            ui.inventory_ledger(player_runtime)
                .negotiated_item_entry(item.network_id())
        })
        .flatten()
        .map(|entry| entry.identifier.as_ref());
    let instant = caps.instant_break;
    if instant && !destroys_in_creative(identifier) {
        return None;
    }
    let tool = identifier.and_then(HeldTool::from_identifier);
    let world = PaletteWorld::new(
        stream.collision_store(),
        context.collisions.registry(mode),
        stream.current_dimension(),
    );
    let effects = context.effects.mining_effects();
    let helmet = ui.local_armor(player_runtime).helmet;
    let unbreaking =
        protocol::item_enchantment_level(item.extra_data(), UNBREAKING_ENCHANTMENT_ID).unwrap_or(0);
    let wear = tool.filter(|_| !instant).and_then(|tool| {
        (item.stack_network_id() > 0).then(|| ToolWear {
            // Outstanding and corrected predictions outrank the stack's own tag.
            current_damage: ui
                .inventory_ledger(player_runtime)
                .predicted_slot_damage(observed.selection.slot)
                .or_else(|| {
                    protocol::item_extra_damage(item.extra_data())
                        .and_then(|damage| i32::try_from(damage).ok())
                })
                .unwrap_or(0),
            break_damage: if protocol::item_extra_unbreakable(item.extra_data())
                || !unbreaking_keeps_damage(unbreaking, percent_roll())
            {
                0
            } else {
                tool_break_damage(tool.kind)
            },
        })
    });
    Some(DestroyTarget {
        position: observed.target.position,
        face: observed.target.face,
        runtime_id: observed.target.runtime_id,
        relative_hit: observed.target.relative_hit,
        block,
        conditions: DestroyConditions {
            tool,
            efficiency_level: protocol::item_enchantment_level(
                item.extra_data(),
                EFFICIENCY_ENCHANTMENT_ID,
            )
            .unwrap_or(0),
            haste_amplifier: effects.haste,
            conduit_power_amplifier: effects.conduit_power,
            mining_fatigue_amplifier: effects.mining_fatigue,
            on_ground: true,
            flying: exempt_from_airborne_penalty(
                context.physics.as_ref().map(|physics| physics.mode()),
            ),
            riding: ui.gameplay_hud().mount_unique_id().is_some(),
            eyes_in_water: eyes_in_water(&world, observed.ray.origin),
            aqua_affinity: protocol::item_enchantment_level(
                &helmet.extra_data,
                AQUA_AFFINITY_ENCHANTMENT_ID,
            )
            .is_some_and(|level| level > 0),
        },
        selection: observed.selection,
        wear,
        instant,
    })
}

const AQUA_AFFINITY_ENCHANTMENT_ID: i16 = 8;
const EFFICIENCY_ENCHANTMENT_ID: i16 = 15;
const UNBREAKING_ENCHANTMENT_ID: i16 = 17;

impl SurvivalMiningRuntime {
    /// Supplies existing audio and particle adapters with the current destroy target.
    pub(crate) fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.0.destroying_target()
    }
}
