//! Item use reconciled with the server-written `UsingItem` flag.
//!
//! Vanilla's start-or-stop-using-item system runs in each movement tick and
//! compares the actor flag with the item in use: a set flag with nothing in
//! use starts the selected item, which also reports `StartUsingItem`, and a
//! clear flag stops the use without a release transaction. Between movement
//! ticks the use is untouched, so a clear and set before the next tick leave
//! it running, and a button-up or focus change in between still releases it.

use super::*;

impl ItemUseRuntime {
    /// Records the latest server `UsingItem` value for the next movement tick.
    pub fn adopt_server_using_item(&mut self, using: bool) {
        self.server_using = Some(using);
    }

    /// Notes the use button outside tick-gated maintenance: holding it lets
    /// button-up release a use the server started.
    pub fn observe_use_button(&mut self, held: bool) {
        if held && let Some(active) = self.active.as_mut() {
            active.button_owned = true;
        }
    }

    /// Takes the item-use flag the latest local start (`true`) or end
    /// (`false`) wrote, for movement's live flag word.
    pub fn take_local_using_item(&mut self) -> Option<bool> {
        self.local_using.take()
    }

    /// Whether a server `UsingItem` value still awaits a movement tick.
    pub const fn has_server_using_item(&self) -> bool {
        self.server_using.is_some()
    }

    /// Reconciles a pending server `UsingItem` value with the item in use at
    /// the movement tick `frame` names, just before it simulates; returns
    /// whether a use started. A start waits while the selected stack is
    /// unverified and covers held uses only.
    pub fn resolve_server_using_item(&mut self, frame: &UseFrame) -> bool {
        let Some(using) = self.server_using else {
            return false;
        };
        if !using {
            self.server_using = None;
            // An observed release already ends the use with its transaction.
            if !self.release_pending {
                self.active = None;
                self.riptide_release = None;
            }
            return false;
        }
        if self.active.is_some() {
            self.server_using = None;
            return false;
        }
        if frame.selection.is_none() {
            return false;
        }
        self.server_using = None;
        let air_use = self.crossbows.air_use(frame);
        let Some(AirUse::Hold {
            max_ticks,
            slowdown,
            ..
        }) = air_use
        else {
            return false;
        };
        let Some(selection) = self.displayed_selection(frame) else {
            return false;
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
        // Held-use maintenance resumes once this tick completes.
        self.last_frame_tick = Some(frame.tick);
        true
    }

    /// Notes whether gameplay has input focus. Vanilla's focus and chat
    /// handlers release an item in use when focus leaves gameplay, whoever
    /// started the use, so losing focus lets button-up release it.
    pub fn observe_gameplay_focus(&mut self, focused: bool) {
        if self.gameplay_focused
            && !focused
            && let Some(active) = self.active.as_mut()
        {
            active.button_owned = true;
        }
        self.gameplay_focused = focused;
    }
}
