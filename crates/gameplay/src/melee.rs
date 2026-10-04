//! Attack target selection, local swings and ordered combat admission.
use crate::{
    BatchSendError, interaction_authority::MAX_PENDING_INTERACTION_FRAMES,
    mining::FrozenMiningSelection, movement::MiningEffects,
};
use client_world::ActorSnapshot;
use protocol::{
    ActorUseAction, ActorUseRequest, BedrockSession, PlayerGameMode, PlayerInputMode, SwingSource,
};

/// Pick-box inflation and actor-versus-block bias. Needs independent measurement.
const ACTOR_PICK_RADIUS: f64 = 0.1;
/// Documented default swing length of 0.3 seconds.
const DEFAULT_SWING_TICKS: i32 = 6;
/// Wall-clock time after an attack during which block use is suppressed. Needs
/// independent measurement.
const ATTACK_BUILD_BLOCK_MILLIS: u64 = 200;

/// Actors vanilla cannot pick: drops, orbs, projectiles and effect carriers.
const UNPICKABLE_ACTORS: &[&str] = &[
    "minecraft:item",
    "minecraft:xp_orb",
    "minecraft:arrow",
    "minecraft:thrown_trident",
    "minecraft:snowball",
    "minecraft:egg",
    "minecraft:ender_pearl",
    "minecraft:splash_potion",
    "minecraft:lingering_potion",
    "minecraft:xp_bottle",
    "minecraft:fireball",
    "minecraft:small_fireball",
    "minecraft:wither_skull",
    "minecraft:wither_skull_dangerous",
    "minecraft:dragon_fireball",
    "minecraft:wind_charge_projectile",
    "minecraft:breeze_wind_charge_projectile",
    "minecraft:fishing_hook",
    "minecraft:falling_block",
    "minecraft:lightning_bolt",
    "minecraft:area_effect_cloud",
    "minecraft:evocation_fang",
    "minecraft:eye_of_ender_signal",
    "minecraft:fireworks_rocket",
    "minecraft:llama_spit",
    "minecraft:shulker_bullet",
];

/// The nearest pickable actor along the crosshair ray.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActorHit {
    pub runtime_id: u64,
    pub distance: f64,
    pub point: [f32; 3],
}

/// What an attack press resolves to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Crosshair {
    Actor(ActorHit),
    Block,
    /// Nothing in reach, including an actor in front but beyond melee reach.
    Miss,
}

/// Nearest actor whose inflated box the ray enters within `reach`.
pub fn pick_actor<'a>(
    actors: impl Iterator<Item = &'a ActorSnapshot>,
    excluded_unique_id: Option<i64>,
    origin: [f32; 3],
    direction: [f32; 3],
    reach: f64,
) -> Option<ActorHit> {
    let origin = origin.map(f64::from);
    let length = direction
        .into_iter()
        .map(|axis| f64::from(axis).powi(2))
        .sum::<f64>()
        .sqrt();
    if !length.is_finite() || length == 0.0 {
        return None;
    }
    let direction = direction.map(|axis| f64::from(axis) / length);
    actors
        .filter(|actor| Some(actor.unique_id) != excluded_unique_id && pickable(actor))
        .filter_map(|actor| {
            let (min, max) = actor.bounding_box()?;
            let min = min.map(|axis| f64::from(axis) - ACTOR_PICK_RADIUS);
            let max = max.map(|axis| f64::from(axis) + ACTOR_PICK_RADIUS);
            let distance = ray_box_entry(origin, direction, min, max)?;
            (distance <= reach).then(|| ActorHit {
                runtime_id: actor.runtime_id,
                distance,
                point: [0, 1, 2].map(|axis| (origin[axis] + direction[axis] * distance) as f32),
            })
        })
        .min_by(|left, right| left.distance.total_cmp(&right.distance))
}

/// Drops, orbs, projectiles and effect carriers never block a placement either.
pub fn obstructs_placement(actor: &ActorSnapshot) -> bool {
    pickable(actor)
}

fn pickable(actor: &ActorSnapshot) -> bool {
    match &actor.kind {
        protocol::ActorKind::Player { .. } => true,
        protocol::ActorKind::Entity { identifier } => {
            !UNPICKABLE_ACTORS.contains(&identifier.as_ref())
        }
    }
}

/// Distance along a unit `direction` at which the ray enters the box; zero from inside.
fn ray_box_entry(
    origin: [f64; 3],
    direction: [f64; 3],
    min: [f64; 3],
    max: [f64; 3],
) -> Option<f64> {
    let mut near = 0.0_f64;
    let mut far = f64::INFINITY;
    for axis in 0..3 {
        if direction[axis].abs() < f64::EPSILON {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let first = (min[axis] - origin[axis]) / direction[axis];
        let second = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(first.min(second));
        far = far.min(first.max(second));
        if near > far {
            return None;
        }
    }
    Some(near)
}

/// Resolves the press target; an actor wins only when clearly in front of the block.
pub fn classify(
    actor: Option<ActorHit>,
    block_distance: Option<f64>,
    attack_reach: f64,
) -> Crosshair {
    let limit = block_distance.unwrap_or(f64::INFINITY);
    match actor {
        Some(hit) if hit.distance + ACTOR_PICK_RADIUS < limit.min(attack_reach) => {
            Crosshair::Actor(hit)
        }
        Some(hit) if hit.distance + ACTOR_PICK_RADIUS < limit => Crosshair::Miss,
        _ if block_distance.is_some() => Crosshair::Block,
        _ => Crosshair::Miss,
    }
}

/// Swing length in ticks under the current effects. Adjustments need independent measurement.
pub fn swing_duration(effects: MiningEffects) -> i32 {
    // Server amplifiers are untrusted; saturate instead of overflowing.
    let level = |amplifier: Option<i32>| {
        amplifier
            .filter(|value| *value >= 0)
            .map(|value| value.saturating_add(1))
    };
    let haste = level(effects.haste).max(level(effects.conduit_power));
    let duration = match (haste, level(effects.mining_fatigue)) {
        (Some(haste), _) => DEFAULT_SWING_TICKS.saturating_sub(haste),
        (None, Some(fatigue)) => DEFAULT_SWING_TICKS.saturating_add(fatigue.saturating_mul(2)),
        (None, None) => DEFAULT_SWING_TICKS,
    };
    duration.max(1)
}

/// The local arm-swing guard: a new swing starts once half the current one elapsed.
#[derive(Debug, Default, Clone)]
pub struct SwingTracker {
    last_swing_tick: Option<u64>,
    /// Duration of a swing started since the local rig last took it.
    started: Option<i32>,
}

impl SwingTracker {
    pub fn try_swing(&mut self, tick: u64, duration: i32) -> bool {
        let half = u64::try_from(duration / 2).unwrap_or(0);
        let allowed = self
            .last_swing_tick
            .is_none_or(|last| tick < last || tick - last >= half);
        if allowed {
            self.last_swing_tick = Some(tick);
            self.started = Some(duration);
        }
        allowed
    }

    /// The duration of a swing started since the last call; the local rig plays it.
    pub fn take_started(&mut self) -> Option<i32> {
        self.started.take()
    }
}

/// The unsent tick and local state one attack press resolves against.
#[derive(Debug, Clone, PartialEq)]
pub struct PressContext {
    pub tick: u64,
    pub player_position: [f32; 3],
    pub input_mode: PlayerInputMode,
    pub local_runtime_id: u64,
    pub selection: Option<FrozenMiningSelection>,
    pub swing_duration: i32,
    pub now_millis: u64,
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
    /// Input frame at which a latched press first waited on block evidence.
    deferred_since: Option<u64>,
}

impl MeleeRuntime {
    pub const fn actor_in_front(&self) -> bool {
        self.actor_in_front
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
            .is_some_and(|previous| previous != authority)
        {
            self.latched_press = false;
            self.last_attack_millis = None;
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
        self.actor_in_front = !matches!(crosshair, Crosshair::Block);
    }

    pub fn cancel(&mut self) {
        self.latched_press = false;
        self.actor_in_front = false;
        self.deferred_since = None;
    }

    /// Holds a latched press while block evidence is unavailable, for a bounded number of frames.
    pub fn defer(&mut self, input_frame: u64) {
        if !self.latched_press {
            return;
        }
        let since = *self.deferred_since.get_or_insert(input_frame);
        if input_frame.saturating_sub(since) > MAX_PENDING_INTERACTION_FRAMES {
            self.cancel();
        }
    }

    /// Resolves at most one latched press into packets; a held button never re-attacks.
    pub fn resolve(
        &mut self,
        crosshair: Crosshair,
        press: &PressContext,
        swings: &mut SwingTracker,
    ) -> MeleeOutcome {
        self.observe_crosshair(crosshair);
        self.deferred_since = None;
        let mut outcome = MeleeOutcome::default();
        if !std::mem::take(&mut self.latched_press) {
            return outcome;
        }
        let mut swing = |outcome: &mut MeleeOutcome, source| {
            if swings.try_swing(press.tick, press.swing_duration) {
                outcome
                    .packets
                    .push(protocol::swing_arm_packet(press.local_runtime_id, source));
            }
        };
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
    input_frame: u64,
    send: impl FnOnce(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
) -> bool {
    let (saved_runtime, saved_swings) = (runtime.clone(), swings.clone());
    let outcome = runtime.resolve(crosshair, press, swings);
    match send(outcome.packets) {
        Ok(()) | Err(BatchSendError::Closed) => outcome.missed_swing,
        Err(BatchSendError::Full) => {
            *runtime = saved_runtime;
            *swings = saved_swings;
            runtime.defer(input_frame);
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
