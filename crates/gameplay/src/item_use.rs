//! Accepted item-use state and atomic same-tick transaction admission.
//!
//! Vanilla item use handles build actions, rearms after air use, and sends use
//! and release transactions.
//! Extraction retains the accepted-use timing and transport rollback rules unchanged.
use crate::{BatchSendError, melee::SwingTracker, mining::FrozenMiningSelection};
use protocol::{HeldItemRequest, PredictedSlotChange, VerifiedNetworkItemStack};
mod admission;
pub mod classify;
mod crossbow;
pub use admission::step_and_send;
pub use classify::{AirUse, Cooldown, Needs, classify};

pub const QUICK_CHARGE_ENCHANTMENT_ID: i16 = 35;
/// Vanilla re-arms the next build action this long after an air use.
const USE_REARM_MILLIS: u64 = 200;

#[derive(Debug, Clone, PartialEq)]
struct ActiveUse {
    selection: FrozenMiningSelection,
    started_tick: u64,
    max_ticks: u32,
    slowdown: f64,
    crossbow: bool,
}

/// A throw's locally consumed stack, shown until the server restates the slot.
#[derive(Debug, Clone, PartialEq)]
struct PredictedStack {
    slot: u8,
    /// The server's stack when the throw was predicted.
    server: VerifiedNetworkItemStack,
    stack: VerifiedNetworkItemStack,
}

/// One unsent tick's view of the use input and the selected stack.
#[derive(Debug, Clone, PartialEq)]
pub struct UseFrame {
    pub tick: u64,
    pub now_millis: u64,
    pub position: [f32; 3],
    pub held: bool,
    pub selection: Option<FrozenMiningSelection>,
    pub air_use: Option<AirUse>,
    /// The use's `Needs` are met (always true in creative).
    pub ready: bool,
    pub creative: bool,
    /// Authoritative writes, including identical rejected-charge corrections.
    pub inventory_revision: Option<u64>,
    pub charge_projectile: Option<&'static str>,
    /// A block interaction or recent attack consumed this press.
    pub press_consumed: bool,
}

/// Transactions in send order, plus what the local player did on this tick.
#[derive(Debug, Default)]
pub struct UseOutcome {
    pub packets: Vec<protocol::Packet>,
    pub started: bool,
    /// A throw swung the arm; its swing packet precedes `packets`.
    pub swung: bool,
    used: bool,
    released: bool,
}

/// The press latch, the accepted use, cooldowns and the throw prediction.
#[derive(Debug, Default, Clone)]
pub struct ItemUseRuntime {
    latched_press: bool,
    delay_fix: bool,
    selected_slot: Option<u8>,
    slot_change_pending: bool,
    last_air_use_tick: Option<u64>,
    active: Option<ActiveUse>,
    session: Option<u64>,
    rearm_millis: Option<u64>,
    /// Cooldown category and the tick it ends.
    cooldowns: Vec<(&'static str, u64)>,
    predicted: Option<PredictedStack>,
    /// A throw's emptied slot and the authoritative revision it consumed, until the ledger takes it.
    emptied_slot: Option<(u8, u64)>,
    /// The use button has stayed down since a press no block interaction consumed.
    repeat_armed: bool,
    /// A rejected click retries only while its verified selection remains current.
    deferred_selection: Option<FrozenMiningSelection>,
    /// Only this owner's rejected batch may retry a published tick.
    rejected_swing_tick: Option<(u64, Option<(u64, u64)>)>,
    /// A rejected release still precedes the next use, even if Use is pressed again.
    release_pending: bool,
    /// Vanilla's process-wide legacy item-stack request id counter.
    last_legacy_request_id: i32,
    crossbows: crossbow::CrossbowPredictions,
}

impl ItemUseRuntime {
    /// Allows one immediate air use after each slot change; item cooldowns remain authoritative.
    pub fn set_delay_fix(&mut self, enabled: bool) {
        self.delay_fix = enabled;
        if !enabled {
            self.slot_change_pending = false;
        }
    }

    /// Observes slot changes even without use input; unavailable inventory grants no new use.
    pub fn observe_selected_slot(&mut self, slot: Option<u8>) {
        let Some(slot) = slot else {
            return;
        };
        if self.selected_slot != Some(slot) {
            self.slot_change_pending = self.delay_fix && self.selected_slot.is_some();
            self.selected_slot = Some(slot);
        }
    }

    /// Accepted use timing shared by native presentation and movement.
    pub fn active_timing(&self) -> Option<(u64, u32)> {
        self.active
            .as_ref()
            .map(|active| (active.started_tick, active.max_ticks))
    }

    /// Remaining fraction of an admitted category cooldown at the completed player tick.
    pub fn cooldown_progress(&self, cooldown: Cooldown, tick: u64) -> f32 {
        if cooldown.ticks == 0 {
            return 0.0;
        }
        self.cooldowns
            .iter()
            .find(|(category, _)| *category == cooldown.category)
            .map_or(0.0, |(_, until)| {
                (until.saturating_sub(tick) as f32 / cooldown.ticks as f32).min(1.0)
            })
    }
    /// Predicted crossbow charge for one authoritative inventory revision.
    pub fn predicted_projectile(
        &self,
        slot: u8,
        revision: u64,
        stack: &protocol::NetworkItemStack,
    ) -> Option<Option<&'static str>> {
        self.crossbows.projectile_for_stack(slot, revision, stack)
    }

    /// Whether a use started locally and has not ended.
    pub const fn is_using(&self) -> bool {
        self.active.is_some()
    }

    /// Movement-input factor while a use runs; `None` when idle.
    pub fn movement_modifier(&self) -> Option<f64> {
        self.active.as_ref().map(|active| active.slowdown)
    }

    /// A new session drops the press, the use, cooldowns and the prediction without packets.
    pub fn synchronize(&mut self, session: u64) {
        if self.session.is_some_and(|previous| previous != session) {
            self.cancel();
        }
        self.session = Some(session);
    }

    pub fn observe_press(&mut self, pressed: bool) {
        if pressed {
            self.deferred_selection = None;
            self.rejected_swing_tick = None;
            self.latched_press = true;
        }
    }

    /// Screens, focus loss and spectator mode cancel queued clicks, but accepted uses release.
    pub fn cancel_pending_input(&mut self) {
        self.latched_press = false;
        self.repeat_armed = false;
        self.deferred_selection = None;
        self.rejected_swing_tick = None;
    }

    /// Clears session-owned use state after disconnect or session replacement.
    fn cancel(&mut self) {
        self.delay_fix = false;
        self.selected_slot = None;
        self.slot_change_pending = false;
        self.last_air_use_tick = None;
        self.cancel_pending_input();
        self.active = None;
        self.rearm_millis = None;
        self.cooldowns.clear();
        self.predicted = None;
        self.emptied_slot = None;
        self.release_pending = false;
        self.crossbows.clear();
    }

    /// The slot and authoritative revision an admitted throw of the last item emptied, once.
    pub fn take_emptied_slot(&mut self) -> Option<(u8, u64)> {
        self.emptied_slot.take()
    }

    /// Whether this frame has anything to resolve against an unsent tick.
    pub const fn has_work(&self, held: bool) -> bool {
        self.latched_press || self.active.is_some() || held
    }

    /// The tick this frame's use resolves against: the newest unsent tick, else for a press or
    /// release the next tick, as vanilla sends both in their frame. Held repeats and completion
    /// wait for a tick.
    pub fn frame_sample(
        &self,
        movement: &crate::movement::MovementTicker,
        held: bool,
        between_ticks: bool,
    ) -> Option<crate::movement::InteractionSample> {
        movement.newest_unsent_sample().map(Into::into).or_else(|| {
            let edge =
                self.latched_press || self.release_pending || (self.active.is_some() && !held);
            (between_ticks && edge)
                .then(|| movement.between_ticks_sample())
                .flatten()
        })
    }

    /// Ends a use on release, depletion or reselection, then resolves a press or held repeat.
    pub fn step(&mut self, frame: &UseFrame) -> UseOutcome {
        self.observe_selected_slot(frame.selection.as_ref().map(|selection| selection.slot));
        let mut outcome = UseOutcome::default();
        let pressed = std::mem::take(&mut self.latched_press);
        if pressed {
            self.repeat_armed = !frame.press_consumed
                && (frame.air_use.is_some()
                    || frame
                        .selection
                        .as_ref()
                        .is_none_or(|selection| selection.item.block_runtime_id() == 0));
        }
        self.cooldowns.retain(|(_, until)| frame.tick < *until);
        let release_pending = std::mem::take(&mut self.release_pending);
        self.end_use(frame, &mut outcome, release_pending);
        if self.active.is_none() && (pressed || frame.held) {
            self.try_use(frame, pressed, &mut outcome);
        }
        if !frame.held {
            self.repeat_armed = false;
        }
        outcome
    }

    fn end_use(&mut self, frame: &UseFrame, outcome: &mut UseOutcome, release_pending: bool) {
        let Some(active) = &self.active else {
            return;
        };
        let selection = match &frame.selection {
            Some(current)
                if current.slot != active.selection.slot
                    || current.item.network_id() != active.selection.item.network_id() =>
            {
                // Switching away stops the use without a release, as vanilla does.
                self.active = None;
                return;
            }
            Some(current) => current.clone(),
            // An in-flight inventory request hides the stack; keep the one the use began with.
            None => active.selection.clone(),
        };
        let depleted =
            frame.tick.saturating_sub(active.started_tick) >= u64::from(active.max_ticks);
        // Queue pressure must not turn an already-observed early release into a full charge.
        if !release_pending && depleted && (frame.held || active.crossbow) {
            // A depleted use finishes locally, without a release transaction.
            // A crossbow stores its loaded projectile for the next press's pose/action.
            if active.crossbow && frame.charge_projectile.is_some() {
                self.crossbows.predict(
                    &selection,
                    frame.inventory_revision,
                    frame.charge_projectile,
                );
            }
            self.active = None;
            return;
        }
        if frame.held && !release_pending {
            return;
        }
        self.active = None;
        if let Ok(packet) = protocol::release_item_packet(held_request(&selection, frame)) {
            outcome.packets.push(packet);
            outcome.released = true;
        }
    }

    /// Why a latched press sends nothing on this frame, for the click-drop trace; `None` when it
    /// is used or no press waits.
    pub fn press_drop_reason(&self, frame: &UseFrame) -> Option<&'static str> {
        if !self.latched_press || self.active.is_some() {
            return None;
        }
        if frame.press_consumed {
            Some("consumed_by_block_or_attack")
        } else if self.delay_fix && self.last_air_use_tick == Some(frame.tick) {
            Some("use_tick_already_admitted")
        } else if self.rearm_pending(frame) {
            Some("rearm_pending")
        } else if frame.selection.is_none() {
            Some("selection_unverified")
        } else if frame
            .selection
            .as_ref()
            .is_some_and(|selection| selection.item.is_empty())
        {
            Some("no_air_use_for_item")
        } else {
            None
        }
    }

    fn try_use(&mut self, frame: &UseFrame, pressed: bool, outcome: &mut UseOutcome) {
        let air_use = self.crossbows.air_use(frame);
        if frame.press_consumed
            || (self.delay_fix && self.last_air_use_tick == Some(frame.tick))
            || self.rearm_pending(frame)
            || (!pressed
                && (!self.repeat_armed
                    || air_use.is_some_and(|air_use| !air_use.repeats_while_held())))
        {
            return;
        }
        let Some(selection) = self.displayed_selection(frame) else {
            return;
        };
        self.slot_change_pending = false;
        self.last_air_use_tick = Some(frame.tick);
        self.rearm_millis = Some(frame.now_millis.saturating_add(USE_REARM_MILLIS));
        // Vanilla opens a legacy request scope on every air use.
        let legacy_request_id = self.next_legacy_request_id();
        let on_cooldown = air_use
            .and_then(AirUse::cooldown)
            .is_some_and(|cooldown| self.on_cooldown(cooldown.category));
        let mut change = None;
        let mut active_use = None;
        let mut predicted_stack = None;
        let mut emptied_slot = None;
        let mut throw_cooldown = None;
        let mut swung = false;
        match air_use {
            Some(AirUse::Hold {
                max_ticks,
                slowdown,
                ..
            }) if frame.ready => {
                active_use = Some(ActiveUse {
                    selection: selection.clone(),
                    started_tick: frame.tick,
                    max_ticks,
                    slowdown,
                    crossbow: crossbow::is_crossbow(air_use),
                });
            }
            Some(AirUse::Throw { cooldown }) if !on_cooldown => {
                swung = true;
                if let Some(Cooldown { category, ticks }) = cooldown {
                    throw_cooldown = Some((category, frame.tick.saturating_add(u64::from(ticks))));
                }
                if !frame.creative {
                    let to = selection.item.less_one(legacy_request_id);
                    if to.is_empty() {
                        emptied_slot = frame
                            .inventory_revision
                            .map(|revision| (selection.slot, revision));
                    }
                    predicted_stack = frame.selection.as_ref().map(|server| PredictedStack {
                        slot: selection.slot,
                        server: server.item.clone(),
                        stack: to.clone(),
                    });
                    change = Some(PredictedSlotChange {
                        legacy_request_id,
                        from: selection.item.clone(),
                        to,
                    });
                }
            }
            _ => {}
        }
        if let Ok(packet) = protocol::click_air_packet(held_request(&selection, frame), change) {
            outcome.packets.push(packet);
            outcome.used = true;
            outcome.started = active_use.is_some();
            self.active = active_use;
            outcome.swung = swung;
            if let Some(cooldown) = throw_cooldown {
                self.cooldowns.push(cooldown);
            }
            if predicted_stack.is_some() {
                self.predicted = predicted_stack;
            }
            if emptied_slot.is_some() {
                self.emptied_slot = emptied_slot;
            }
            if air_use == Some(AirUse::Instant) {
                self.crossbows
                    .predict(&selection, frame.inventory_revision, None);
            }
        }
    }

    /// Retains the normal repeat gate after the slot change's first admitted air use.
    fn rearm_pending(&self, frame: &UseFrame) -> bool {
        !(self.delay_fix && self.slot_change_pending)
            && self
                .rearm_millis
                .is_some_and(|rearm| frame.now_millis <= rearm)
    }

    /// The selected stack with an unconfirmed throw applied; `None` when nothing is held.
    fn displayed_selection(&mut self, frame: &UseFrame) -> Option<FrozenMiningSelection> {
        let server = frame.selection.as_ref()?;
        let predicted = self
            .predicted
            .as_ref()
            .filter(|predicted| predicted.slot == server.slot && predicted.server == server.item);
        let selection = match predicted {
            Some(predicted) => FrozenMiningSelection {
                slot: server.slot,
                item: predicted.stack.clone(),
            },
            None => {
                self.predicted = None;
                server.clone()
            }
        };
        (!selection.item.is_empty()).then_some(selection)
    }

    fn on_cooldown(&self, category: &str) -> bool {
        self.cooldowns.iter().any(|(active, _)| *active == category)
    }

    /// Vanilla legacy request ids: even ids from -4 downward, restarting past the range.
    pub fn next_legacy_request_id(&mut self) -> i32 {
        let current = if self.last_legacy_request_id < -2 {
            self.last_legacy_request_id
        } else {
            -2
        };
        self.last_legacy_request_id = current.checked_sub(2).unwrap_or(-4);
        self.last_legacy_request_id
    }
}

pub use inventory::crossbow_animation_frame;

fn held_request(selection: &FrozenMiningSelection, frame: &UseFrame) -> HeldItemRequest {
    HeldItemRequest {
        selected_slot: selection.slot,
        selected_item: selection.item.clone(),
        player_position: frame.position,
    }
}

/// Commits accepted item use and its exact unsent movement tick in one gameplay phase.
/// Packet admission precedes the tick flag; rejected admission preserves the existing retry rules.
pub fn admit_on_tick(
    runtime: &mut ItemUseRuntime,
    swings: &mut SwingTracker,
    movement: &mut crate::movement::MovementTicker,
    frame: &UseFrame,
    local_runtime_id: u64,
    swing_duration: i32,
    send: impl FnOnce(Vec<protocol::Packet>) -> Result<(), BatchSendError>,
) {
    if step_and_send(
        runtime,
        swings,
        frame,
        local_runtime_id,
        swing_duration,
        send,
    ) {
        movement.mark_started_using_item(frame.tick);
    }
}

#[cfg(test)]
mod tests;
