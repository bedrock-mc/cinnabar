//! Attack target selection, local swings and ordered combat admission.
use crate::{BatchSendError, mining::FrozenMiningSelection, movement::MiningEffects};
use protocol::{ActorUseAction, ActorUseRequest, PlayerGameMode, PlayerInputMode, SwingSource};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

/// Wall-clock time after an attack during which block use is suppressed. Needs
/// independent measurement.
const ATTACK_BUILD_BLOCK_MILLIS: u64 = 200;

#[cfg(test)]
mod actor_pick_tests;
mod target;
pub use target::{
    ActorHit, Crosshair, actor_is_pickable, actor_pick_unique_id, block_pick_distance, classify,
    classify_block_hit, obstructs_placement, pick_actor, pick_actor_by, pick_distance,
    ray_box_entry,
};

/// Bedrock swing length in ticks, with the stronger Haste or Conduit Power taking priority.
pub fn swing_duration(effects: MiningEffects) -> i32 {
    actor_animation::SwingEffects {
        haste: effects.haste.max(effects.conduit_power),
        mining_fatigue: effects.mining_fatigue,
    }
    .ticks()
}

/// Java 1.7 swing length in ticks; Conduit Power does not modify its animation.
pub fn java_swing_duration(effects: MiningEffects) -> i32 {
    swing_duration(MiningEffects {
        conduit_power: None,
        ..effects
    })
}

mod admission;
mod queue;
pub use admission::{
    AttackAdmission, AttackEffects, AttackFrame, admit_attack_frame, admit_attack_press,
};

mod swing;
pub use swing::SwingTracker;

/// The tick and local state one attack press resolves against.
#[derive(Debug, Clone, PartialEq)]
pub struct PressContext {
    pub tick: u64,
    pub player_position: [f32; 3],
    pub input_mode: PlayerInputMode,
    pub local_runtime_id: u64,
    pub selection: Option<FrozenMiningSelection>,
    pub swing_duration: i32,
    pub now_millis: u64,
    /// Piercing components route actor/air attacks through the item transaction.
    pub item_attack: Option<ItemAttackPress>,
    /// The reached block when the game mode or held item refuses to start destroying it.
    pub refused_destroy: Option<RefusedDestroy>,
}

/// A block press whose destroy fails at its start, as vanilla reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefusedDestroy {
    pub position: [i32; 3],
    pub face: u8,
    pub reason: RefusedDestroyReason,
}

/// Why a block press cannot start destroying.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusedDestroyReason {
    /// Adventure rules forbid breaking the block with what the player holds.
    Denied,
    /// Creative mode holds an item that cannot break blocks.
    CreativeNoBreak,
}

impl RefusedDestroy {
    /// The press tick's block actions: an aborted start, preceded in Creative by the start and
    /// its standalone destroy request.
    fn payload(self, local_runtime_id: u64) -> crate::survival_mining::SurvivalTickPayload {
        let mut payload = crate::survival_mining::SurvivalTickPayload::default();
        let action = |kind, face| protocol::BlockAction {
            kind,
            position: self.position,
            face,
        };
        // An abort carries the progress percent, which no refused start has made.
        let abort = action(protocol::BlockActionKind::AbortDestroy, 0);
        if self.reason == RefusedDestroyReason::CreativeNoBreak {
            let _ = payload
                .actions
                .push(action(protocol::BlockActionKind::StartDestroy, self.face));
            payload.creative_destroy = Some(crate::survival_mining::CreativeDestroyBlock {
                local_runtime_id,
                position: self.position,
                face: self.face,
            });
        }
        // At most two actions, within the bounded list.
        let _ = payload.actions.push(abort);
        payload
    }
}

/// The sampled aim and authored cooldown for an item-directed attack.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemAttackPress {
    pub direction: [f32; 3],
    pub cooldown: Option<protocol::ItemAttackCooldown>,
}

/// Standalone packets in send order, plus whether the tick reports a missed swing.
#[derive(Debug, Default)]
pub struct MeleeOutcome {
    pub packets: Vec<protocol::Packet>,
    pub missed_swing: bool,
}

/// A local actor attack whose packet batch entered the outbound FIFO, not a damage confirmation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdmittedActorAttack {
    pub sequence: u64,
    pub tick: u64,
    pub hit: ActorHit,
}

/// Attack-press state; `actor_in_front` vetoes mining behind a targeted actor.
#[derive(Debug, Default, Clone)]
pub struct MeleeRuntime {
    queue: queue::AttackQueue,
    attack_revision: u64,
    admitted_attack: Option<AdmittedActorAttack>,
    actor_in_front: bool,
    last_attack_millis: Option<u64>,
    position_authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    attack_cooldowns: Vec<(std::sync::Arc<str>, u64)>,
}

impl MeleeRuntime {
    /// Whether a press or mining carrier still waits to enter the outbound FIFO.
    pub fn has_queued_work(&self, movement: &crate::movement::MovementTicker) -> bool {
        self.queue.entries.iter().any(|entry| match entry.work {
            queue::Work::Carrying { tick } => movement.unsent_sample_at(tick).is_some(),
            _ => true,
        })
    }

    /// Latest successfully queued local actor attack in the current position authority.
    pub const fn admitted_attack(&self) -> Option<AdmittedActorAttack> {
        self.admitted_attack
    }

    pub const fn actor_in_front(&self) -> bool {
        self.actor_in_front
    }

    /// Remaining fraction of an admitted attack category at the completed player tick.
    pub fn cooldown_progress(&self, cooldown: &protocol::ItemAttackCooldown, tick: u64) -> f32 {
        if cooldown.ticks == 0 {
            return 0.0;
        }
        self.attack_cooldowns
            .iter()
            .find(|(category, _)| *category == cooldown.category)
            .map_or(0.0, |(_, until)| {
                (until.saturating_sub(tick) as f32 / cooldown.ticks as f32).min(1.0)
            })
    }

    /// An attack press not yet resolved; a use press waits for it, as an attack handled first
    /// holds off the use.
    pub fn press_pending(&self) -> bool {
        self.queue.pending()
    }

    /// Whether a recent attack still suppresses block use.
    pub fn blocks_use_at(&self, now_millis: u64) -> bool {
        self.last_attack_millis.is_some_and(|attack| {
            now_millis >= attack && now_millis - attack < ATTACK_BUILD_BLOCK_MILLIS
        })
    }

    /// Drops queued presses when the session or interaction identity changes.
    pub fn synchronize(&mut self, authority: (SessionGeneration, PositionAuthorityGeneration)) {
        if self
            .position_authority
            .is_some_and(|previous| previous.0 != authority.0)
        {
            self.attack_cooldowns.clear();
        }
        if self
            .position_authority
            .is_some_and(|previous| previous != authority)
        {
            self.queue.entries.clear();
            self.actor_in_front = false;
            self.admitted_attack = None;
            self.last_attack_millis = None;
        }
        self.position_authority = Some(authority);
    }

    /// Records this frame's attack input; returns whether a press is waiting.
    pub fn observe_input(&mut self, pressed: bool, held: bool) -> bool {
        if pressed {
            self.queue.push();
        }
        self.queue.pending() || held
    }

    /// Records whether actor picking consumes the current attack target.
    pub fn observe_crosshair(&mut self, crosshair: Crosshair) {
        self.observe_attack_target(crosshair, false);
    }

    /// Piercing components own attacks at blocks as well as actors and air.
    /// Return the target shared by press admission and the held-button mining veto.
    pub fn observe_attack_target(
        &mut self,
        crosshair: Crosshair,
        item_directed: bool,
    ) -> Crosshair {
        let target = if item_directed && crosshair == Crosshair::Block {
            Crosshair::Miss
        } else {
            crosshair
        };
        self.actor_in_front = !matches!(target, Crosshair::Block);
        target
    }

    /// Cancels unadmitted presses while keeping the current target competition.
    pub fn cancel_press(&mut self) {
        self.queue.cancel_pending();
    }

    /// Cancels input and its target without removing an earlier accepted break.
    pub fn cancel(&mut self) {
        self.cancel_press();
        self.actor_in_front = false;
    }

    /// Bounds each pending entry's wait independently; admitted mining never expires.
    pub fn defer(&mut self, now_millis: u64) {
        self.queue.expire(now_millis);
    }

    /// Presses wait behind older inputs. Block presses retain their current-frame or rejected
    /// tick; actor attacks and misses report the state before that tick simulated.
    pub fn press_sample(
        &mut self,
        crosshair: Crosshair,
        movement: &crate::movement::MovementTicker,
        recent_ticks: usize,
        now_millis: u64,
    ) -> Option<crate::movement::InteractionSample> {
        let sample = if crosshair == Crosshair::Block {
            movement
                .first_unsent_sample_in_frame(recent_ticks)
                .or_else(|| {
                    if self.position_authority == Some(movement.interaction_authority_identity()) {
                        movement.unsent_sample_at(self.queue.first_pending()?.rejected_tick?)
                    } else {
                        None
                    }
                })
                .filter(|sample| movement.is_first_unsent_tick(sample.tick))
                .map(Into::into)
        } else {
            movement
                .first_unsent_sample_in_frame(recent_ticks)
                .and_then(|sample| movement.interaction_before_tick(sample.tick))
        };
        if sample.is_none() {
            self.defer(now_millis);
        }
        sample
    }

    /// A latched press resolves in its own frame when no unsent tick exists, as vanilla handles
    /// the press before the next tick; a miss flag or block start still rides that tick.
    pub fn between_ticks_press(
        &self,
        movement: &crate::movement::MovementTicker,
    ) -> Option<crate::movement::InteractionSample> {
        if !self.queue.pending() {
            return None;
        }
        movement.between_ticks_sample()
    }

    /// Resolves at most one latched press into packets; a held button never re-attacks.
    pub fn resolve(
        &mut self,
        crosshair: Crosshair,
        press: &PressContext,
        swings: &mut SwingTracker,
    ) -> MeleeOutcome {
        let crosshair = self.observe_attack_target(crosshair, press.item_attack.is_some());
        let mut outcome = MeleeOutcome::default();
        if self.queue.front_pending().is_none() {
            return outcome;
        }
        if crosshair == Crosshair::Block {
            self.queue.entries.front_mut().unwrap().work = match press.refused_destroy {
                Some(refused) => queue::Work::Refused {
                    tick: press.tick,
                    payload: Box::new(refused.payload(press.local_runtime_id)),
                },
                None => queue::Work::Block { tick: press.tick },
            };
        } else {
            self.queue.entries.pop_front();
        }
        let mut swing = |outcome: &mut MeleeOutcome, source| {
            if swings.try_attack_swing(press.tick, press.swing_duration) {
                outcome
                    .packets
                    .push(protocol::swing_arm_packet(press.local_runtime_id, source));
            }
        };
        if let Some(attack) = press.item_attack.as_ref()
            && let Some(selection) = press.selection.as_ref()
        {
            self.attack_cooldowns
                .retain(|(_, until)| press.tick < *until);
            let on_cooldown = attack.cooldown.as_ref().is_some_and(|cooldown| {
                self.attack_cooldowns
                    .iter()
                    .any(|(category, _)| *category == cooldown.category)
            });
            let Ok(packet) = protocol::use_item_as_attack_packet(
                protocol::HeldItemRequest {
                    hand: protocol::ItemUseHand::Mainhand,
                    selected_slot: selection.slot,
                    selected_item: selection.item.clone(),
                    player_position: press.player_position,
                },
                attack.direction,
                on_cooldown,
            ) else {
                return outcome;
            };
            outcome.packets.push(packet);
            if !on_cooldown {
                swing(&mut outcome, SwingSource::Attack);
                if let Some(cooldown) = attack.cooldown.as_ref() {
                    self.attack_cooldowns.push((
                        std::sync::Arc::clone(&cooldown.category),
                        press.tick.saturating_add(u64::from(cooldown.ticks)),
                    ));
                }
            }
            self.last_attack_millis = Some(press.now_millis);
            return outcome;
        }
        match crosshair {
            Crosshair::Actor(hit) => {
                self.last_attack_millis = Some(press.now_millis);
                let Some(selection) = press.selection.clone() else {
                    swing(&mut outcome, SwingSource::Attack);
                    return outcome;
                };
                if let Ok(packet) = protocol::use_actor_packet(ActorUseRequest {
                    actor_runtime_id: hit.runtime_id,
                    action: ActorUseAction::Attack,
                    selected_slot: selection.slot,
                    selected_item: selection.item,
                    player_position: press.player_position,
                    hit_position: hit.point,
                }) {
                    outcome.packets.push(packet);
                }
                swing(&mut outcome, SwingSource::Attack);
            }
            Crosshair::Block => match press.refused_destroy {
                None => swing(&mut outcome, SwingSource::Mine),
                // A refused start swings as an attack, reports a denial, and misses.
                Some(refused) => {
                    swing(&mut outcome, SwingSource::Attack);
                    if refused.reason == RefusedDestroyReason::Denied {
                        outcome.packets.push(protocol::deny_destroy_block_packet(
                            press.local_runtime_id,
                            refused.position,
                            refused.face,
                        ));
                    }
                    outcome.missed_swing = true;
                }
            },
            Crosshair::Miss => {
                // Touch skips the early swing on a miss but still reports it.
                if press.input_mode != PlayerInputMode::Touch {
                    swing(&mut outcome, SwingSource::Attack);
                }
                outcome.missed_swing = true;
            }
        }
        outcome
    }
}

/// Resolves the press and queues its swing and transaction as one batch.
///
/// A full queue restores the pre-press state so the same press retries, bounded like any
/// deferred press; returns whether the tick reports a missed swing.
pub fn resolve_and_send(
    runtime: &mut MeleeRuntime,
    swings: &mut SwingTracker,
    crosshair: Crosshair,
    press: &PressContext,
    now_millis: u64,
    send: impl FnOnce(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
) -> bool {
    if let Some(pending) = runtime.queue.front_pending()
        && swings.tick_is_published(press.tick)
        && (pending.rejected_tick != Some(press.tick)
            || (crosshair == Crosshair::Block && !swings.tick_is_current_publication(press.tick)))
    {
        runtime.observe_attack_target(crosshair, press.item_attack.is_some());
        runtime.defer(now_millis);
        return false;
    }
    let (saved_runtime, saved_swings) = (runtime.clone(), swings.clone());
    let outcome = runtime.resolve(crosshair, press, swings);
    let admitted = match crosshair {
        Crosshair::Actor(hit)
            if outcome
                .packets
                .iter()
                .any(protocol::is_aim_assist_rotation_action) =>
        {
            Some(hit)
        }
        _ => None,
    };
    match send(outcome.packets) {
        Ok(()) => {
            if let Some(hit) = admitted
                && let Some(sequence) = runtime.attack_revision.checked_add(1)
            {
                runtime.attack_revision = sequence;
                runtime.admitted_attack = Some(AdmittedActorAttack {
                    sequence,
                    tick: press.tick,
                    hit,
                });
            }
            outcome.missed_swing
        }
        Err(BatchSendError::Closed) => {
            if let Some(id) = saved_runtime.queue.entries.front().map(|entry| entry.id)
                && runtime
                    .queue
                    .entries
                    .front()
                    .is_some_and(|entry| entry.id == id)
            {
                runtime.queue.entries.pop_front();
            }
            outcome.missed_swing
        }
        Err(BatchSendError::Full) => {
            *runtime = saved_runtime;
            if let Some(pending) = runtime.queue.front_pending_mut() {
                pending.rejected_tick = Some(press.tick);
            }
            let candidate_swings = std::mem::replace(swings, saved_swings);
            swings.defer_unadmitted_attempt(&candidate_swings);
            runtime.defer(now_millis);
            false
        }
    }
}

pub fn press_admission(
    gameplay_screen: bool,
    game_mode: Option<PlayerGameMode>,
) -> Result<(), &'static str> {
    if !gameplay_screen {
        Err("screen_open")
    } else if game_mode == Some(PlayerGameMode::Spectator) {
        Err("spectator")
    } else {
        Ok(())
    }
}

#[cfg(test)]
use crate::interaction_authority::MAX_PENDING_INTERACTION_MILLIS;
#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "melee/selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "melee/item_attack_tests.rs"]
mod item_attack_tests;

#[cfg(test)]
#[path = "melee/press_timing_tests.rs"]
mod press_timing_tests;

#[cfg(test)]
mod queue_tests;
