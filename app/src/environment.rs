use bevy::prelude::Resource;
use protocol::{WeatherChannel, WorldEnvironmentBootstrap};

use crate::world_stream::CommittedControlEvent;

/// Server-authored world-clock snapshot for the active StartGame session.
///
/// This stores protocol values only. Visual day-cycle interpretation belongs
/// to the later atmosphere tranche.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct WorldClock {
    session_generation: u64,
    server_time: Option<i32>,
    day_cycle_stop_time: i32,
    last_update_sequence: Option<u64>,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read by the upcoming Phase 2.7 atmosphere systems"
    )
)]
impl WorldClock {
    #[must_use]
    pub(crate) const fn session_generation(self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub(crate) const fn server_time(self) -> Option<i32> {
        self.server_time
    }

    #[must_use]
    pub(crate) const fn day_cycle_stop_time(self) -> i32 {
        self.day_cycle_stop_time
    }

    #[must_use]
    pub(crate) const fn last_update_sequence(self) -> Option<u64> {
        self.last_update_sequence
    }
}

/// Server-authored weather targets for the active StartGame session.
///
/// The protocol layer already bounds both channels to `0.0..=1.0`; this
/// resource deliberately retains those targets without interpolation.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct WeatherState {
    session_generation: u64,
    rain_level: f32,
    lightning_level: f32,
    last_update_sequence: Option<u64>,
}

#[cfg_attr(
    not(test),
    expect(
        dead_code,
        reason = "read by the upcoming Phase 2.7 atmosphere systems"
    )
)]
impl WeatherState {
    #[must_use]
    pub(crate) const fn session_generation(self) -> u64 {
        self.session_generation
    }

    #[must_use]
    pub(crate) const fn rain_level(self) -> f32 {
        self.rain_level
    }

    #[must_use]
    pub(crate) const fn lightning_level(self) -> f32 {
        self.lightning_level
    }

    #[must_use]
    pub(crate) const fn last_update_sequence(self) -> Option<u64> {
        self.last_update_sequence
    }
}

/// Replaces the environment snapshot when a new StartGame begins a session.
pub(crate) fn replace_session(
    clock: &mut WorldClock,
    weather: &mut WeatherState,
    bootstrap: WorldEnvironmentBootstrap,
) {
    let session_generation = clock
        .session_generation
        .max(weather.session_generation)
        .saturating_add(1);
    *clock = WorldClock {
        session_generation,
        server_time: None,
        day_cycle_stop_time: bootstrap.day_cycle_stop_time,
        last_update_sequence: None,
    };
    *weather = WeatherState {
        session_generation,
        rain_level: bootstrap.rain_level,
        lightning_level: bootstrap.lightning_level,
        last_update_sequence: None,
    };
}

/// Applies one FIFO-committed environment control.
///
/// Returns `true` when the control was environment-only. Spatial controls,
/// including dimension changes, leave the session snapshot untouched.
pub(crate) fn apply_environment_control(
    control: CommittedControlEvent,
    clock: &mut WorldClock,
    weather: &mut WeatherState,
) -> bool {
    match control {
        CommittedControlEvent::SetTime { sequence, update } => {
            clock.server_time = Some(update.time);
            clock.last_update_sequence = Some(sequence);
            true
        }
        CommittedControlEvent::Weather { sequence, update } => {
            match update.channel {
                WeatherChannel::Rain => weather.rain_level = update.level,
                WeatherChannel::Lightning => weather.lightning_level = update.level,
            }
            weather.last_update_sequence = Some(sequence);
            true
        }
        CommittedControlEvent::MovePlayer { .. }
        | CommittedControlEvent::PlayerMovementCorrection { .. }
        | CommittedControlEvent::ChangeDimension { .. } => false,
    }
}

#[cfg(test)]
mod tests {
    use protocol::{
        ChangeDimensionEvent, SetTimeEvent, WeatherChannel, WeatherUpdateEvent,
        WorldEnvironmentBootstrap,
    };

    use super::{WeatherState, WorldClock, apply_environment_control, replace_session};
    use crate::world_stream::CommittedControlEvent;

    fn bootstrap(
        day_cycle_stop_time: i32,
        rain_level: f32,
        lightning_level: f32,
    ) -> WorldEnvironmentBootstrap {
        WorldEnvironmentBootstrap {
            day_cycle_stop_time,
            rain_level,
            lightning_level,
        }
    }

    #[test]
    fn start_game_replacement_resets_time_and_replaces_exact_environment_snapshot() {
        let mut clock = WorldClock::default();
        let mut weather = WeatherState::default();

        replace_session(&mut clock, &mut weather, bootstrap(-1, 0.25, 0.75));
        assert_eq!(clock.session_generation(), 1);
        assert_eq!(clock.server_time(), None);
        assert_eq!(clock.day_cycle_stop_time(), -1);
        assert_eq!(weather.session_generation(), 1);
        assert_eq!(weather.rain_level(), 0.25);
        assert_eq!(weather.lightning_level(), 0.75);

        assert!(apply_environment_control(
            CommittedControlEvent::SetTime {
                sequence: 7,
                update: SetTimeEvent { time: i32::MIN },
            },
            &mut clock,
            &mut weather,
        ));
        replace_session(&mut clock, &mut weather, bootstrap(i32::MAX, 1.0, 0.0));

        assert_eq!(clock.session_generation(), 2);
        assert_eq!(
            clock.server_time(),
            None,
            "a new StartGame has no SetTime yet"
        );
        assert_eq!(clock.day_cycle_stop_time(), i32::MAX);
        assert_eq!(clock.last_update_sequence(), None);
        assert_eq!(weather.session_generation(), 2);
        assert_eq!(weather.rain_level(), 1.0);
        assert_eq!(weather.lightning_level(), 0.0);
        assert_eq!(weather.last_update_sequence(), None);
    }

    #[test]
    fn committed_updates_preserve_signed_time_channel_targets_and_order() {
        let mut clock = WorldClock::default();
        let mut weather = WeatherState::default();
        replace_session(&mut clock, &mut weather, bootstrap(18_000, 0.0, 0.0));

        for control in [
            CommittedControlEvent::Weather {
                sequence: 11,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 1.0,
                },
            },
            CommittedControlEvent::SetTime {
                sequence: 12,
                update: SetTimeEvent { time: -24_001 },
            },
            CommittedControlEvent::Weather {
                sequence: 13,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Lightning,
                    level: 0.75,
                },
            },
            CommittedControlEvent::Weather {
                sequence: 14,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 0.25,
                },
            },
        ] {
            assert!(apply_environment_control(control, &mut clock, &mut weather));
        }

        assert_eq!(clock.server_time(), Some(-24_001));
        assert_eq!(clock.day_cycle_stop_time(), 18_000);
        assert_eq!(clock.last_update_sequence(), Some(12));
        assert_eq!(weather.rain_level(), 0.25);
        assert_eq!(weather.lightning_level(), 0.75);
        assert_eq!(weather.last_update_sequence(), Some(14));
    }

    #[test]
    fn dimension_change_is_not_an_environment_session_replacement() {
        let mut clock = WorldClock::default();
        let mut weather = WeatherState::default();
        replace_session(&mut clock, &mut weather, bootstrap(6_000, 0.5, 0.75));
        let before_clock = clock;
        let before_weather = weather;

        assert!(!apply_environment_control(
            CommittedControlEvent::ChangeDimension {
                change: ChangeDimensionEvent {
                    dimension: 1,
                    position: [0.0, 64.0, 0.0],
                },
                resolved: crate::server_position::ResolvedServerPosition {
                    position: [0.0, 64.0, 0.0],
                    surface_anchor: None,
                },
            },
            &mut clock,
            &mut weather,
        ));

        assert_eq!(clock, before_clock);
        assert_eq!(weather, before_weather);
    }
}
