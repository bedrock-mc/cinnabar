//! Fixed local visual time, controlled by the host's demo action.

use mod_api::bindings::{
    Guest,
    cinnabar::extension::{environment, hud, input},
};
use std::cell::Cell;

// Vanilla /time presets; Night is distinct from Midnight.
const MODES: [(Option<u32>, &str); 5] = [
    (None, "Time: Server"),
    (Some(1_000), "Time: Day"),
    (Some(12_000), "Time: Sunset"),
    (Some(13_000), "Time: Night"),
    (Some(18_000), "Time: Midnight"),
];
thread_local! { static MODE: Cell<usize> = const { Cell::new(0) }; }

struct TimeChanger;

impl Guest for TimeChanger {
    /// Starts with the tracked server clock.
    fn init() {
        publish(0);
    }

    /// Cycles fixed presets only on the host-assigned action edge.
    fn frame() {
        if input::demo_pressed() {
            MODE.with(|mode| {
                let next = (mode.get() + 1) % MODES.len();
                if publish(next) {
                    mode.set(next);
                }
            });
        }
    }
}

/// Changes the label only when the host accepts the clock mutation.
fn publish(mode: usize) -> bool {
    let (ticks, label) = MODES[mode];
    match environment::set_time_override(ticks) {
        Ok(()) => {
            let _ = hud::set_label(label);
            true
        }
        Err(_) => {
            let _ = hud::set_label("Time: Environment denied");
            false
        }
    }
}

mod_api::bindings::export!(TimeChanger with_types_in mod_api::bindings);
