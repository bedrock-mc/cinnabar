//! Item use a server `UsingItem` write starts or stops.
//!
//! Vanilla's start-or-stop-using-item system compares the actor flag with the
//! item in use each movement tick: a set flag with nothing in use starts the
//! selected item, which also reports `StartUsingItem`, and a clear flag stops
//! the use without a release transaction.

use super::*;

impl ItemUseRuntime {
    /// Records the latest server `UsingItem` value for the next resolved tick.
    pub fn adopt_server_using_item(&mut self, using: bool) {
        self.server_using = Some(using);
    }

    /// Applies a pending server `UsingItem` value to this frame's tick. A start
    /// waits while the selected stack is unverified and covers held uses only.
    pub(super) fn apply_server_using_item(&mut self, frame: &UseFrame, outcome: &mut UseOutcome) {
        match self.server_using {
            None => {}
            Some(false) => {
                self.server_using = None;
                // An observed release already ends the use with its transaction.
                if !self.release_pending {
                    self.active = None;
                    self.riptide_release = None;
                }
            }
            Some(true) if self.active.is_some() => self.server_using = None,
            Some(true) => {
                if frame.selection.is_none() {
                    return;
                }
                self.server_using = None;
                let air_use = self.crossbows.air_use(frame);
                let Some(AirUse::Hold {
                    max_ticks,
                    slowdown,
                    ..
                }) = air_use
                else {
                    return;
                };
                let Some(selection) = self.displayed_selection(frame) else {
                    return;
                };
                self.active = Some(ActiveUse {
                    selection,
                    started_tick: frame.tick,
                    max_ticks,
                    slowdown,
                    crossbow: crossbow::is_crossbow(air_use),
                    riptide: frame.riptide.is_some_and(|riptide| riptide.level > 0),
                    last_progress_tick: None,
                    button_owned: false,
                });
                outcome.started = true;
            }
        }
    }
}
