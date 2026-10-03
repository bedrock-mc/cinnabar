//! Air item use: the click-air transaction every held item sends, and holding, releasing and
//! throwing.
//!
//! Follows `ClientInputCallbacks::handleBuildAction`, `GameMode::baseUseItem`,
//! `GameMode::releaseUsingItem` and `Player::completeUsingItem`; projectiles, food effects and
//! ammunition stay server-owned.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use client_world::{LocalItemUse, WorldStream};
use protocol::{HeldItemRequest, PlayerGameMode, PredictedSlotChange, VerifiedNetworkItemStack};
use semantic_input::Action;

use crate::{
    block_use::{BlockUseRuntime, verified_use_selection},
    melee::{MeleeRuntime, SwingTracker, swing_duration},
    menu::MenuRuntime,
    mining::FrozenMiningSelection,
    movement::{LocalMovementEffectTimeline, MovementTicker},
    runtime::{
        network::{BatchSendError, NetworkHandle},
        world::ClientWorld,
    },
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

mod admission;
mod classify;
mod crossbow;
use admission::step_and_send;
pub(crate) use classify::{AirUse, Cooldown, Needs, classify};

const QUICK_CHARGE_ENCHANTMENT_ID: i16 = 35;
/// `handleBuildAction` re-arms the next build action this long after an air use.
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
pub(crate) struct UseFrame {
    pub(crate) tick: u64,
    pub(crate) now_millis: u64,
    pub(crate) position: [f32; 3],
    pub(crate) held: bool,
    pub(crate) selection: Option<FrozenMiningSelection>,
    pub(crate) air_use: Option<AirUse>,
    /// The use's `Needs` are met (always true in creative).
    pub(crate) ready: bool,
    pub(crate) creative: bool,
    /// Authoritative writes, including identical rejected-charge corrections.
    pub(crate) inventory_revision: Option<u64>,
    pub(crate) charge_projectile: Option<&'static str>,
    /// A block interaction or recent attack consumed this press.
    pub(crate) press_consumed: bool,
}

/// Transactions in send order, plus what the local player did on this tick.
#[derive(Debug, Default)]
pub(crate) struct UseOutcome {
    pub(crate) packets: Vec<protocol::Packet>,
    pub(crate) started: bool,
    /// A throw swung the arm; its swing packet precedes `packets`.
    pub(crate) swung: bool,
    used: bool,
    released: bool,
}

/// The press latch, the accepted use, cooldowns and the throw prediction.
#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct ItemUseRuntime {
    latched_press: bool,
    active: Option<ActiveUse>,
    session: Option<u64>,
    rearm_millis: Option<u64>,
    /// Cooldown category and the tick it ends.
    cooldowns: Vec<(&'static str, u64)>,
    predicted: Option<PredictedStack>,
    /// The use button has stayed down since a press no block interaction consumed.
    repeat_armed: bool,
    /// A rejected click retries only while its verified selection remains current.
    deferred_selection: Option<FrozenMiningSelection>,
    /// A rejected release still precedes the next use, even if Use is pressed again.
    release_pending: bool,
    /// `TypedClientNetId<ItemStackLegacyRequestIdTag>`'s process-wide counter.
    last_legacy_request_id: i32,
    crossbows: crossbow::CrossbowPredictions,
}

impl ItemUseRuntime {
    /// Whether a use started locally and has not ended.
    pub(crate) const fn is_using(&self) -> bool {
        self.active.is_some()
    }

    /// Native attachables read remaining ticks, not the actor animation's elapsed seconds.
    pub(crate) fn render_input(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
        tick: u64,
        frame_alpha: f32,
    ) -> client_world::AttachableAnimationInput<'static> {
        let max_use_ticks = self.active.as_ref().map_or_else(
            // Native CrossbowItem::getMaxUseDuration remains its charge duration
            // when loaded; Instant describes the next action, not that query.
            || match selected_air_use_with_projectile(player_runtime, stream, ui, Some(None)) {
                Some(AirUse::Hold { max_ticks, .. }) => max_ticks,
                _ => 0,
            },
            |active| active.max_ticks,
        );
        let use_elapsed_ticks = self.active.as_ref().map(|active| {
            tick.saturating_sub(active.started_tick)
                .min(u64::from(active.max_ticks)) as u32
        });
        let selected = ui
            .selected_stack(player_runtime)
            .and_then(|stack| stream.canonical_item_stack(stack));
        let projectile = self
            .crossbows
            .selected_projectile(player_runtime, ui)
            .unwrap_or_else(|| {
                selected
                    .as_ref()
                    .and_then(|item| item.charged_projectile.as_deref())
            });
        let charged = projectile.is_some();
        let animation_frame = if selected
            .as_ref()
            .and_then(|item| item.identifier.as_deref())
            == Some("minecraft:crossbow")
        {
            let firework = ui
                .gameplay_hud()
                .offhand_stack()
                .and_then(|stack| stream.canonical_item_stack(stack))
                .is_some_and(|item| {
                    item.identifier.as_deref() == Some("minecraft:firework_rocket")
                });
            crossbow_animation_frame(use_elapsed_ticks, max_use_ticks, projectile, firework)
        } else {
            ranged_animation_frame(use_elapsed_ticks)
        };
        client_world::AttachableAnimationInput {
            first_person: true,
            frame_alpha,
            use_elapsed_ticks,
            max_use_ticks,
            hand_charged: charged,
            animation_frame,
            ..Default::default()
        }
    }

    /// Movement-input factor while a use runs; `None` when idle.
    pub(crate) fn movement_modifier(&self) -> Option<f64> {
        self.active.as_ref().map(|active| active.slowdown)
    }

    /// The same native frame used by the attachable, for a player-inventory cell.
    pub(crate) fn inventory_animation_frame(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
        slot: u8,
        tick: u64,
    ) -> Option<u32> {
        let stack = ui.inventory_ledger(player_runtime).displayed_stack(slot)?;
        let canonical = stream.canonical_item_stack(stack)?;
        if canonical.identifier.as_deref() != Some("minecraft:crossbow") {
            return None;
        }
        if ui.selected_hotbar_slot(player_runtime) == Some(slot) {
            return Some(
                self.render_input(player_runtime, stream, ui, tick, 0.0)
                    .animation_frame,
            );
        }
        let projectile = self
            .crossbows
            .slot_projectile(player_runtime, ui, slot)
            .unwrap_or(canonical.charged_projectile.as_deref());
        Some(crossbow_animation_frame(None, 0, projectile, false))
    }

    /// A new session drops the press, the use, cooldowns and the prediction without packets.
    pub(crate) fn synchronize(&mut self, session: u64) {
        if self.session.is_some_and(|previous| previous != session) {
            self.cancel();
        }
        self.session = Some(session);
    }

    pub(crate) fn observe_press(&mut self, pressed: bool) {
        if pressed {
            self.deferred_selection = None;
            self.latched_press = true;
        }
    }

    /// Screens, focus loss and spectator mode cancel queued clicks, but accepted uses release.
    pub(crate) fn cancel_pending_input(&mut self) {
        self.latched_press = false;
        self.repeat_armed = false;
        self.deferred_selection = None;
    }

    /// Clears session-owned use state after disconnect or session replacement.
    fn cancel(&mut self) {
        self.cancel_pending_input();
        self.active = None;
        self.rearm_millis = None;
        self.cooldowns.clear();
        self.predicted = None;
        self.release_pending = false;
        self.crossbows.clear();
    }

    /// Whether this frame has anything to resolve against an unsent tick.
    pub(crate) const fn has_work(&self, held: bool) -> bool {
        self.latched_press || self.active.is_some() || held
    }

    /// Ends a use on release, depletion or reselection, then resolves a press or held repeat.
    pub(crate) fn step(&mut self, frame: &UseFrame) -> UseOutcome {
        let mut outcome = UseOutcome::default();
        let pressed = std::mem::take(&mut self.latched_press);
        if pressed {
            self.repeat_armed = !frame.press_consumed;
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
                // Switching away stops the use without a release, as `Player::stopUsingItem`.
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
            // `completeUsingItem` finishes locally, without a release transaction.
            // CrossbowItem stores its loaded projectile for the next press's pose/action.
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
    pub(crate) fn press_drop_reason(&self, frame: &UseFrame) -> Option<&'static str> {
        if !self.latched_press || self.active.is_some() {
            return None;
        }
        if frame.press_consumed {
            Some("consumed_by_block_or_attack")
        } else if self
            .rearm_millis
            .is_some_and(|rearm| frame.now_millis <= rearm)
        {
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
            || self
                .rearm_millis
                .is_some_and(|rearm| frame.now_millis <= rearm)
            || (!pressed
                && (!self.repeat_armed
                    || air_use.is_some_and(|air_use| !air_use.repeats_while_held())))
        {
            return;
        }
        let Some(selection) = self.displayed_selection(frame) else {
            return;
        };
        self.rearm_millis = Some(frame.now_millis.saturating_add(USE_REARM_MILLIS));
        // `baseUseItem` opens a legacy request scope on every air use.
        let legacy_request_id = self.next_legacy_request_id();
        let on_cooldown = air_use
            .and_then(AirUse::cooldown)
            .is_some_and(|cooldown| self.on_cooldown(cooldown.category));
        let mut change = None;
        let mut active_use = None;
        let mut predicted_stack = None;
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
            if air_use == Some(AirUse::Instant) {
                self.crossbows
                    .predict(&selection, frame.inventory_revision, None);
            }
        }
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

    /// `TypedClientNetId::_generateNext`: even ids from -4 downward, restarting past the range.
    fn next_legacy_request_id(&mut self) -> i32 {
        let current = if self.last_legacy_request_id < -2 {
            self.last_legacy_request_id
        } else {
            -2
        };
        self.last_legacy_request_id = current.checked_sub(2).unwrap_or(-4);
        self.last_legacy_request_id
    }

    /// The local rig's use flag: set while a use runs, cleared while a held-use item idles.
    pub(crate) fn local_item_use(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
    ) -> LocalItemUse {
        if self.active.is_some() {
            return LocalItemUse::Using;
        }
        match selected_air_use_with_projectile(
            player_runtime,
            stream,
            ui,
            self.crossbows.selected_projectile(player_runtime, ui),
        ) {
            Some(AirUse::Hold { .. } | AirUse::Instant) => LocalItemUse::Idle,
            Some(AirUse::Throw { .. }) | None => LocalItemUse::Unpredicted,
        }
    }
}

/// RangedWeaponItem::getAnimationFrame (26.50 RVA 034e78f0): the icon follows
/// the quadratic draw-power curve, independently of the attachable's charge pose.
fn ranged_animation_frame(elapsed: Option<u32>) -> u32 {
    let Some(elapsed) = elapsed else {
        return 0;
    };
    let seconds = elapsed as f32 / 20.0;
    let power = ((seconds * seconds + 2.0 * seconds) / 3.0).min(1.0);
    (3.0 * power * 0.99) as u32 + 1
}

/// CrossbowItem::getAnimationFrame (26.50 RVA 09a137a0), including loaded projectile art.
pub(crate) fn crossbow_animation_frame(
    elapsed: Option<u32>,
    duration: u32,
    projectile: Option<&str>,
    offhand_firework: bool,
) -> u32 {
    if let Some(elapsed) = elapsed.filter(|_| duration > 0) {
        let fraction = elapsed as f32 / duration as f32;
        let power = ((fraction * fraction + 2.0 * fraction) / 3.0).min(1.0);
        let frame = (power * 0.99 * 5.0) as u32;
        if frame >= 4 && power < 1.0 && offhand_firework {
            5
        } else {
            frame
        }
    } else {
        projectile.map_or(0, |projectile| {
            if projectile == "minecraft:arrow" {
                4
            } else {
                5
            }
        })
    }
}

fn held_request(selection: &FrozenMiningSelection, frame: &UseFrame) -> HeldItemRequest {
    HeldItemRequest {
        selected_slot: selection.slot,
        selected_item: selection.item.clone(),
        player_position: frame.position,
    }
}

/// The selected stack's authoritative air use, if supported.
pub(crate) fn selected_air_use(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &WorldStream,
    ui: &UiRuntime,
) -> Option<AirUse> {
    selected_air_use_with_projectile(player_runtime, stream, ui, None)
}

fn selected_air_use_with_projectile(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &WorldStream,
    ui: &UiRuntime,
    projectile_override: Option<Option<&str>>,
) -> Option<AirUse> {
    let stack = ui.selected_stack(player_runtime)?;
    let canonical = stream.canonical_item_stack(stack)?;
    let identifier = canonical.identifier.as_deref()?;
    let quick_charge =
        protocol::item_enchantment_level(&stack.extra_data, QUICK_CHARGE_ENCHANTMENT_ID)
            .unwrap_or(0);
    let pack_ticks = stream.item_max_use_ticks(identifier).or_else(|| {
        classify::pack_identifier(identifier).and_then(|pack| stream.item_max_use_ticks(pack))
    });
    classify(
        identifier,
        projectile_override.map_or(canonical.charged_projectile.is_some(), |projectile| {
            projectile.is_some()
        }),
        quick_charge,
        pack_ticks,
    )
}

/// The use duration of the selected stack when vanilla animates its use as eating or drinking.
pub(crate) fn consume_ticks(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &WorldStream,
    ui: &UiRuntime,
) -> Option<u32> {
    let canonical = stream.canonical_item_stack(ui.selected_stack(player_runtime)?)?;
    match selected_air_use(player_runtime, stream, ui)? {
        AirUse::Hold { max_ticks, .. }
            if classify::is_consumed(canonical.identifier.as_deref()?) =>
        {
            Some(max_ticks)
        }
        _ => None,
    }
}

/// Whether the known state meets `needs`.
fn needs_met(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &WorldStream,
    ui: &UiRuntime,
    needs: Needs,
) -> bool {
    let is = |stack: &protocol::NetworkItemStack, identifier: &str| {
        !stack.is_empty()
            && stream
                .item_identifier(stack.network_id)
                .is_some_and(|name| &*name == identifier)
    };
    let arrow_in_inventory = || {
        let ledger = ui.inventory_ledger(player_runtime);
        (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ledger.displayed_stack(slot))
            .chain(ui.gameplay_hud().offhand_stack())
            .any(|stack| is(stack, "minecraft:arrow"))
    };
    match needs {
        Needs::Nothing => true,
        Needs::Arrow => arrow_in_inventory(),
        Needs::ArrowOrOffhandRocket => {
            arrow_in_inventory()
                || ui
                    .gameplay_hud()
                    .offhand_stack()
                    .is_some_and(|stack| is(stack, "minecraft:firework_rocket"))
        }
        // Peaceful difficulty's always-edible rule is not modeled.
        Needs::Appetite => ui
            .hud()
            .hunger()
            .is_none_or(|hunger| hunger.current() < hunger.maximum()),
    }
}

#[derive(SystemParam)]
pub(crate) struct ItemUseContext<'w, 's> {
    input: Res<'w, SemanticInputSnapshot>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    melee: Res<'w, MeleeRuntime>,
    block_use: Res<'w, BlockUseRuntime>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    network: Res<'w, NetworkHandle>,
    time: Res<'w, Time<Real>>,
}

/// Runs after block use so a press that interacted with a block starts no item use.
pub(crate) fn produce_item_use(
    player_runtime: bevy::prelude::Res<crate::player_runtime::PlayerRuntime>,
    context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut swings: ResMut<SwingTracker>,
) {
    runtime.synchronize(context.ui.session_id());
    let focused =
        !context.menu.is_visible() && context.windows.single().is_ok_and(|window| window.focused);
    let use_phase = context.input.phase(Action::Use);
    let admitted = if context.input.snapshot().is_none() {
        false
    } else if !focused || context.ui.ui_focused(&player_runtime) {
        use_phase
            .pressed
            .then(|| crate::movement::note_click_drop("use", "screen_open"));
        false
    } else if context
        .ui
        .game_mode_capabilities(&player_runtime)
        .is_some_and(|caps| !caps.can_use_items)
    {
        use_phase
            .pressed
            .then(|| crate::movement::note_click_drop("use", "spectator"));
        false
    } else {
        true
    };
    if !admitted {
        runtime.cancel_pending_input();
    }
    runtime.observe_press(admitted && use_phase.pressed);
    let held = admitted && use_phase.held;
    let Some(stream) = context.client_world.stream.as_ref() else {
        return;
    };
    if !runtime.has_work(held) {
        return;
    }
    // Frames between physics ticks have no unsent tick; the press waits for one.
    let Some(sample) = movement.newest_unsent_sample() else {
        return;
    };
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let air_use = selected_air_use_with_projectile(
        &player_runtime,
        stream,
        &context.ui,
        runtime
            .crossbows
            .selected_projectile(&player_runtime, &context.ui),
    );
    let creative = context.ui.player_game_mode(&player_runtime) == Some(PlayerGameMode::Creative);
    let frame = UseFrame {
        tick: sample.tick,
        now_millis,
        position: sample.position,
        held,
        selection: verified_use_selection(&player_runtime, &context.ui),
        air_use,
        ready: match air_use {
            Some(AirUse::Hold { needs, .. }) => {
                creative || needs_met(&player_runtime, stream, &context.ui, needs)
            }
            _ => false,
        },
        creative,
        inventory_revision: context
            .ui
            .selected_hotbar_slot(&player_runtime)
            .and_then(|slot| {
                context
                    .ui
                    .inventory_ledger(&player_runtime)
                    .authoritative_slot_revision(slot)
            }),
        charge_projectile: crossbow::loading_projectile(
            &player_runtime,
            stream,
            &context.ui,
            creative,
        ),
        press_consumed: context.melee.blocks_use_at(now_millis)
            || context.block_use.interacted_at(sample.tick),
    };
    if let Some(reason) = runtime.press_drop_reason(&frame) {
        crate::movement::note_click_drop("use", reason);
    }
    let duration = swing_duration(context.effects.mining_effects());
    if step_and_send(
        &mut runtime,
        &mut swings,
        &frame,
        stream.local_player_runtime_id(),
        duration,
        |packets| context.network.send_inventory_packets(packets),
    ) {
        movement.mark_started_using_item(sample.tick);
    }
}

#[cfg(test)]
mod tests;
