use super::*;
use client_world::CommittedControlEvent;
use protocol::{
    DaylightCycleUpdateEvent, SetTimeEvent, WorldClockDefinition, WorldEnvironmentBootstrap,
};

use super::super::{WeatherState, apply_environment_control, replace_session};

fn running_clock(time: i32, paused: bool) -> WorldClock {
    let mut clock = WorldClock {
        daylight_cycle_enabled: true,
        ..Default::default()
    };
    apply_clock_update(
        &mut clock,
        WorldClockUpdateEvent::Initialize(WorldClockDefinition {
            id: OVERWORLD_CLOCK_ID,
            time,
            paused,
        }),
        1,
        5.0,
    );
    clock
}

fn sync(clock: &mut WorldClock, id: u64, time: i32, paused: bool, elapsed: f64) {
    apply_clock_update(
        clock,
        WorldClockUpdateEvent::Sync(WorldClockState { id, time, paused }),
        2,
        elapsed,
    );
}

#[test]
fn unknown_clock_ids_are_counted_and_never_replace_the_native_consumer() {
    let mut clock = WorldClock {
        daylight_cycle_enabled: true,
        ..Default::default()
    };
    let foreign_id = OVERWORLD_CLOCK_ID.wrapping_add(1);
    sync(&mut clock, foreign_id, 18_000, true, 1.0);
    assert_eq!(clock.server_time, None);
    assert_eq!(clock.ignored_clock_records, 1);

    apply_clock_update(
        &mut clock,
        WorldClockUpdateEvent::Initialize(WorldClockDefinition {
            id: foreign_id,
            time: 18_000,
            paused: true,
        }),
        3,
        2.0,
    );
    assert_eq!(clock.overworld_clock_id, None);
    assert_eq!(clock.ignored_clock_records, 2);

    clock = running_clock(6_000, false);
    let before = clock;
    sync(&mut clock, foreign_id, -1, true, 8.0);
    assert_eq!(clock.server_time, before.server_time);
    assert_eq!(
        clock.server_time_anchor_seconds,
        before.server_time_anchor_seconds
    );
    assert!(!clock.paused);
    assert_eq!(clock.last_update_sequence, before.last_update_sequence);
    assert_eq!(clock.ignored_clock_records, 1);
}

#[test]
fn named_clock_pause_and_daylight_cycle_are_independent_gates() {
    let mut clock = running_clock(6_000, false);
    let mut weather = WeatherState::default();
    let per_second = f64::from(world::TICKS_PER_SECOND);
    let after_second = 6_000 + world::TICKS_PER_SECOND as i32;
    assert_eq!(visual_world_time(clock, 6.0), f64::from(after_second));
    sync(&mut clock, OVERWORLD_CLOCK_ID, after_second, true, 6.0);
    assert_eq!(visual_world_time(clock, 60.0), f64::from(after_second));

    apply_environment_control(
        CommittedControlEvent::DaylightCycle {
            sequence: 3,
            update: DaylightCycleUpdateEvent { enabled: false },
        },
        &mut clock,
        &mut weather,
        60.0,
    );
    sync(&mut clock, OVERWORLD_CLOCK_ID, after_second, false, 61.0);
    assert!(!clock.paused);
    assert_eq!(visual_world_time(clock, 80.0), f64::from(after_second));
    apply_environment_control(
        CommittedControlEvent::DaylightCycle {
            sequence: 4,
            update: DaylightCycleUpdateEvent { enabled: true },
        },
        &mut clock,
        &mut weather,
        80.0,
    );
    assert_eq!(
        visual_world_time(clock, 81.0),
        f64::from(after_second) + per_second
    );
}

#[test]
fn equal_integer_sync_does_not_reset_the_existing_fractional_tick() {
    let mut clock = running_clock(6_000, false);
    let partial_tick_seconds = 0.5 / f64::from(world::TICKS_PER_SECOND);
    sync(
        &mut clock,
        OVERWORLD_CLOCK_ID,
        6_000,
        false,
        5.0 + partial_tick_seconds,
    );
    assert_eq!(clock.server_time_anchor_seconds, Some(5.0));
    assert!((visual_world_time(clock, 5.0 + partial_tick_seconds) - 6_000.5).abs() < 1.0e-9);
    sync(&mut clock, OVERWORLD_CLOCK_ID, -1, false, 6.0);
    assert_eq!(visual_world_time(clock, 6.0), -1.0);
}

#[test]
fn equal_legacy_time_does_not_reset_fraction_but_an_older_tick_is_a_real_update() {
    let mut clock = running_clock(6_000, false);
    let partial_tick_seconds = 0.5 / f64::from(world::TICKS_PER_SECOND);
    apply_legacy_time(&mut clock, 6_000, 5.0 + partial_tick_seconds);
    assert_eq!(clock.server_time_anchor_seconds, Some(5.0));
    apply_legacy_time(&mut clock, 6_000, 6.0);
    assert_eq!(clock.server_time_anchor_seconds, Some(6.0));
    assert_eq!(visual_world_time(clock, 6.0), 6_000.0);
}

#[test]
fn legacy_set_time_replaces_time_without_unpausing_a_named_clock() {
    let mut clock = running_clock(18_000, true);
    let mut weather = WeatherState::default();
    apply_environment_control(
        CommittedControlEvent::SetTime {
            sequence: 3,
            update: SetTimeEvent { time: i32::MIN },
        },
        &mut clock,
        &mut weather,
        10.0,
    );
    assert!(clock.paused);
    assert_eq!(visual_world_time(clock, 100.0), f64::from(i32::MIN));
}

#[test]
fn unrelated_later_initialization_does_not_clear_the_retained_registration() {
    let mut clock = running_clock(6_000, true);
    apply_clock_update(
        &mut clock,
        WorldClockUpdateEvent::Initialize(WorldClockDefinition {
            id: OVERWORLD_CLOCK_ID.wrapping_add(1),
            time: 0,
            paused: false,
        }),
        3,
        10.0,
    );
    sync(&mut clock, OVERWORLD_CLOCK_ID, 18_000, true, 11.0);
    assert_eq!(visual_world_time(clock, 100.0), 18_000.0);
    assert_eq!(clock.overworld_clock_id, Some(OVERWORLD_CLOCK_ID));
}

#[test]
fn new_session_elapsed_tick_is_not_daylight_and_canonical_clock_accepts_sync_before_init() {
    for elapsed_tick in [0, 6_000, i64::MAX] {
        let mut clock = running_clock(18_000, true);
        let mut weather = WeatherState::default();
        replace_session(
            &mut clock,
            &mut weather,
            WorldEnvironmentBootstrap {
                initial_time: elapsed_tick,
                day_cycle_lock_time: 18_000,
                daylight_cycle_enabled: true,
                weather_cycle_enabled: true,
                rain_level: 0.0,
                lightning_level: 0.0,
            },
            10.0,
        );
        assert_eq!(visual_world_time(clock, 10.0), 0.0);
        assert_eq!(clock.overworld_clock_id, Some(OVERWORLD_CLOCK_ID));
        assert!(!clock.paused);
        sync(
            &mut clock,
            OVERWORLD_CLOCK_ID.wrapping_add(1),
            18_000,
            true,
            10.0,
        );
        assert_eq!(visual_world_time(clock, 10.0), 0.0);
        sync(&mut clock, OVERWORLD_CLOCK_ID, 18_000, true, 10.0);
        assert_eq!(visual_world_time(clock, 10.0), 18_000.0);
        sync(&mut clock, OVERWORLD_CLOCK_ID, 18_000, false, 10.0);
        apply_legacy_time(&mut clock, 6_000, 10.0);
        assert_eq!(visual_world_time(clock, 10.0), 6_000.0);
        assert_eq!(
            visual_world_time(clock, 11.0),
            6_000.0 + f64::from(world::TICKS_PER_SECOND)
        );
    }
}
