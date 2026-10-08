use protocol::{ActorEffectAction, ActorEffectEvent};
use sim::MovementEffects;

use super::physics::MovementEffectSource;

const JUMP_BOOST_EFFECT_ID: i32 = 8;
const LEVITATION_EFFECT_ID: i32 = 24;
const SLOW_FALLING_EFFECT_ID: i32 = 27;
const HASTE_EFFECT_ID: i32 = 3;
const MINING_FATIGUE_EFFECT_ID: i32 = 4;
const CONDUIT_POWER_EFFECT_ID: i32 = 26;
const WEAVING_EFFECT_ID: i32 = 33;
const BLINDNESS_EFFECT_ID: i32 = 15;
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
        match id {
            JUMP_BOOST_EFFECT_ID => Some(Self::JumpBoost),
            LEVITATION_EFFECT_ID => Some(Self::Levitation),
            SLOW_FALLING_EFFECT_ID => Some(Self::SlowFalling),
            HASTE_EFFECT_ID => Some(Self::Haste),
            MINING_FATIGUE_EFFECT_ID => Some(Self::MiningFatigue),
            CONDUIT_POWER_EFFECT_ID => Some(Self::ConduitPower),
            WEAVING_EFFECT_ID => Some(Self::Weaving),
            BLINDNESS_EFFECT_ID => Some(Self::Blindness),
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

/// Server-predicted movement boosts the simulator reads.
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

    const fn index(self) -> usize {
        match self {
            Self::Glide => 0,
            Self::Dolphin => 1,
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

/// How long a server-predicted movement boost lasts from the next simulated tick.
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

    /// Whether the boost covers the `index`th tick after it starts, counting from one.
    #[must_use]
    pub fn covers(self, index: u64) -> bool {
        match self {
            Self::Ticks(ticks) => index <= u64::from(ticks),
            Self::Unbounded => true,
        }
    }

    /// The span left after `elapsed` ticks, if any.
    #[must_use]
    pub fn after(self, elapsed: u64) -> Option<Self> {
        match self {
            Self::Ticks(ticks) => u64::from(ticks)
                .checked_sub(elapsed)
                .filter(|left| *left > 0)
                .map(|left| Self::Ticks(left as u32)),
            Self::Unbounded => Some(Self::Unbounded),
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
    boosts: [Option<BoostSpan>; 2],
    diagnostics: MovementEffectDiagnostics,
    recent_mining: [(MiningEffects, MiningEffects); super::MAX_LOCAL_PHYSICS_TICKS_PER_FRAME],
    recent_tick_count: usize,
}

impl LocalMovementEffectTimeline {
    pub fn begin_session(&mut self, session_generation: u64) {
        self.session_generation = session_generation;
        self.last_sequence = None;
        self.active = [None; TRACKED_EFFECT_COUNT];
        self.boosts = [None; 2];
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

    /// Replaces a boost the next simulated tick sees, after any rewind consumed part of it.
    pub fn set_movement_boost(
        &mut self,
        session_generation: u64,
        sequence: u64,
        boost: MovementBoost,
        remaining: Option<BoostSpan>,
    ) {
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
        self.boosts[boost.index()] = remaining;
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
            glide_boost: self.boosts[MovementBoost::Glide.index()].is_some(),
            dolphin_boost: self.boosts[MovementBoost::Dolphin.index()].is_some(),
        }
    }

    fn consume_successful_tick(&mut self) {
        let before = self.mining_effects();
        for boost in &mut self.boosts {
            *boost = boost.and_then(|span| span.after(1));
        }
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
            effect_id: BLINDNESS_EFFECT_ID,
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

    /// A glide boost covers exactly its duration in ticks; zero still covers one tick.
    #[test]
    fn glide_boost_covers_its_duration_in_successful_ticks() {
        for (duration, ticks) in [(3, 3), (0, 1), (-7, 1)] {
            let mut timeline = LocalMovementEffectTimeline::default();
            timeline.begin_session(1);
            timeline.set_movement_boost(
                1,
                1,
                MovementBoost::Glide,
                Some(BoostSpan::from_wire(duration)),
            );
            for _ in 0..ticks {
                assert!(timeline.snapshot().glide_boost, "{duration}");
                timeline.commit_successful_tick();
            }
            assert!(!timeline.snapshot().glide_boost, "{duration}");
        }
        let mut timeline = LocalMovementEffectTimeline::default();
        timeline.begin_session(1);
        timeline.set_movement_boost(1, 1, MovementBoost::Dolphin, Some(BoostSpan::from_wire(-1)));
        for _ in 0..100 {
            timeline.commit_successful_tick();
        }
        assert!(timeline.snapshot().dolphin_boost);
        assert!(!timeline.snapshot().glide_boost);
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
                effect_id: WEAVING_EFFECT_ID,
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
