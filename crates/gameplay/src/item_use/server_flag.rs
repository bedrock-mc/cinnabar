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

    /// Notes the use button outside tick-gated maintenance: holding it lets
    /// button-up release a use the server started, and button-up still
    /// releases a held use a server clear stopped for a tick after
    /// `completed_tick`, which vanilla keeps until that tick.
    pub fn observe_use_button(&mut self, held: bool, completed_tick: u64) {
        if held {
            if let Some(active) = self.active.as_mut()
                && active.has_run(completed_tick)
            {
                active.button_owned = true;
            }
        } else {
            self.restore_cleared_use(completed_tick, true);
        }
    }

    /// Returns an established use a server clear stopped for a tick after
    /// `completed_tick` to be released, as vanilla still holds it then; a start
    /// whose tick never ran is not a use. `owned_only` limits it to uses the
    /// button held.
    fn restore_cleared_use(&mut self, completed_tick: u64, owned_only: bool) {
        if self.active.is_some() {
            return;
        }
        let Some((_, cleared)) = self.cleared_use.take_if(|(tick, cleared)| {
            *tick > completed_tick
                && cleared.has_run(tick.saturating_sub(1))
                && (cleared.button_owned || !owned_only)
        }) else {
            return;
        };
        self.active = Some(cleared);
    }

    /// Whether a server clear stopped an established use for `tick`, which
    /// vanilla still holds until that tick runs, so a press starts nothing.
    pub(super) fn clear_awaits_tick(&self, tick: u64) -> bool {
        self.cleared_use
            .as_ref()
            .is_some_and(|(cleared_tick, cleared)| {
                *cleared_tick == tick && cleared.has_run(tick.saturating_sub(1))
            })
    }

    /// Whether a server `UsingItem` value still awaits a tick.
    pub const fn has_server_using_item(&self) -> bool {
        self.server_using.is_some()
    }

    /// Takes the tick whose `StartUsingItem` a server clear withdrew.
    pub fn take_withdrawn_start(&mut self) -> Option<u64> {
        self.withdrawn_start.take()
    }

    /// Resolves a pending server `UsingItem` value against `frame`'s tick
    /// before that tick simulates; returns whether a use started.
    pub fn resolve_server_using_item(&mut self, frame: &UseFrame) -> bool {
        let mut outcome = UseOutcome::default();
        self.apply_server_using_item(frame, &mut outcome);
        if outcome.started {
            // Held-use maintenance resumes once this tick completes.
            self.last_frame_tick = Some(frame.tick);
        }
        outcome.started
    }

    /// Notes whether gameplay has input focus. Vanilla's focus and chat
    /// handlers release an item in use when focus leaves gameplay, whoever
    /// started the use, so losing focus lets button-up release it. A use a
    /// server clear stopped for a tick after `completed_tick` is still in use
    /// natively, so it returns to be released.
    pub fn observe_gameplay_focus(&mut self, focused: bool, completed_tick: u64) {
        if self.gameplay_focused && !focused {
            self.restore_cleared_use(completed_tick, false);
            if let Some(active) = self.active.as_mut()
                && active.has_run(completed_tick)
            {
                active.button_owned = true;
            }
        }
        self.gameplay_focused = focused;
    }

    /// Applies a pending server `UsingItem` value to this frame's tick. A start
    /// waits while the selected stack is unverified and covers held uses only.
    pub(super) fn apply_server_using_item(&mut self, frame: &UseFrame, outcome: &mut UseOutcome) {
        match self.server_using {
            None => {}
            Some(false) => {
                self.server_using = None;
                // An observed release already ends the use with its transaction.
                if self.release_pending {
                    return;
                }
                if let Some(active) = self.active.take() {
                    // A start whose tick has not run never reaches that tick's input.
                    if active.server_started && active.started_tick >= frame.tick {
                        self.withdrawn_start = Some(active.started_tick);
                    }
                    self.cleared_use = Some((frame.tick, active));
                }
                self.riptide_release = None;
            }
            Some(true) if self.active.is_some() => self.server_using = None,
            Some(true) => {
                // A set before any tick processed the clear keeps the use as it was.
                if let Some((_, active)) = self.cleared_use.take().filter(|(tick, cleared)| {
                    *tick == frame.tick && !cleared.selection_changed(frame.selection.as_ref())
                }) {
                    self.server_using = None;
                    outcome.started = active.started_tick >= frame.tick;
                    self.active = Some(active);
                    return;
                }
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
                    server_started: true,
                });
                outcome.started = true;
            }
        }
    }
}
