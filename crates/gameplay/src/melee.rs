//! Attack target selection, local swings and ordered combat admission.
use crate::{
    BatchSendError, interaction_authority::MAX_PENDING_INTERACTION_MILLIS,
    mining::FrozenMiningSelection, movement::MiningEffects,
};
use protocol::{
    ActorUseAction, ActorUseRequest, BedrockSession, PlayerGameMode, PlayerInputMode, SwingSource,
};

/// Wall-clock time after an attack during which block use is suppressed. Needs
/// independent measurement.
const ATTACK_BUILD_BLOCK_MILLIS: u64 = 200;

mod target;
pub use target::{
    ActorHit, Crosshair, classify, obstructs_placement, pick_actor, pick_actor_by, ray_box_entry,
};

/// Bedrock swing length in ticks, with the stronger Haste or Conduit Power taking priority.
pub fn swing_duration(effects: MiningEffects) -> i32 {
    // Server amplifiers are untrusted; saturate instead of overflowing.
    let level = |amplifier: Option<i32>| {
        amplifier
            .filter(|value| *value >= 0)
            .map(|value| value.saturating_add(1))
    };
    let haste = level(effects.haste).max(level(effects.conduit_power));
    let duration = match (haste, level(effects.mining_fatigue)) {
        (Some(haste), _) => client_world::ACTOR_SWING_TICKS.saturating_sub(haste),
        (None, Some(fatigue)) => {
            client_world::ACTOR_SWING_TICKS.saturating_add(fatigue.saturating_mul(2))
        }
        (None, None) => client_world::ACTOR_SWING_TICKS,
    };
    duration.max(1)
}

/// Java 1.7 swing length in ticks; Conduit Power does not modify its animation.
pub fn java_swing_duration(effects: MiningEffects) -> i32 {
    swing_duration(MiningEffects {
        conduit_power: None,
        ..effects
    })
}

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

/// Attack-press state; `actor_in_front` vetoes mining behind a targeted actor.
#[derive(Debug, Default, Clone)]
pub struct MeleeRuntime {
    latched_press: bool,
    actor_in_front: bool,
    last_attack_millis: Option<u64>,
    position_authority: Option<(u64, u64)>,
    /// Wall-clock millis at which a latched press first waited for admission.
    deferred_since: Option<u64>,
    rejected_tick: Option<u64>,
    attack_cooldowns: Vec<(std::sync::Arc<str>, u64)>,
}

impl MeleeRuntime {
    pub const fn actor_in_front(&self) -> bool {
        self.actor_in_front
    }

    /// An attack press not yet resolved; a use press waits for it, as an attack handled first
    /// holds off the use.
    pub const fn press_pending(&self) -> bool {
        self.latched_press
    }

    /// Whether a recent attack still suppresses block use.
    pub fn blocks_use_at(&self, now_millis: u64) -> bool {
        self.last_attack_millis.is_some_and(|attack| {
            now_millis >= attack && now_millis - attack < ATTACK_BUILD_BLOCK_MILLIS
        })
    }

    /// Drops a latched press when the session or position authority changes.
    pub fn synchronize(&mut self, authority: (u64, u64)) {
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
            self.latched_press = false;
            self.last_attack_millis = None;
            self.deferred_since = None;
            self.rejected_tick = None;
        }
        self.position_authority = Some(authority);
    }

    /// Records this frame's attack input; returns whether a press is waiting.
    pub fn observe_input(&mut self, pressed: bool, held: bool) -> bool {
        self.latched_press |= pressed;
        self.latched_press || held
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

    pub fn cancel(&mut self) {
        self.latched_press = false;
        self.actor_in_front = false;
        self.deferred_since = None;
        self.rejected_tick = None;
    }

    /// Bounds waiting on unavailable interaction evidence or simulation ticks in time, so the
    /// frame rate never shortens it.
    pub fn defer(&mut self, now_millis: u64) {
        if !self.latched_press {
            return;
        }
        let since = *self.deferred_since.get_or_insert(now_millis);
        if now_millis.saturating_sub(since) > MAX_PENDING_INTERACTION_MILLIS {
            self.cancel();
        }
    }

    /// Prioritizes current-frame block ticks, then this authority's exact rejected tick.
    /// Bounds a latched press while no eligible simulation sample exists.
    pub fn press_sample(
        &mut self,
        crosshair: Crosshair,
        movement: &crate::movement::MovementTicker,
        recent_ticks: usize,
        now_millis: u64,
    ) -> Option<crate::movement::UnsentSampleView> {
        let sample = if crosshair == Crosshair::Block {
            movement
                .first_unsent_sample_in_frame(recent_ticks)
                .or_else(|| {
                    if self.latched_press
                        && self.position_authority
                            == Some(movement.interaction_authority_identity())
                    {
                        movement.unsent_sample_at(self.rejected_tick?)
                    } else {
                        None
                    }
                })
        } else {
            movement.newest_unsent_sample()
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
        if !self.latched_press {
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
        self.deferred_since = None;
        let mut outcome = MeleeOutcome::default();
        if !std::mem::take(&mut self.latched_press) {
            return outcome;
        }
        self.rejected_tick = None;
        let mut swing = |outcome: &mut MeleeOutcome, source| {
            if swings.try_swing(press.tick, press.swing_duration) {
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
                    selected_slot: selection.slot,
                    selected_item: selection.item.clone(),
                    player_position: press.player_position,
                },
                attack.direction,
                on_cooldown,
            ) else {
                return outcome;
            };
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
            outcome.packets.push(packet);
            return outcome;
        }
        match crosshair {
            Crosshair::Actor(hit) => {
                swing(&mut outcome, SwingSource::Attack);
                self.last_attack_millis = Some(press.now_millis);
                let Some(selection) = press.selection.clone() else {
                    return outcome;
                };
                // The item descriptor no longer reads session state.
                let session = BedrockSession { shield_item_id: 0 };
                if let Ok(packet) = protocol::use_actor_packet(
                    ActorUseRequest {
                        actor_runtime_id: hit.runtime_id,
                        action: ActorUseAction::Attack,
                        selected_slot: selection.slot,
                        selected_item: selection.item,
                        player_position: press.player_position,
                        hit_position: hit.point,
                    },
                    &session,
                ) {
                    outcome.packets.push(packet);
                }
            }
            Crosshair::Block => swing(&mut outcome, SwingSource::Mine),
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
    if runtime.latched_press
        && swings.tick_is_published(press.tick)
        && (runtime.rejected_tick != Some(press.tick)
            || (crosshair == Crosshair::Block && !swings.tick_is_current_publication(press.tick)))
    {
        runtime.observe_attack_target(crosshair, press.item_attack.is_some());
        runtime.defer(now_millis);
        return false;
    }
    let (saved_runtime, saved_swings) = (runtime.clone(), swings.clone());
    let outcome = runtime.resolve(crosshair, press, swings);
    match send(outcome.packets) {
        Ok(()) | Err(BatchSendError::Closed) => outcome.missed_swing,
        Err(BatchSendError::Full) => {
            *runtime = saved_runtime;
            if runtime.latched_press {
                runtime.rejected_tick = Some(press.tick);
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
mod tests;

#[cfg(test)]
#[path = "melee/selection_tests.rs"]
mod selection_tests;

#[cfg(test)]
#[path = "melee/item_attack_tests.rs"]
mod item_attack_tests;
