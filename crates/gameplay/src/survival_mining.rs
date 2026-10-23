//! Per-tick mining state, destruction prediction and interaction attachment.
//!
//! Held mining continues destruction on each movement tick. The provisional
//! destroy timings below keep their existing status after extraction.
use crate::{mining::FrozenMiningSelection, movement::MovementTicker};
use client_world::game_mode_capabilities::GameModeCapabilities;
use protocol::{
    BlockAction, BlockActionKind, BlockActions, BlockItemInteraction, BlockUseRequest,
    PlayerAuthInputInteractions,
};
use sim::{BlockDestroyInfo, DestroyConditions, PaletteWorld};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

/// Local effects emitted after a destroy was attached to its movement tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BlockBreakCue {
    Break {
        position: [i32; 3],
        block_runtime_id: i32,
    },
}

/// Ticks between a completed destroy and the next start. Needs independent measurement.
pub const DESTROY_DELAY_TICKS: u8 = 5;
/// Progress at which a non-instant destroy completes.
const COMPLETION_THRESHOLD: f32 = 0.99999;
/// Below this speed (blocks/s) a held Creative destroy waits out the delay;
/// above it, it destroys once per block travelled.
const CREATIVE_SLOW_SPEED: f32 = 0.5;
const CREATIVE_TRAVEL_PER_DESTROY: f32 = 1.0;

/// Which side StartGame's negotiation makes authoritative for block destruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockBreakingAuthority {
    /// Progress travels as per-tick block actions; completion is only predicted.
    Server,
    /// Completion travels as an item-use destroy transaction.
    Client,
}

impl BlockBreakingAuthority {
    pub const fn from_negotiation(server_authoritative: bool) -> Self {
        if server_authoritative {
            Self::Server
        } else {
            Self::Client
        }
    }
}

/// The block under the crosshair and everything its destroy rate depends on.
#[derive(Debug, Clone, PartialEq)]
pub struct DestroyTarget {
    /// Actor identity captured with the target and retained through send retries.
    pub local_runtime_id: u64,
    pub position: [i32; 3],
    pub face: u8,
    pub runtime_id: u32,
    /// Session wire identity frozen with the observed block.
    pub wire_runtime_id: u32,
    pub relative_hit: [f32; 3],
    pub block: Option<BlockDestroyInfo>,
    /// Everything except `on_ground`, which is taken from each stepped tick.
    pub conditions: DestroyConditions,
    pub selection: FrozenMiningSelection,
    /// The held tool's wear from a non-instant destroy, when it is predictable.
    pub wear: Option<ToolWear>,
    /// The game mode destroys instantly, as Creative does.
    pub instant: bool,
}

/// Held-tool damage before a destroy and the damage one destroy adds, after the
/// Unbreakable and Unbreaking rolls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolWear {
    pub current_damage: i32,
    pub break_damage: i32,
}

/// The standalone `PlayerAction` emitted for an instant creative destroy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CreativeDestroyBlock {
    pub local_runtime_id: u64,
    pub position: [i32; 3],
    pub face: u8,
}

impl DestroyTarget {
    /// Computes the current per-tick progress in the client's float precision.
    fn rate(&self, on_ground: bool) -> f32 {
        let conditions = DestroyConditions {
            on_ground,
            ..self.conditions
        };
        self.block
            .as_ref()
            .and_then(|block| sim::destroy_progress_per_tick(block, &conditions))
            .unwrap_or(0.0)
    }
}

/// Destroy actions for one tick, plus the client-authoritative completion if any.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SurvivalTickPayload {
    pub actions: BlockActions,
    pub destroy: Option<DestroyTarget>,
    /// The standalone creative action that belongs to this completed destroy.
    pub creative_destroy: Option<CreativeDestroyBlock>,
    /// Holding attack on a block attempts a mining swing every tick.
    pub swing: bool,
    /// `(slot, predicted damage, stack network id)` of a worn held tool.
    pub wear: Option<(u8, i32, i32)>,
    /// The request carrying `wear`, once a request id is allocated.
    pub mine_block: Option<protocol::MineBlockRequest>,
    /// The block this tick's completion removes locally once the tick carries it.
    pub broken: Option<[i32; 3]>,
}

impl SurvivalTickPayload {
    pub fn is_empty(&self) -> bool {
        self.actions.is_empty()
            && self.destroy.is_none()
            && self.creative_destroy.is_none()
            && self.mine_block.is_none()
    }

    /// Faces, percents, hit offsets and slots are bounded at their sources, so
    /// the carrier's own validation cannot reject the result.
    pub fn into_interactions(
        self,
        player_position: [f32; 3],
    ) -> (
        PlayerAuthInputInteractions,
        Option<protocol::MineBlockRequest>,
    ) {
        let block_interaction = self.destroy.map(|target| {
            BlockItemInteraction::Destroy(BlockUseRequest {
                hand: protocol::ItemUseHand::Mainhand,
                block_position: target.position,
                face: target.face,
                selected_slot: target.selection.slot,
                selected_item: target.selection.item,
                player_position,
                relative_hit: target.relative_hit,
                block_runtime_id: u64::from(target.wire_runtime_id),
            })
        });
        let interactions = PlayerAuthInputInteractions {
            block_actions: self.actions,
            block_interaction,
        };
        (interactions, self.mine_block)
    }

    /// Allocates the inventory request for valid tool wear when a tick can carry it.
    pub(crate) fn assign_request(&mut self, request_id: &mut impl FnMut(u8, i32) -> Option<i32>) {
        self.mine_block = self
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
pub struct TickMotion {
    pub on_ground: bool,
    /// Distance moved this tick, in blocks.
    pub moved: f32,
}

/// Attack input for one tick.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DestroyInput<'a> {
    Released,
    /// Held, aimed at this block or at nothing destroyable.
    Held(Option<&'a DestroyTarget>),
    /// Held input may continue the current block, but cannot start or retarget mining.
    Continue(Option<&'a DestroyTarget>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Destroying {
    position: [i32; 3],
    face: u8,
    progress: f32,
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
/// destroy transaction on completion. Instant creative destroys also carry the
/// standalone creative player action on both authority paths.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DestroyMachine {
    destroying: Option<Destroying>,
    delay: u8,
    pending_abort: Option<([i32; 3], u8)>,
    /// Blocks travelled toward the next moving Creative destroy.
    travel: f32,
}

impl DestroyMachine {
    /// The block and face currently cracking; none once it broke, so hit
    /// sounds and particles stop with the break.
    pub fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.destroying
            .filter(|destroying| !destroying.completed)
            .map(|destroying| (destroying.position, destroying.face))
    }

    /// Forgets unsent progress; an in-flight destroy is aborted on the next step.
    pub fn interrupt(&mut self) {
        if let Some(destroying) = self.destroying.take() {
            self.pending_abort = Some((destroying.position, destroying.abort_percent()));
        }
    }

    pub fn step(
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
            DestroyInput::Held(Some(target)) | DestroyInput::Continue(Some(target))
                if !matches!(input, DestroyInput::Continue(_))
                    || self
                        .destroying_target()
                        .is_some_and(|(position, _)| position == target.position) =>
            {
                payload.swing = true;
                target
            }
            DestroyInput::Released | DestroyInput::Held(_) | DestroyInput::Continue(_) => {
                if input == DestroyInput::Released {
                    // Releasing clears the destroy delay, as in vanilla.
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
                let slow =
                    motion.moved * simulation_clock::TICKS_PER_SECOND as f32 <= CREATIVE_SLOW_SPEED;
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
        if target.instant {
            payload.creative_destroy = Some(CreativeDestroyBlock {
                local_runtime_id: target.local_runtime_id,
                position: target.position,
                face: target.face,
            });
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
#[derive(Debug, Default)]
pub struct SurvivalMiningRuntime {
    machine: DestroyMachine,
    last_stepped_tick: Option<u64>,
    position_authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    last_blocked_log_millis: Option<u64>,
    break_cues: Vec<BlockBreakCue>,
}

impl SurvivalMiningRuntime {
    /// The block and face the local player is breaking, for hit particles.
    pub fn destroying_target(&self) -> Option<([i32; 3], u8)> {
        self.machine.destroying_target()
    }

    /// Takes local destroy effects after their block actions were attached to a tick.
    pub fn take_break_cues(&mut self) -> Vec<BlockBreakCue> {
        std::mem::take(&mut self.break_cues)
    }

    /// Steps every unsent tick once and retains its destroy work on that sample,
    /// including silent progress or delay, so later presses cannot precede it.
    /// Returns requests no tick carried; a break is predicted only after attachment.
    pub fn step_ticks(
        &mut self,
        ticker: &mut MovementTicker,
        input: DestroyInput<'_>,
        authority: BlockBreakingAuthority,
        mut swing: impl FnMut(u64),
        mut request_id: impl FnMut(u8, i32) -> Option<i32>,
        mut predict_break: impl FnMut([i32; 3]),
    ) -> Vec<i32> {
        let mut unsent = Vec::new();
        self.synchronize(ticker.interaction_authority_identity());
        let ticks = ticker.unstepped_interaction_ticks(self.last_stepped_tick);
        let Some(&(newest, _)) = ticks.last() else {
            return unsent;
        };
        if ticker.settling_position_authority() {
            // These ticks stay queued until earlier inputs are confirmed; step them then.
            return unsent;
        }
        self.last_stepped_tick = Some(newest);
        if !ticker.accepts_block_interactions() {
            // Withheld ticks never reach the server, so neither may their actions.
            self.machine.interrupt();
            return unsent;
        }
        for (tick, motion) in ticks {
            let maintaining = self.machine != DestroyMachine::default();
            let mut payload = self.machine.step(input, motion, authority);
            payload.assign_request(&mut request_id);
            if payload.swing {
                swing(tick);
            }
            let mine_block = payload
                .mine_block
                .as_ref()
                .map(|request| request.request_id());
            let broken = payload.broken;
            if payload.is_empty() && !maintaining && self.machine == DestroyMachine::default() {
                continue;
            }
            if ticker.attach_survival_mining(tick, payload) {
                broken.into_iter().for_each(&mut predict_break);
                if let Some(position) = broken {
                    if let DestroyInput::Held(Some(target)) | DestroyInput::Continue(Some(target)) =
                        input
                    {
                        self.break_cues.push(BlockBreakCue::Break {
                            position,
                            block_runtime_id: target.runtime_id as i32,
                        });
                    }
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

    /// Carries existing destroy cleanup before a refusal, committing both only after attachment.
    pub(crate) fn attach_refused_press(
        &mut self,
        ticker: &mut MovementTicker,
        tick: u64,
        mut refusal: SurvivalTickPayload,
        authority: BlockBreakingAuthority,
    ) -> bool {
        let Some((_, motion)) = ticker
            .unstepped_interaction_ticks(None)
            .into_iter()
            .find(|(candidate, _)| *candidate == tick)
        else {
            return false;
        };
        let mut machine = self.machine.clone();
        let mut cleanup = machine.step(DestroyInput::Released, motion, authority);
        if cleanup.actions.append(&refusal.actions).is_err() {
            return false;
        }
        refusal.actions = cleanup.actions;
        if !ticker.attach_survival_mining(tick, refusal) {
            return false;
        }
        self.machine = machine;
        self.last_stepped_tick = Some(tick);
        true
    }

    /// Previews a queued mining start without committing work before its carrier accepts it.
    pub(crate) fn preview_press(
        &self,
        target: &DestroyTarget,
        motion: TickMotion,
        authority: BlockBreakingAuthority,
    ) -> (DestroyMachine, SurvivalTickPayload) {
        let mut machine = self.machine.clone();
        let payload = machine.step(DestroyInput::Held(Some(target)), motion, authority);
        (machine, payload)
    }

    /// Commits a queued start only after its payload is attached to an unsent movement tick.
    pub(crate) fn commit_press(
        &mut self,
        machine: DestroyMachine,
        tick: u64,
        completed_tick: u64,
        target: &DestroyTarget,
        broken: Option<[i32; 3]>,
    ) {
        self.machine = machine;
        self.last_stepped_tick = Some(if broken.is_some() {
            completed_tick
        } else {
            tick
        });
        if let Some(position) = broken {
            self.break_cues.push(BlockBreakCue::Break {
                position,
                block_runtime_id: target.runtime_id as i32,
            });
        }
    }

    /// Predicts an admitted instant start between ticks; its queue entry retains the payload.
    pub(crate) fn predict_press(
        &mut self,
        ticker: &MovementTicker,
        target: &DestroyTarget,
        authority: BlockBreakingAuthority,
    ) -> Option<(u64, SurvivalTickPayload)> {
        if self.machine.destroying.is_some() || !ticker.accepts_block_interactions() {
            return None;
        }
        let sample = ticker.between_ticks_sample()?;
        let (machine, payload) = self.preview_press(
            target,
            TickMotion {
                on_ground: true,
                moved: 0.0,
            },
            authority,
        );
        let (Some(position), None) = (payload.broken, payload.wear) else {
            return None;
        };
        self.machine = machine;
        self.break_cues.push(BlockBreakCue::Break {
            position,
            block_runtime_id: target.runtime_id as i32,
        });
        Some((sample.tick, payload))
    }

    /// A predicted press's carrier already includes the break; later catch-up ticks must not repeat it.
    pub(crate) fn carried_prediction(&mut self, completed_tick: u64) {
        self.last_stepped_tick = Some(completed_tick);
    }

    /// Reanchors tick tracking when the session or interaction identity changes.
    /// A revoking reanchor interrupts active mining; a session change resets it entirely.
    pub fn synchronize(&mut self, identity: (SessionGeneration, PositionAuthorityGeneration)) {
        if let Some((session, _)) = self
            .position_authority
            .filter(|previous| *previous != identity)
        {
            self.last_stepped_tick = None;
            if session == identity.0 {
                self.machine.interrupt();
            } else {
                self.machine = DestroyMachine::default();
            }
        }
        self.position_authority = Some(identity);
    }

    /// Emits one throttled line naming the gate that blocked a held-attack break.
    pub fn log_blocked_mining(
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
        tracing::debug!(
            target: "cinnabar::survival_mining",
            reason,
            game_mode_known = caps.is_some(),
            can_mine = caps.is_some_and(|caps| caps.can_mine),
            authority = ?authority,
            "held attack produced no block break",
        );
    }
}

/// Whether held-mining should look for a destroy target this frame. The
/// block-breaking wire mode is deliberately not an input here: it sequences a
/// break, it never decides whether one may happen.
pub fn mining_active(
    caps: Option<GameModeCapabilities>,
    focused: bool,
    snapshot_present: bool,
) -> bool {
    focused && snapshot_present && caps.is_some_and(|caps| caps.can_mine)
}

/// Why a held attack yielded no destroy target, for the throttled diagnostic.
pub fn blocked_mining_reason(
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

/// Admitted items carrying the creative no-break trait block local destroy prediction.
pub fn destroys_in_creative(fact: &assets::facts::ItemFact) -> bool {
    !fact
        .traits
        .contains(assets::facts::ItemTraits::CREATIVE_NO_BREAK)
}

/// Flight, not mere airborne movement, exempts the destroy-speed penalty (`PlayerDestroy`).
pub fn exempt_from_airborne_penalty(mode: Option<sim::MovementMode>) -> bool {
    mode == Some(sim::MovementMode::Flying)
}

/// Whether Unbreaking at `level` lets a `roll` in `0..100` damage the item:
/// the item takes damage only when the roll is below 100 / (level + 1).
pub fn unbreaking_keeps_damage(level: u8, roll: u32) -> bool {
    level == 0 || roll < 100 / (u32::from(level) + 1)
}

/// A uniformly distributed roll in `0..100` from the process hash seed.
pub fn percent_roll() -> u32 {
    use std::hash::BuildHasher;
    (std::collections::hash_map::RandomState::new().hash_one(()) % 100) as u32
}

/// Durability one destroy costs, per dragonfly's `item/*.go` durability info (MIT).
pub const fn tool_break_damage(kind: assets::compiled::block_destroy::ToolKind) -> i32 {
    match kind {
        assets::compiled::block_destroy::ToolKind::Sword => 2,
        _ => 1,
    }
}

/// Unreadable eye blocks count as submerged, which only slows prediction.
pub fn eyes_in_water(world: &PaletteWorld<'_>, eye: [f32; 3]) -> bool {
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
