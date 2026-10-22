use protocol::{ActorEffectAction, ActorEffectEvent};
use protocol_ids::Effect;
use sim::MovementEffects;

use super::physics::MovementEffectSource;

const TRACKED_EFFECT_COUNT: usize = 8;
// Keeps every admitted Jump Boost impulse below sim's collision-query extent.
// Levitation contracts a valid current velocity toward a target of at most
// 51.25 blocks/tick at this bound, so it cannot poison the following tick.
// This is an application safety envelope, not a claim about a wire maximum.
const MAX_SUPPORTED_MOVEMENT_EFFECT_AMPLIFIER: i32 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum TrackedEffect {
    JumpBoost,
    Levitation,
    SlowFalling,
    Haste,
    MiningFatigue,
    ConduitPower,
    Weaving,
    Blindness,
}

impl TrackedEffect {
    const fn from_protocol_id(id: i32) -> Option<Self> {
        match Effect::from_wire(id) {
            Some(Effect::JumpBoost) => Some(Self::JumpBoost),
            Some(Effect::Levitation) => Some(Self::Levitation),
            Some(Effect::SlowFalling) => Some(Self::SlowFalling),
            Some(Effect::Haste) => Some(Self::Haste),
            Some(Effect::MiningFatigue) => Some(Self::MiningFatigue),
            Some(Effect::ConduitPower) => Some(Self::ConduitPower),
            Some(Effect::Weaving) => Some(Self::Weaving),
            Some(Effect::Blindness) => Some(Self::Blindness),
            _ => None,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::JumpBoost => 0,
            Self::Levitation => 1,
            Self::SlowFalling => 2,
            Self::Haste => 3,
            Self::MiningFatigue => 4,
            Self::ConduitPower => 5,
            Self::Weaving => 6,
            Self::Blindness => 7,
        }
    }

    const fn uses_vertical_safety_limit(self) -> bool {
        matches!(self, Self::JumpBoost | Self::Levitation | Self::SlowFalling)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveEffect {
    amplifier: i32,
    remaining_ticks: Option<u32>,
    /// Retained for packet correlation only; this is not the local expiry clock.
    server_tick: u64,
    sequence: u64,
}

/// Movement boosts the simulator reads, from the server or a predicted rocket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MovementBoost {
    Glide,
    Dolphin,
}

impl MovementBoost {
    #[must_use]
    pub const fn from_kind(kind: protocol::MovementEffectKind) -> Option<Self> {
        match kind {
            protocol::MovementEffectKind::GlideBoost => Some(Self::Glide),
            protocol::MovementEffectKind::DolphinBoost => Some(Self::Dolphin),
            protocol::MovementEffectKind::GeyserBoost
            | protocol::MovementEffectKind::Unknown(_) => None,
        }
    }

    /// This boost's lane in the simulator's per-tick effects.
    pub fn flag(self, effects: &mut MovementEffects) -> &mut bool {
        match self {
            Self::Glide => &mut effects.glide_boost,
            Self::Dolphin => &mut effects.dolphin_boost,
        }
    }
}

/// How long a server movement boost lasts from its first tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BoostSpan {
    Ticks(u32),
    Unbounded,
}

impl BoostSpan {
    /// Vanilla reads -1 as unbounded and lower durations as zero; a zero-tick
    /// boost still reaches the next simulated tick before it expires.
    #[must_use]
    pub fn from_wire(duration_ticks: i32) -> Self {
        match duration_ticks {
            -1 => Self::Unbounded,
            ticks => Self::Ticks(u32::try_from(ticks).unwrap_or(0).max(1)),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MiningEffects {
    pub haste: Option<i32>,
    pub mining_fatigue: Option<i32>,
    pub conduit_power: Option<i32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MovementEffectDiagnostics {
    pub stale_or_wrong_session: u64,
    pub unknown_effect_or_action: u64,
    pub unsupported_amplifier: u64,
}

/// Arrival-ordered local effect state for the current network session.
///
/// Finite protocol durations are remaining duration at arrival. They advance
/// only when local fixed-step prediction successfully commits a tick. Packet
/// ticks remain correlation metadata and deliberately do not schedule or
/// expire an effect in the local prediction clock.
/// Haste, Mining Fatigue, and Conduit Power are retained for destroy-rate prediction.
#[derive(Debug, Default)]
pub struct LocalMovementEffectTimeline {
    session_generation: u64,
    last_sequence: Option<u64>,
    active: [Option<ActiveEffect>; TRACKED_EFFECT_COUNT],
    diagnostics: MovementEffectDiagnostics,
    recent_mining: [(MiningEffects, MiningEffects); super::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME],
    recent_tick_count: usize,
}

impl LocalMovementEffectTimeline {
    pub fn begin_session(&mut self, session_generation: u64) {
        self.session_generation = session_generation;
        self.last_sequence = None;
        self.active = [None; TRACKED_EFFECT_COUNT];
        self.diagnostics = MovementEffectDiagnostics::default();
        self.recent_tick_count = 0;
    }

    pub fn apply(&mut self, session_generation: u64, sequence: u64, event: ActorEffectEvent) {
        if session_generation != self.session_generation
            || self
                .last_sequence
                .is_some_and(|last_sequence| sequence <= last_sequence)
        {
            self.diagnostics.stale_or_wrong_session =
                self.diagnostics.stale_or_wrong_session.saturating_add(1);
            return;
        }
        self.last_sequence = Some(sequence);

        let Some(effect) = TrackedEffect::from_protocol_id(event.effect_id) else {
            self.diagnostics.unknown_effect_or_action =
                self.diagnostics.unknown_effect_or_action.saturating_add(1);
            return;
        };
        match event.action {
            ActorEffectAction::Remove => self.active[effect.index()] = None,
            ActorEffectAction::Add | ActorEffectAction::Update => {
                if effect.uses_vertical_safety_limit()
                    && !(-MAX_SUPPORTED_MOVEMENT_EFFECT_AMPLIFIER
                        ..=MAX_SUPPORTED_MOVEMENT_EFFECT_AMPLIFIER)
                        .contains(&event.amplifier)
                {
                    self.diagnostics.unsupported_amplifier =
                        self.diagnostics.unsupported_amplifier.saturating_add(1);
                    return;
                }
                let remaining_ticks = if event.duration_ticks < 0 {
                    None
                } else {
                    Some(event.duration_ticks as u32)
                };
                self.active[effect.index()] = remaining_ticks
                    .is_none_or(|remaining_ticks| remaining_ticks != 0)
                    .then_some(ActiveEffect {
                        amplifier: event.amplifier,
                        remaining_ticks,
                        server_tick: event.tick,
                        sequence,
                    });
            }
            ActorEffectAction::Unknown(_) => {
                self.diagnostics.unknown_effect_or_action =
                    self.diagnostics.unknown_effect_or_action.saturating_add(1);
            }
        }
    }

    /// Wire amplifiers of the effects that scale destroy speed.
    pub fn mining_effects(&self) -> MiningEffects {
        let amplifier =
            |effect: TrackedEffect| self.active[effect.index()].map(|effect| effect.amplifier);
        MiningEffects {
            haste: amplifier(TrackedEffect::Haste),
            mining_fatigue: amplifier(TrackedEffect::MiningFatigue),
            conduit_power: amplifier(TrackedEffect::ConduitPower),
        }
    }

    /// Starts a render frame's bounded history of successfully committed effects.
    pub fn begin_frame(&mut self) {
        self.recent_tick_count = 0;
    }

    /// Number of successful ticks recorded since the latest frame began.
    pub fn recent_tick_count(&self) -> usize {
        self.recent_tick_count
    }

    /// Effects before admission and after expiry on an identified committed local tick.
    pub fn mining_tick(&self, tick: u64, completed_tick: u64) -> (MiningEffects, MiningEffects) {
        let distance = completed_tick.saturating_sub(tick) as usize;
        if tick <= completed_tick && distance < self.recent_tick_count {
            self.recent_mining[self.recent_tick_count - distance - 1]
        } else {
            let current = self.mining_effects();
            (current, current)
        }
    }

    fn current_snapshot(&self) -> MovementEffects {
        MovementEffects {
            jump_boost: self.active[TrackedEffect::JumpBoost.index()]
                .map(|effect| effect.amplifier),
            levitation: self.active[TrackedEffect::Levitation.index()]
                .map(|effect| effect.amplifier),
            slow_falling: self.active[TrackedEffect::SlowFalling.index()].is_some(),
            weaving: self.active[TrackedEffect::Weaving.index()].is_some(),
            blindness: self.active[TrackedEffect::Blindness.index()].is_some(),
            // The physics controller's boost timeline owns the boost lanes.
            glide_boost: false,
            dolphin_boost: false,
        }
    }

    fn consume_successful_tick(&mut self) {
        let before = self.mining_effects();
        for active in &mut self.active {
            let Some(effect) = active else {
                continue;
            };
            let Some(remaining_ticks) = &mut effect.remaining_ticks else {
                continue;
            };
            *remaining_ticks = remaining_ticks.saturating_sub(1);
            if *remaining_ticks == 0 {
                *active = None;
            }
        }
        if self.recent_tick_count == self.recent_mining.len() {
            self.recent_mining.rotate_left(1);
            self.recent_tick_count -= 1;
        }
        self.recent_mining[self.recent_tick_count] = (before, self.mining_effects());
        self.recent_tick_count += 1;
    }

    #[cfg(test)]
    pub const fn diagnostics(&self) -> MovementEffectDiagnostics {
        self.diagnostics
    }

    #[cfg(test)]
    pub fn metadata_for_protocol_id(&self, effect_id: i32) -> Option<(u64, u64, Option<u32>)> {
        let effect = TrackedEffect::from_protocol_id(effect_id)?;
        self.active[effect.index()]
            .map(|active| (active.sequence, active.server_tick, active.remaining_ticks))
    }
}

impl MovementEffectSource for LocalMovementEffectTimeline {
    fn snapshot(&self) -> MovementEffects {
        self.current_snapshot()
    }

    fn commit_successful_tick(&mut self) {
        self.consume_successful_tick();
    }
}

#[cfg(test)]
#[path = "mining_effects_tests.rs"]
mod mining_effects_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blindness_snapshot_tracks_duration_update_removal_and_session_reset() {
        let mut timeline = LocalMovementEffectTimeline::default();
        timeline.begin_session(1);
        let mut event = ActorEffectEvent {
            dimension: 0,
            actor_runtime_id: 1,
            action: ActorEffectAction::Add,
            effect_id: Effect::Blindness.wire_value(),
            amplifier: 0,
            particles: false,
            ambient: false,
            duration_ticks: 2,
            tick: 90,
        };
        timeline.apply(1, 1, event);
        let retained = timeline.snapshot();
        assert!(retained.blindness);
        assert!(!retained.is_empty());
        timeline.commit_successful_tick();
        assert!(timeline.snapshot().blindness);
        timeline.commit_successful_tick();
        assert!(!timeline.snapshot().blindness);
        assert!(retained.blindness);

        event.action = ActorEffectAction::Update;
        event.duration_ticks = -1;
        timeline.apply(1, 2, event);
        timeline.commit_successful_tick();
        assert!(timeline.snapshot().blindness);
        event.action = ActorEffectAction::Remove;
        timeline.apply(1, 3, event);
        assert!(timeline.snapshot().is_empty());
        event.action = ActorEffectAction::Add;
        timeline.apply(1, 4, event);
        timeline.begin_session(2);
        assert!(timeline.snapshot().is_empty());
    }

    #[test]
    fn weaving_uses_native_id_and_expires_after_successful_ticks() {
        let mut timeline = LocalMovementEffectTimeline::default();
        timeline.begin_session(1);
        timeline.apply(
            1,
            1,
            ActorEffectEvent {
                dimension: 0,
                actor_runtime_id: 1,
                action: ActorEffectAction::Add,
                effect_id: Effect::Weaving.wire_value(),
                amplifier: 0,
                particles: false,
                ambient: false,
                duration_ticks: 1,
                tick: 0,
            },
        );
        assert!(timeline.snapshot().weaving);
        timeline.commit_successful_tick();
        assert!(!timeline.snapshot().weaving);
    }
}
