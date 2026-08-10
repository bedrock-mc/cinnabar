use bevy::prelude::Resource;
use protocol::{ActorEffectAction, ActorEffectEvent};
use sim::MovementEffects;

use super::physics::MovementEffectSource;

const JUMP_BOOST_EFFECT_ID: i32 = 8;
const LEVITATION_EFFECT_ID: i32 = 24;
const SLOW_FALLING_EFFECT_ID: i32 = 27;
// Keeps every admitted Jump Boost impulse below sim's collision-query extent.
// Levitation contracts a valid current velocity toward a target of at most
// 51.25 blocks/tick at this bound, so it cannot poison the following tick.
// This is an application safety envelope, not a claim about a wire maximum.
const MAX_SUPPORTED_MOVEMENT_EFFECT_AMPLIFIER: i32 = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum VerticalEffect {
    JumpBoost,
    Levitation,
    SlowFalling,
}

impl VerticalEffect {
    const fn from_protocol_id(id: i32) -> Option<Self> {
        match id {
            JUMP_BOOST_EFFECT_ID => Some(Self::JumpBoost),
            LEVITATION_EFFECT_ID => Some(Self::Levitation),
            SLOW_FALLING_EFFECT_ID => Some(Self::SlowFalling),
            _ => None,
        }
    }

    const fn index(self) -> usize {
        match self {
            Self::JumpBoost => 0,
            Self::Levitation => 1,
            Self::SlowFalling => 2,
        }
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct MovementEffectDiagnostics {
    pub(crate) stale_or_wrong_session: u64,
    pub(crate) unknown_effect_or_action: u64,
    pub(crate) unsupported_amplifier: u64,
}

/// Arrival-ordered local effect state for the current network session.
///
/// Finite protocol durations are remaining duration at arrival. They advance
/// only when local fixed-step prediction successfully commits a tick. Packet
/// ticks remain correlation metadata and deliberately do not schedule or
/// expire an effect in the local prediction clock.
#[derive(Resource, Debug, Default)]
pub(crate) struct LocalMovementEffectTimeline {
    session_generation: u64,
    last_sequence: Option<u64>,
    active: [Option<ActiveEffect>; 3],
    diagnostics: MovementEffectDiagnostics,
}

impl LocalMovementEffectTimeline {
    pub(crate) fn begin_session(&mut self, session_generation: u64) {
        self.session_generation = session_generation;
        self.last_sequence = None;
        self.active = [None; 3];
        self.diagnostics = MovementEffectDiagnostics::default();
    }

    pub(crate) fn apply(
        &mut self,
        session_generation: u64,
        sequence: u64,
        event: ActorEffectEvent,
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

        let Some(effect) = VerticalEffect::from_protocol_id(event.effect_id) else {
            self.diagnostics.unknown_effect_or_action =
                self.diagnostics.unknown_effect_or_action.saturating_add(1);
            return;
        };
        match event.action {
            ActorEffectAction::Remove => self.active[effect.index()] = None,
            ActorEffectAction::Add | ActorEffectAction::Update => {
                if !(-MAX_SUPPORTED_MOVEMENT_EFFECT_AMPLIFIER
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

    fn current_snapshot(&self) -> MovementEffects {
        MovementEffects {
            jump_boost: self.active[VerticalEffect::JumpBoost.index()]
                .map(|effect| effect.amplifier),
            levitation: self.active[VerticalEffect::Levitation.index()]
                .map(|effect| effect.amplifier),
            slow_falling: self.active[VerticalEffect::SlowFalling.index()].is_some(),
        }
    }

    fn consume_successful_tick(&mut self) {
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
    }

    #[cfg(test)]
    pub(crate) const fn diagnostics(&self) -> MovementEffectDiagnostics {
        self.diagnostics
    }

    #[cfg(test)]
    pub(crate) fn metadata_for_protocol_id(
        &self,
        effect_id: i32,
    ) -> Option<(u64, u64, Option<u32>)> {
        let effect = VerticalEffect::from_protocol_id(effect_id)?;
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
