//! The atmosphere's bounded consumer of the native named clock registry.
//!
//! Vanilla registry initialization and synchronization. Sync skips unknown IDs and independently applies time and pause;
//! each clock's tick and the registry tick respectively gate advancement on the
//! clock's pause state and global doDaylightCycle.
//! The level-time lookup uses its canonical pre-registered hash
//! directly: a different server ID carrying the same string is not that clock.
//! Unrelated clocks and marker callbacks have no current Cinnabar consumer.

use protocol::{OVERWORLD_CLOCK_ID, WorldClockState, WorldClockUpdateEvent};

use super::{WorldClock, numeric::finite_nonnegative, visual_world_time};

pub(super) fn apply_legacy_time(clock: &mut WorldClock, time: i32, elapsed_seconds: f64) {
    let elapsed_seconds = finite_nonnegative(elapsed_seconds);
    // The legacy handler compares the current level time first; identical integer
    // times are not re-anchored.
    if clock.server_time.is_none()
        || visual_world_time(*clock, elapsed_seconds).floor() != f64::from(time)
    {
        clock.server_time = Some(f64::from(time));
        clock.server_time_anchor_seconds = Some(elapsed_seconds);
    }
}

pub(super) fn apply_clock_update(
    clock: &mut WorldClock,
    update: WorldClockUpdateEvent,
    sequence: u64,
    elapsed_seconds: f64,
) {
    let elapsed_seconds = finite_nonnegative(elapsed_seconds);
    match update {
        WorldClockUpdateEvent::Initialize(definition) => {
            if definition.id != OVERWORLD_CLOCK_ID {
                clock.ignored_clock_records = clock.ignored_clock_records.saturating_add(1);
                return;
            }
            clock.overworld_clock_id = Some(definition.id);
            // Initialization assigns the complete registered clock, even
            // when its integer time matches the previous clock.
            clock.server_time = Some(f64::from(definition.time));
            clock.server_time_anchor_seconds = Some(elapsed_seconds);
            clock.paused = definition.paused;
            clock.last_update_sequence = Some(sequence);
        }
        WorldClockUpdateEvent::Sync(state) => {
            if clock.overworld_clock_id != Some(state.id) {
                clock.ignored_clock_records = clock.ignored_clock_records.saturating_add(1);
                return;
            }
            apply_state(clock, state, elapsed_seconds);
            clock.last_update_sequence = Some(sequence);
        }
    }
}

fn apply_state(clock: &mut WorldClock, state: WorldClockState, elapsed_seconds: f64) {
    let current_time = visual_world_time(*clock, elapsed_seconds);
    // Vanilla compares the integer clock time, rather than resetting the
    // renderer's fractional tick on every equal-valued synchronization packet.
    let time_changed = current_time.floor() != f64::from(state.time);
    if time_changed || clock.paused != state.paused {
        clock.server_time = Some(if time_changed {
            f64::from(state.time)
        } else if state.paused {
            current_time.floor()
        } else {
            current_time
        });
        clock.server_time_anchor_seconds = Some(elapsed_seconds);
    }
    clock.paused = state.paused;
}

/// Opt-in, at most one scalar clock record per second. No server names or
/// arbitrary payloads are logged, and retaining diagnostics is constant-space.
pub(super) fn trace_clock(clock: &mut WorldClock, packet: &'static str, elapsed_seconds: f64) {
    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !*ENABLED.get_or_init(|| {
        std::env::var("CINNABAR_WORLD_CLOCK_DIAGNOSTICS").is_ok_and(|value| value == "1")
    }) {
        return;
    }
    let elapsed_seconds = finite_nonnegative(elapsed_seconds);
    if clock
        .last_diagnostic_seconds
        .is_some_and(|last| elapsed_seconds - last < 1.0)
    {
        return;
    }
    clock.last_diagnostic_seconds = Some(elapsed_seconds);
    bevy::log::info!(
        packet,
        session = clock.session_generation,
        sequence = ?clock.last_update_sequence,
        clock_name = protocol::OVERWORLD_CLOCK_NAME,
        clock_id = ?clock.overworld_clock_id,
        server_anchor = ?clock.server_time,
        visual_tick = visual_world_time(*clock, elapsed_seconds),
        elapsed_seconds,
        paused = clock.paused,
        daylight_cycle = clock.daylight_cycle_enabled,
        ignored_records = clock.ignored_clock_records,
        "world clock diagnostic"
    );
}

#[cfg(test)]
mod tests;
