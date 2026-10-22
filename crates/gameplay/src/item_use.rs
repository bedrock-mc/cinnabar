//! Accepted item-use state and atomic same-tick transaction admission.
//!
//! Vanilla item use handles build actions, rearms after air use, and sends use
//! and release transactions.
//! Extraction retains the accepted-use timing and transport rollback rules unchanged.
use crate::{BatchSendError, melee::SwingTracker, mining::FrozenMiningSelection};
use protocol::{HeldItemRequest, PredictedSlotChange, VerifiedNetworkItemStack};
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
mod admission;
pub mod classify;
mod crossbow;
mod progress;
mod server_flag;
pub use admission::step_and_send;
use assets::item_use::ItemUseRequirement as Needs;
pub use classify::{AirUse, classify};
use client_world::ResolvedUseCooldown as Cooldown;

/// Vanilla re-arms the next build action this long after an air use.
const USE_REARM_MILLIS: u64 = 200;
/// Minimum held ticks before a Riptide release can launch the player.
pub const RIPTIDE_CHARGE_TICKS: u64 = 10;

#[derive(Debug, Clone, PartialEq)]
struct ActiveUse {
    selection: FrozenMiningSelection,
    started_tick: u64,
    max_ticks: u32,
    slowdown: f64,
    crossbow: bool,
    riptide: bool,
    last_progress_tick: Option<u64>,
    /// The use button started or has held this use, so button-up releases it.
    /// A use a server flag started is released only after the button held it.
    button_owned: bool,
}

/// Stack and environment facts for a selected trident.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RiptideUse {
    pub level: u8,
    pub wet: bool,
    pub mounted: bool,
    pub usable: bool,
}

/// The effective release-time `[pitch, yaw]` in protocol degrees and its aim policy.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct RiptideFacing {
    pub rotation: [f32; 2],
    pub assisted: bool,
}

/// An admitted launch carrying the same facing as its preceding action input.
/// Movement still validates the raw level against its collision-query budget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RiptideLaunch {
    pub level: u8,
    pub rotation: [f32; 2],
}

/// One observed Riptide release, retained until admission or cancellation.
#[derive(Debug, Clone, PartialEq)]
struct RiptideRelease {
    selection: FrozenMiningSelection,
    level: Option<u8>,
    facing: RiptideFacing,
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
    pub riptide: Option<RiptideUse>,
    /// Effective action facing at this frame; non-Riptide item use ignores it.
    pub riptide_facing: RiptideFacing,
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
    pub charge_projectile: Option<assets::item_use::ItemProjectileRole>,
    /// A block interaction or recent attack consumed this press.
    pub press_consumed: bool,
}

/// Transactions in send order, plus what the local player did on this tick.
#[derive(Debug, Default)]
pub struct UseOutcome {
    pub packets: Vec<protocol::Packet>,
    pub started: bool,
    /// A projectile use swung the arm; its swing packet precedes `packets`.
    pub swung: bool,
    used: bool,
    released: bool,
}

/// Unadmitted press state belongs to either a legacy input owner or one ordered attempt.
#[derive(Debug, Default, Clone)]
pub struct PendingUseInput {
    latched_press: bool,
    /// A rejected click retries only while its verified selection remains current.
    deferred_selection: Option<FrozenMiningSelection>,
    /// Only this owner's rejected batch may retry a published tick.
    rejected_swing_tick: Option<(
        u64,
        Option<(SessionGeneration, PositionAuthorityGeneration)>,
    )>,
}

impl PendingUseInput {
    /// Starts one resolved edge without publishing its latch to held-use maintenance.
    pub fn new(pressed: bool) -> Self {
        Self {
            latched_press: pressed,
            ..Self::default()
        }
    }
}

/// The press latch, the accepted use, cooldowns and the throw prediction.
#[derive(Debug, Default, Clone)]
pub struct ItemUseRuntime {
    pending: PendingUseInput,
    delay_fix: bool,
    selected_slot: Option<u8>,
    slot_change_pending: bool,
    last_air_use_tick: Option<u64>,
    last_frame_tick: Option<u64>,
    active: Option<ActiveUse>,
    session: Option<u64>,
    rearm_millis: Option<u64>,
    /// Cooldown category and the tick it ends.
    cooldowns: Vec<(client_world::UseCooldownCategory, u64)>,
    predicted: Option<PredictedStack>,
    /// The admitted throw change and its source revision, until the inventory ledger takes it.
    inventory_change: Option<(u8, u64, PredictedSlotChange)>,
    /// The use button has stayed down since a press no block interaction consumed.
    repeat_armed: bool,
    /// A rejected release still precedes the next use, even if Use is pressed again.
    release_pending: bool,
    crossbows: crossbow::CrossbowPredictions,
    /// Launch retained only after the release transaction was admitted.
    riptide_launch: Option<RiptideLaunch>,
    /// Selection, eligibility and facing captured together at the first release edge.
    riptide_release: Option<RiptideRelease>,
    /// Server `UsingItem` value awaiting a movement tick to reconcile with.
    server_using: Option<bool>,
    /// Gameplay had input focus on the last observed frame.
    gameplay_focused: bool,
}

impl ItemUseRuntime {
    /// Takes one admitted launch for the next successful movement tick.
    pub fn take_riptide_launch(&mut self) -> Option<RiptideLaunch> {
        self.riptide_launch.take()
    }
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

    /// A newly admitted selection ends a charge without inventing a release transaction.
    pub fn retire_reselected_use(&mut self, slot: u8) {
        self.observe_selected_slot(Some(slot));
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.selection.slot != slot)
        {
            self.active = None;
            self.riptide_release = None;
            self.release_pending = false;
            self.server_using = None;
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
    ) -> Option<Option<assets::item_use::ItemProjectileRole>> {
        self.crossbows.projectile_for_stack(slot, revision, stack)
    }

    /// Whether a use started locally and has not ended.
    pub const fn is_using(&self) -> bool {
        self.active.is_some()
    }

    /// Whether an accepted use has observed button-up or still owes its deferred release.
    pub fn is_releasing(&self, held: bool) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| (active.button_owned && !held) || self.release_pending)
    }

    /// Reports an observed release that still belongs to the accepted use.
    pub const fn has_pending_release(&self) -> bool {
        self.release_pending
    }

    /// Returns the first release-facing decision while its packet still awaits admission.
    pub fn riptide_release_facing(&self) -> Option<RiptideFacing> {
        self.riptide_release.as_ref().map(|release| release.facing)
    }

    /// Observes release facts on the local clock before any interaction or facing input can wait.
    /// Returns true only when this call captures the first accepted release observation.
    pub fn observe_riptide_release(
        &mut self,
        tick: u64,
        held: bool,
        riptide: Option<RiptideUse>,
        selection: Option<&FrozenMiningSelection>,
        facing: RiptideFacing,
    ) -> bool {
        let Some(active) = self.active.as_ref().filter(|active| active.riptide) else {
            return false;
        };
        if !self.is_releasing(held) {
            return false;
        }
        if active.release_selection_changed(selection, !self.release_pending) {
            self.active = None;
            self.release_pending = false;
            self.riptide_release = None;
        } else if self.riptide_release.is_none() {
            self.riptide_release = Some(RiptideRelease {
                selection: selection.unwrap_or(&active.selection).clone(),
                level: active.riptide_launch(tick, riptide, selection),
                facing,
            });
            self.release_pending = true;
            return true;
        }
        false
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

    /// Transfers the canonical pending input between an attempt and the active admission call.
    pub fn swap_pending_input(&mut self, pending: &mut PendingUseInput) {
        std::mem::swap(&mut self.pending, pending);
    }

    pub fn observe_press(&mut self, pressed: bool) {
        if pressed {
            self.pending.deferred_selection = None;
            self.pending.rejected_swing_tick = None;
            self.pending.latched_press = true;
        }
    }

    /// Screens, focus loss and spectator mode cancel queued clicks, but accepted uses release.
    pub fn cancel_pending_input(&mut self) {
        self.pending.latched_press = false;
        self.repeat_armed = false;
        self.pending.deferred_selection = None;
        self.pending.rejected_swing_tick = None;
    }

    /// Drops old-life charges and unsent use actions, preserving inventory and cooldown policy.
    pub fn cancel_for_respawn(&mut self) {
        self.cancel_pending_input();
        self.active = None;
        self.last_frame_tick = None;
        self.release_pending = false;
        self.riptide_launch = None;
        self.riptide_release = None;
        self.server_using = None;
    }

    /// Clears session-owned use state after disconnect or session replacement.
    fn cancel(&mut self) {
        self.delay_fix = false;
        self.selected_slot = None;
        self.slot_change_pending = false;
        self.last_air_use_tick = None;
        self.cancel_for_respawn();
        self.rearm_millis = None;
        self.cooldowns.clear();
        self.predicted = None;
        self.inventory_change = None;
        self.crossbows.clear();
    }

    /// Takes an admitted throw's inventory change once, including nonempty remainders.
    pub fn take_inventory_change(&mut self) -> Option<(u8, u64, PredictedSlotChange)> {
        self.inventory_change.take()
    }

    /// Whether a press or observed release still awaits packet admission.
    pub const fn pending_edge(&self) -> bool {
        self.pending.latched_press || self.release_pending
    }

    /// Whether this frame has anything to resolve against an unsent tick.
    pub const fn has_work(&self, held: bool) -> bool {
        self.pending.latched_press || self.active.is_some() || held
    }

    /// Resolves after completed inputs drain, against the next tick and its preceding pose.
    /// Presses and releases act immediately; held use advances once per completed tick.
    pub fn frame_sample(
        &self,
        movement: &crate::movement::MovementTicker,
        held: bool,
        between_ticks: bool,
    ) -> Option<crate::movement::InteractionSample> {
        let edge = self.pending.latched_press || self.release_pending || self.is_releasing(held);
        let ticked = self
            .last_frame_tick
            .is_some_and(|tick| movement.completed_tick() >= tick);
        (between_ticks && (edge || ticked))
            .then(|| movement.between_ticks_sample())
            .flatten()
    }

    /// Ends a use on release, depletion or reselection, then resolves a press or held repeat.
    pub fn step(&mut self, frame: &UseFrame) -> UseOutcome {
        self.observe_riptide_release(
            frame.tick,
            frame.held,
            frame.riptide,
            frame.selection.as_ref(),
            frame.riptide_facing,
        );
        self.last_frame_tick = Some(frame.tick);
        let was_using = self.active.is_some();
        self.observe_selected_slot(frame.selection.as_ref().map(|selection| selection.slot));
        let mut outcome = UseOutcome::default();
        let pressed = std::mem::take(&mut self.pending.latched_press);
        if pressed {
            self.repeat_armed = !frame.press_consumed
                && (frame.air_use.is_some()
                    || frame
                        .selection
                        .as_ref()
                        .is_none_or(|selection| selection.item.block_runtime_id() == 0));
        }
        self.cooldowns.retain(|(_, until)| frame.tick < *until);
        if frame.held
            && let Some(active) = self.active.as_mut()
        {
            active.button_owned = true;
        }
        let release_pending = std::mem::take(&mut self.release_pending);
        self.end_use(frame, &mut outcome, release_pending);
        if self.active.is_none() && (pressed || frame.held) {
            self.try_use(frame, pressed, &mut outcome);
        }
        if !frame.held {
            self.repeat_armed = false;
        }
        if was_using != self.active.is_some() {
            // A local start or end writes the item-use flag after any server write.
            self.server_using = None;
        }
        outcome
    }

    fn end_use(&mut self, frame: &UseFrame, outcome: &mut UseOutcome, release_pending: bool) {
        let Some(active) = &self.active else {
            return;
        };
        let mut selection = match &frame.selection {
            Some(_)
                if active.release_selection_changed(
                    frame.selection.as_ref(),
                    !frame.held && !release_pending,
                ) =>
            {
                // Switching away stops the use without a release, as vanilla does.
                self.active = None;
                self.riptide_release = None;
                return;
            }
            Some(current) => current.clone(),
            // An in-flight inventory request hides the stack; keep the one the use began with.
            None => active.selection.clone(),
        };
        let depleted =
            frame.tick.saturating_sub(active.started_tick) >= u64::from(active.max_ticks);
        // Queue pressure must not turn an already-observed early release into a full charge.
        if !release_pending && depleted && (frame.held || active.crossbow || !active.button_owned) {
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
        if (frame.held || !active.button_owned) && !release_pending {
            return;
        }
        let launch = self.riptide_release.take().and_then(|release| {
            selection = release.selection;
            release.level.map(|level| RiptideLaunch {
                level,
                rotation: release.facing.rotation,
            })
        });
        self.active = None;
        if let Ok(packet) = protocol::release_item_packet(held_request(&selection, frame)) {
            outcome.packets.push(packet);
            outcome.released = true;
            self.riptide_launch = launch;
        }
    }

    /// Why a latched press sends nothing on this frame, for the click-drop trace; `None` when it
    /// is used or no press waits.
    pub fn press_drop_reason(&self, frame: &UseFrame) -> Option<&'static str> {
        if !self.pending.latched_press || self.active.is_some() {
            return None;
        }
        if frame.press_consumed {
            Some("consumed_by_block_or_attack")
        } else if self.use_tick_already_admitted(frame) {
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
            || self.use_tick_already_admitted(frame)
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
        // Vanilla opens a legacy request scope on every air use.
        let Some(legacy_request_id) = protocol::next_legacy_request_id() else {
            return;
        };
        self.slot_change_pending = false;
        self.last_air_use_tick = Some(frame.tick);
        self.rearm_millis = Some(frame.now_millis.saturating_add(USE_REARM_MILLIS));
        let cooldown = air_use.and_then(AirUse::cooldown);
        let on_cooldown = cooldown.is_some_and(|cooldown| self.on_cooldown(cooldown.category));
        let mut change = None;
        let mut active_use = None;
        let mut predicted_stack = None;
        let mut swung = false;
        match air_use {
            Some(AirUse::Hold {
                max_ticks,
                slowdown,
                ..
            }) if frame.ready
                && !on_cooldown
                && frame.riptide.is_none_or(|riptide| {
                    riptide.usable && (riptide.level == 0 || riptide.wet)
                }) =>
            {
                active_use = Some(ActiveUse {
                    selection: selection.clone(),
                    started_tick: frame.tick,
                    max_ticks,
                    slowdown,
                    crossbow: crossbow::is_crossbow(air_use),
                    riptide: frame.riptide.is_some_and(|riptide| riptide.level > 0),
                    last_progress_tick: None,
                    button_owned: true,
                });
            }
            Some(AirUse::Throw { .. }) if !on_cooldown => {
                swung = true;
                if !frame.creative {
                    let to = selection.item.less_one(legacy_request_id);
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
            Some(AirUse::GlideRocket) if frame.ready => swung = true,
            _ => {}
        }
        let inventory_change = change.as_ref().and_then(|change| {
            frame
                .inventory_revision
                .map(|revision| (selection.slot, revision, change.clone()))
        });
        if let Ok(packet) = protocol::click_air_packet(held_request(&selection, frame), change) {
            outcome.packets.push(packet);
            outcome.used = true;
            outcome.started = active_use.is_some();
            self.active = active_use;
            outcome.swung = swung;
            if (outcome.started || swung)
                && let Some(Cooldown { category, ticks }) = cooldown
            {
                self.cooldowns
                    .push((category, frame.tick.saturating_add(u64::from(ticks))));
            }
            if predicted_stack.is_some() {
                self.predicted = predicted_stack;
            }
            if inventory_change.is_some() {
                self.inventory_change = inventory_change;
            }
            if matches!(air_use, Some(AirUse::Instant { .. })) {
                self.crossbows
                    .predict(&selection, frame.inventory_revision, None);
            }
        }
    }

    /// Blocks repeated admissions in the same slot without swallowing a slot-change use.
    fn use_tick_already_admitted(&self, frame: &UseFrame) -> bool {
        self.delay_fix && self.last_air_use_tick == Some(frame.tick) && !self.slot_change_pending
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

    fn on_cooldown(&self, category: client_world::UseCooldownCategory) -> bool {
        self.cooldowns.iter().any(|(active, _)| *active == category)
    }
}

impl ActiveUse {
    /// Unverified inventory preserves a use; a different selected slot or item cancels it.
    fn selection_changed(&self, selection: Option<&FrozenMiningSelection>) -> bool {
        selection.is_some_and(|selection| {
            selection.slot != self.selection.slot
                || selection.item.network_id() != self.selection.item.network_id()
        })
    }

    /// Adds count admission for a fresh release while retaining held and retry policies.
    fn release_selection_changed(
        &self,
        selection: Option<&FrozenMiningSelection>,
        check_count: bool,
    ) -> bool {
        self.selection_changed(selection)
            || (check_count
                && selection
                    .is_some_and(|selection| selection.item.count() != self.selection.item.count()))
    }

    /// Resolves launch eligibility from this charge and the facts observed at release.
    fn riptide_launch(
        &self,
        tick: u64,
        riptide: Option<RiptideUse>,
        selection: Option<&FrozenMiningSelection>,
    ) -> Option<u8> {
        riptide
            .filter(|riptide| {
                self.riptide
                    && riptide.level > 0
                    && riptide.wet
                    && !riptide.mounted
                    && riptide.usable
                    && selection.is_some()
                    && !self.release_selection_changed(selection, true)
                    && tick.saturating_sub(self.started_tick) >= RIPTIDE_CHARGE_TICKS
            })
            .map(|riptide| riptide.level)
    }
}

fn held_request(selection: &FrozenMiningSelection, frame: &UseFrame) -> HeldItemRequest {
    HeldItemRequest {
        hand: protocol::ItemUseHand::Mainhand,
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

#[cfg(any(test, feature = "test-support"))]
pub mod fixtures;
#[cfg(test)]
mod tests;
