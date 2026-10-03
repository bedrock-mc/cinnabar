//! Hold-to-mine for every game mode: one destroy state-machine step per completed
//! physics tick. Instant (Creative) destroys complete on their start tick.
//!
//! Completion, timed from the provisional destroy table, removes the block locally
//! as vanilla's local destroy does; inbound block updates stay authoritative and
//! replace the prediction. Unknown blocks keep cracking until the server breaks them.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use protocol::{
    BlockAction, BlockActionKind, BlockActions, BlockItemInteraction, BlockUseRequest,
    PlayerAuthInputInteractions,
};
use semantic_input::Action;
use sim::{BlockDestroyInfo, DestroyConditions, HeldTool, PaletteWorld};

use crate::{
    game_mode_capabilities::GameModeCapabilities,
    interaction_authority::{observe_block, within_pick_range},
    local_player::InteractionOriginSnapshot,
    melee::{MeleeRuntime, SwingTracker, swing_duration},
    menu::MenuRuntime,
    mining::{
        FrozenMiningSelection, creative_reach, hand_interaction_selection, protocol_input_mode,
        survival_reach,
    },
    movement::{
        LocalMovementEffectTimeline, LocalPhysicsController, MovementTicker,
        PhysicsCollisionRegistries,
    },
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

/// Ticks between a completed destroy and the next start. Needs independent measurement.
pub(crate) const DESTROY_DELAY_TICKS: u8 = 5;
/// Progress at which a destroy completes, absorbing float accumulation. Needs
/// independent measurement.
const COMPLETION_THRESHOLD: f64 = 0.99999;
/// Below this speed (blocks/s) a held Creative destroy waits out the delay;
/// above it, it destroys once per block travelled (`GameMode::continueDestroyBlock`).
const CREATIVE_SLOW_SPEED: f32 = 0.5;
const CREATIVE_TRAVEL_PER_DESTROY: f32 = 1.0;
/// Bedrock enchantment ids.
const AQUA_AFFINITY_ENCHANTMENT_ID: i16 = 8;
const EFFICIENCY_ENCHANTMENT_ID: i16 = 15;
const UNBREAKING_ENCHANTMENT_ID: i16 = 17;

/// Which side StartGame's negotiation makes authoritative for block destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockBreakingAuthority {
    /// Progress travels as per-tick block actions; completion is only predicted.
    Server,
    /// Completion travels as an item-use destroy transaction.
    Client,
}

impl BlockBreakingAuthority {
    const fn from_negotiation(server_authoritative: bool) -> Self {
        if server_authoritative {
            Self::Server
        } else {
            Self::Client
        }
    }
}

/// The block under the crosshair and everything its destroy rate depends on.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct DestroyTarget {
    pub(crate) position: [i32; 3],
    pub(crate) face: u8,
    pub(crate) runtime_id: u32,
    pub(crate) relative_hit: [f32; 3],
    pub(crate) block: Option<BlockDestroyInfo>,
    /// Everything except `on_ground`, which is taken from each stepped tick.
    pub(crate) conditions: DestroyConditions,
    pub(crate) selection: FrozenMiningSelection,
    /// The held tool's wear from a non-instant destroy, when it is predictable.
    pub(crate) wear: Option<ToolWear>,
    /// The game mode destroys instantly, as Creative does.
    pub(crate) instant: bool,
}

/// Held-tool damage before a destroy and the damage one destroy adds, after the
/// Unbreakable and Unbreaking rolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ToolWear {
    pub(crate) current_damage: i32,
    pub(crate) break_damage: i32,
}

impl DestroyTarget {
    fn rate(&self, on_ground: bool) -> f64 {
        let conditions = DestroyConditions {
            on_ground,
            ..self.conditions
        };
        self.block
            .as_ref()
            .and_then(|block| sim::destroy_progress_per_tick(block, &conditions))
            .map_or(0.0, f64::from)
    }
}

/// Destroy actions for one tick, plus the client-authoritative completion if any.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct SurvivalTickPayload {
    pub(crate) actions: BlockActions,
    pub(crate) destroy: Option<DestroyTarget>,
    /// Holding attack on a block attempts a mining swing every tick.
    pub(crate) swing: bool,
    /// `(slot, predicted damage, stack network id)` of a worn held tool.
    pub(crate) wear: Option<(u8, i32, i32)>,
    /// The request carrying `wear`, once a request id is allocated.
    pub(crate) mine_block: Option<protocol::MineBlockRequest>,
    /// The block this tick's completion removes locally once the tick carries it.
    pub(crate) broken: Option<[i32; 3]>,
}

impl SurvivalTickPayload {
    pub(crate) fn is_empty(&self) -> bool {
        self.actions.is_empty() && self.destroy.is_none() && self.mine_block.is_none()
    }

    /// Faces, percents, hit offsets and slots are bounded at their sources, so
    /// the carrier's own validation cannot reject the result.
    pub(crate) fn into_interactions(
        self,
        player_position: [f32; 3],
    ) -> (
        PlayerAuthInputInteractions,
        Option<protocol::MineBlockRequest>,
    ) {
        let block_interaction = self.destroy.map(|target| {
            BlockItemInteraction::Destroy(BlockUseRequest {
                block_position: target.position,
                face: target.face,
                selected_slot: target.selection.slot,
                selected_item: target.selection.item,
                player_position,
                relative_hit: target.relative_hit,
                block_runtime_id: u64::from(target.runtime_id),
            })
        });
        let interactions = PlayerAuthInputInteractions {
            block_actions: self.actions,
            block_interaction,
        };
        (interactions, self.mine_block)
    }

    fn push(&mut self, kind: BlockActionKind, position: [i32; 3], face: u8) {
        // A tick emits at most four actions; the bounded list holds eight.
        let _ = self.actions.push(BlockAction {
            kind,
            position,
            face,
        });
    }
}

/// The player's motion over one stepped tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TickMotion {
    pub(crate) on_ground: bool,
    /// Distance moved this tick, in blocks.
    pub(crate) moved: f32,
}

/// Attack input for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum DestroyInput<'a> {
    Released,
    /// Held, aimed at this block or at nothing destroyable.
    Held(Option<&'a DestroyTarget>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Destroying {
    position: [i32; 3],
    face: u8,
    progress: f64,
    /// The block broke; the destroy only waits for its next target.
    completed: bool,
}

impl Destroying {
    fn abort_percent(&self) -> u8 {
        (self.progress * 100.0).clamp(0.0, 100.0) as u8
    }
}

/// Per-tick destroy sequencing.
///
/// Server authority: start, then silence while cracking, a continue on a new
/// block, and continue plus predict on completion; aborts carry progress.
/// Client authority: start, a crack every tick, and a stop plus item-use
/// destroy transaction on completion. An instant destroy completes on its start
/// tick with the same completion actions, then repeats while held.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct DestroyMachine {
    destroying: Option<Destroying>,
    delay: u8,
    pending_abort: Option<([i32; 3], u8)>,
    /// Blocks travelled toward the next moving Creative destroy.
    travel: f32,
}

impl DestroyMachine {
    /// The block and face currently cracking; none once it broke, so hit
    /// sounds and particles stop with the break.
    pub(crate) fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.destroying
            .filter(|destroying| !destroying.completed)
            .map(|destroying| (destroying.position, destroying.face))
    }

    /// Forgets unsent progress; an in-flight destroy is aborted on the next step.
    pub(crate) fn interrupt(&mut self) {
        if let Some(destroying) = self.destroying.take() {
            self.pending_abort = Some((destroying.position, destroying.abort_percent()));
        }
    }

    pub(crate) fn step(
        &mut self,
        input: DestroyInput<'_>,
        motion: TickMotion,
        authority: BlockBreakingAuthority,
    ) -> SurvivalTickPayload {
        let on_ground = motion.on_ground;
        let mut payload = SurvivalTickPayload::default();
        if let Some((position, percent)) = self.pending_abort.take() {
            payload.push(BlockActionKind::AbortDestroy, position, percent);
        }
        let delayed = self.delay > 0;
        self.delay = self.delay.saturating_sub(1);
        let target = match input {
            DestroyInput::Held(Some(target)) => {
                payload.swing = true;
                target
            }
            DestroyInput::Released | DestroyInput::Held(None) => {
                if input == DestroyInput::Released {
                    // stopDestroyBlock clears the destroy delay.
                    self.delay = 0;
                }
                if let Some(destroying) = self.destroying.take() {
                    payload.push(
                        BlockActionKind::AbortDestroy,
                        destroying.position,
                        destroying.abort_percent(),
                    );
                }
                return payload;
            }
        };
        let creative_continue = target.instant && self.destroying.is_some();
        if delayed && !creative_continue {
            return payload;
        }
        match self.destroying {
            None => {
                payload.push(BlockActionKind::StartDestroy, target.position, target.face);
                self.destroying = Some(Destroying {
                    position: target.position,
                    face: target.face,
                    progress: 0.0,
                    completed: false,
                });
                // Only zero hardness breaks on the start tick, then delays.
                if target.instant || target.block.is_some_and(|block| block.hardness == 0.0) {
                    self.complete(&mut payload, target, authority, false);
                    self.delay = DESTROY_DELAY_TICKS;
                    self.travel = 0.0;
                } else if authority == BlockBreakingAuthority::Client {
                    payload.push(BlockActionKind::CrackBlock, target.position, target.face);
                }
            }
            Some(_) if target.instant => {
                let slow = motion.moved * sim::TICKS_PER_SECOND as f32 <= CREATIVE_SLOW_SPEED;
                if !slow {
                    self.travel += motion.moved;
                }
                if slow && !delayed {
                    self.travel = 0.0;
                } else if self.travel > CREATIVE_TRAVEL_PER_DESTROY {
                    self.travel -= self.travel.trunc();
                } else {
                    return payload;
                }
                self.complete(&mut payload, target, authority, true);
                self.delay = DESTROY_DELAY_TICKS;
            }
            Some(destroying) if destroying.position == target.position => {
                let rate = target.rate(on_ground);
                let progress = destroying.progress + rate;
                if progress >= COMPLETION_THRESHOLD {
                    self.complete(&mut payload, target, authority, true);
                    self.delay = if rate < 1.0 { DESTROY_DELAY_TICKS } else { 0 };
                } else {
                    self.destroying = Some(Destroying {
                        progress,
                        completed: false,
                        ..destroying
                    });
                    if authority == BlockBreakingAuthority::Client {
                        payload.push(BlockActionKind::CrackBlock, target.position, target.face);
                    }
                }
            }
            Some(destroying) => {
                match authority {
                    BlockBreakingAuthority::Server => payload.push(
                        BlockActionKind::ContinueDestroy,
                        target.position,
                        target.face,
                    ),
                    BlockBreakingAuthority::Client => {
                        payload.push(
                            BlockActionKind::AbortDestroy,
                            destroying.position,
                            destroying.abort_percent(),
                        );
                        payload.push(BlockActionKind::StartDestroy, target.position, target.face);
                        payload.push(BlockActionKind::CrackBlock, target.position, target.face);
                    }
                }
                self.destroying = Some(Destroying {
                    position: target.position,
                    face: target.face,
                    progress: 0.0,
                    completed: false,
                });
            }
        }
        payload
    }

    /// The destroy stays active on the broken block, so the next target continues it.
    /// A server-authoritative destroy of a block with hardness wears the tool.
    fn complete(
        &mut self,
        payload: &mut SurvivalTickPayload,
        target: &DestroyTarget,
        authority: BlockBreakingAuthority,
        continued: bool,
    ) {
        match authority {
            BlockBreakingAuthority::Server => {
                let worn = target.block.is_some_and(|block| block.hardness > 0.0);
                payload.wear = target.wear.filter(|_| worn).map(|wear| {
                    (
                        target.selection.slot,
                        wear.current_damage.saturating_add(wear.break_damage),
                        target.selection.item.stack_network_id(),
                    )
                });
                if continued {
                    payload.push(
                        BlockActionKind::ContinueDestroy,
                        target.position,
                        target.face,
                    );
                }
                payload.push(
                    BlockActionKind::PredictDestroy,
                    target.position,
                    target.face,
                );
            }
            BlockBreakingAuthority::Client => {
                payload.push(BlockActionKind::StopDestroy, [0; 3], 0);
                payload.destroy = Some(target.clone());
            }
        }
        self.destroying = Some(Destroying {
            position: target.position,
            face: target.face,
            progress: 0.0,
            completed: true,
        });
        payload.broken = Some(target.position);
    }
}

/// Wall-clock spacing between block-diagnostic lines while an attack is held.
const BLOCKED_MINING_LOG_THROTTLE_MILLIS: u64 = 2000;

/// Destroy sequencing bound to the current position authority.
#[derive(Resource, Debug, Default)]
pub(crate) struct SurvivalMiningRuntime {
    machine: DestroyMachine,
    last_stepped_tick: Option<u64>,
    latched_press: bool,
    position_authority: Option<(u64, u64)>,
    last_blocked_log_millis: Option<u64>,
}

impl SurvivalMiningRuntime {
    /// The block and face the local player is breaking, for hit particles.
    pub(crate) fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.machine.destroying_target()
    }

    /// Steps every unsent tick once, attaching nonempty payloads to their
    /// samples, and returns the mining request ids no tick carried. A break is
    /// predicted only once its tick carries it.
    pub(crate) fn step_ticks(
        &mut self,
        ticker: &mut MovementTicker,
        input: DestroyInput<'_>,
        authority: BlockBreakingAuthority,
        mut swing: impl FnMut(u64),
        mut request_id: impl FnMut(u8, i32) -> Option<i32>,
        mut predict_break: impl FnMut([i32; 3]),
    ) -> Vec<i32> {
        let mut unsent = Vec::new();
        let identity = ticker.interaction_authority_identity();
        if let Some((session, _)) = self
            .position_authority
            .filter(|previous| *previous != identity)
        {
            // Reanchors clear the outbox and may rewind tick numbers.
            self.last_stepped_tick = None;
            if session == identity.0 {
                self.machine.interrupt();
            } else {
                self.machine = DestroyMachine::default();
            }
        }
        self.position_authority = Some(identity);
        let ticks = ticker.unstepped_interaction_ticks(self.last_stepped_tick);
        let Some(&(newest, _)) = ticks.last() else {
            return unsent;
        };
        self.last_stepped_tick = Some(newest);
        if !ticker.accepts_block_interactions() {
            // Withheld ticks never reach the server, so neither may their actions.
            self.machine.interrupt();
            return unsent;
        }
        for (tick, motion) in ticks {
            let mut payload = self.machine.step(input, motion, authority);
            payload.mine_block = payload
                .wear
                .filter(|&(slot, damage, stack_network_id)| {
                    slot <= 8 && damage >= 0 && stack_network_id > 0
                })
                .and_then(|(slot, damage, stack_network_id)| {
                    protocol::MineBlockRequest::new(
                        request_id(slot, damage)?,
                        slot,
                        damage,
                        stack_network_id,
                    )
                    .ok()
                });
            self.latched_press = false;
            if payload.swing {
                swing(tick);
            }
            let mine_block = payload
                .mine_block
                .as_ref()
                .map(|request| request.request_id());
            let broken = payload.broken;
            if payload.is_empty() {
                continue;
            }
            if ticker.attach_survival_mining(tick, payload) {
                broken.into_iter().for_each(&mut predict_break);
                if broken.is_some() {
                    break;
                }
            } else {
                // A tick that cannot carry its actions desynchronizes the server's view.
                self.machine.interrupt();
                unsent.extend(mine_block);
            }
        }
        unsent
    }

    /// Emits one throttled line naming the gate that blocked a held-attack break.
    fn log_blocked_mining(
        &mut self,
        now_millis: u64,
        reason: &'static str,
        caps: Option<GameModeCapabilities>,
        authority: Option<BlockBreakingAuthority>,
    ) {
        let due = self.last_blocked_log_millis.is_none_or(|last| {
            now_millis.saturating_sub(last) >= BLOCKED_MINING_LOG_THROTTLE_MILLIS
        });
        if !due {
            return;
        }
        self.last_blocked_log_millis = Some(now_millis);
        bevy::log::debug!(
            target: "bedrock_client::survival_mining",
            reason,
            game_mode_known = caps.is_some(),
            can_mine = caps.is_some_and(|caps| caps.can_mine),
            authority = ?authority,
            "held attack produced no block break",
        );
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
}

/// Whether held-mining should look for a destroy target this frame. The
/// block-breaking wire mode is deliberately not an input here: it sequences a
/// break, it never decides whether one may happen.
fn mining_active(
    caps: Option<GameModeCapabilities>,
    focused: bool,
    snapshot_present: bool,
) -> bool {
    focused && snapshot_present && caps.is_some_and(|caps| caps.can_mine)
}

/// Why a held attack yielded no destroy target, for the throttled diagnostic.
fn blocked_mining_reason(
    caps: Option<GameModeCapabilities>,
    focused: bool,
    snapshot_present: bool,
    actor_in_front: bool,
    target_found: bool,
) -> Option<&'static str> {
    match caps {
        None => Some("game mode unknown"),
        Some(caps) if !caps.can_mine => Some("can_mine=false for this game mode"),
        _ if !focused => Some("window or menu not focused"),
        _ if !snapshot_present => Some("no input snapshot yet"),
        _ if actor_in_front => Some("an actor in front owns the press"),
        _ if !target_found => Some("no breakable block in reach"),
        _ => None,
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

/// Swords and the trident refuse Creative destruction, as `WeaponItem` and
/// `TridentItem::canDestroyInCreative` do; unknown items may destroy.
fn destroys_in_creative(identifier: Option<&str>) -> bool {
    identifier.is_none_or(|identifier| {
        identifier != "minecraft:trident"
            && HeldTool::from_identifier(identifier)
                .is_none_or(|tool| tool.kind != sim::ToolKind::Sword)
    })
}

/// Flight, not mere airborne movement, exempts the destroy-speed penalty (`PlayerDestroy`).
fn exempt_from_airborne_penalty(mode: Option<sim::MovementMode>) -> bool {
    mode == Some(sim::MovementMode::Flying)
}

/// Whether Unbreaking at `level` lets a `roll` in `0..100` damage the item:
/// `ItemStackBase::hurtAndBreak` keeps damage below `Item::getDamageChance`.
fn unbreaking_keeps_damage(level: u8, roll: u32) -> bool {
    level == 0 || roll < 100 / (u32::from(level) + 1)
}

/// A uniformly distributed roll in `0..100` from the process hash seed.
fn percent_roll() -> u32 {
    use std::hash::BuildHasher;
    (std::collections::hash_map::RandomState::new().hash_one(()) % 100) as u32
}

/// Durability one destroy costs, per dragonfly's `item/*.go` durability info (MIT).
const fn tool_break_damage(kind: sim::ToolKind) -> i32 {
    match kind {
        sim::ToolKind::Sword => 2,
        _ => 1,
    }
}

/// Unreadable eye blocks count as submerged, which only slows prediction.
fn eyes_in_water(world: &PaletteWorld<'_>, eye: [f32; 3]) -> bool {
    use sim::CollisionWorld;
    let block = eye.map(|axis| axis.floor() as i32);
    world.block_physics(block).map_or(true, |sample| {
        sample.layers.iter().any(|layer| {
            layer.flags.contains(sim::BlockPhysicsFlags::WATER)
                && f64::from(eye[1]) < f64::from(block[1]) + layer.fluid_height_blocks
        })
    })
}

#[cfg(test)]
pub(crate) mod tests;
