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
use protocol::PlayerGameMode;
use semantic_input::Action;

use crate::{
    block_use::{BlockUseRuntime, verified_use_selection},
    melee::{MeleeRuntime, SwingTracker, swing_duration},
    menu::MenuRuntime,
    movement::{LocalMovementEffectTimeline, MovementTicker},
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

pub(crate) use gameplay::item_use::UseFrame;
pub(crate) use gameplay::item_use::{
    AirUse, Needs, classify, crossbow_animation_frame, ranged_animation_frame,
};
use gameplay::item_use::{QUICK_CHARGE_ENCHANTMENT_ID, admit_on_tick};

/// Bevy resource adapter for the gameplay item_use owner.
#[derive(Resource, Debug, Default, Clone)]
pub(crate) struct ItemUseRuntime(gameplay::item_use::ItemUseRuntime);
impl std::ops::Deref for ItemUseRuntime {
    type Target = gameplay::item_use::ItemUseRuntime;
    /// Borrows the gameplay owner at the existing ordered system boundary.
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for ItemUseRuntime {
    /// Mutates the gameplay owner without duplicating its state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl ItemUseRuntime {
    /// Exposes accepted-use state to the existing presentation adapters.
    pub(crate) fn is_using(&self) -> bool {
        self.0.is_using()
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
        let max_use_ticks = self.active_timing().map_or_else(
            // Native CrossbowItem::getMaxUseDuration remains its charge duration
            // when loaded; Instant describes the next action, not that query.
            || match selected_air_use_with_projectile(player_runtime, stream, ui, Some(None)) {
                Some(AirUse::Hold { max_ticks, .. }) => max_ticks,
                _ => 0,
            },
            |(_, max_ticks)| max_ticks,
        );
        let use_elapsed_ticks = self.active_timing().map(|(started_tick, max_ticks)| {
            tick.saturating_sub(started_tick).min(u64::from(max_ticks)) as u32
        });
        let selected = ui
            .selected_stack(player_runtime)
            .and_then(|stack| stream.canonical_item_stack(stack));
        let projectile = self
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
            .slot_projectile(player_runtime, ui, slot)
            .unwrap_or(canonical.charged_projectile.as_deref());
        Some(crossbow_animation_frame(None, 0, projectile, false))
    }

    /// The local rig's use flag: set while a use runs, cleared while a held-use item idles.
    pub(crate) fn local_item_use(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        stream: &WorldStream,
        ui: &UiRuntime,
    ) -> LocalItemUse {
        if self.is_using() {
            return LocalItemUse::Using;
        }
        match selected_air_use_with_projectile(
            player_runtime,
            stream,
            ui,
            self.selected_projectile(player_runtime, ui),
        ) {
            Some(AirUse::Hold { .. } | AirUse::Instant) => LocalItemUse::Idle,
            Some(AirUse::Throw { .. }) | None => LocalItemUse::Unpredicted,
        }
    }
    /// Resolves a charge prediction against the currently selected authoritative stack.
    fn selected_projectile(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        ui: &UiRuntime,
    ) -> Option<Option<&'static str>> {
        let slot = ui.selected_hotbar_slot(player_runtime)?;
        self.predicted_projectile(
            slot,
            ui.inventory_ledger(player_runtime)
                .authoritative_slot_revision(slot)?,
            ui.selected_stack(player_runtime)?,
        )
    }
    /// Resolves a charge prediction against an authoritative inventory slot.
    fn slot_projectile(
        &self,
        player_runtime: &crate::player_runtime::PlayerRuntime,
        ui: &UiRuntime,
        slot: u8,
    ) -> Option<Option<&'static str>> {
        self.predicted_projectile(
            slot,
            ui.inventory_ledger(player_runtime)
                .authoritative_slot_revision(slot)?,
            ui.inventory_ledger(player_runtime).displayed_stack(slot)?,
        )
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
        runtime.selected_projectile(&player_runtime, &context.ui),
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
        charge_projectile: loading_projectile(&player_runtime, stream, &context.ui, creative),
        press_consumed: context.melee.blocks_use_at(now_millis)
            || context.block_use.interacted_at(sample.tick),
    };
    if let Some(reason) = runtime.press_drop_reason(&frame) {
        crate::movement::note_click_drop("use", reason);
    }
    let duration = swing_duration(context.effects.mining_effects());
    admit_on_tick(
        &mut runtime,
        &mut swings,
        &mut movement,
        &frame,
        stream.local_player_runtime_id(),
        duration,
        |packets| context.network.send_inventory_packets(packets),
    );
}

/// `releaseUsing` checks the offhand for either projectile first, then inventory
/// arrows, and synthesizes an arrow only in creative (09a157e0).
fn loading_projectile(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &client_world::WorldStream,
    ui: &UiRuntime,
    creative: bool,
) -> Option<&'static str> {
    let name = |stack: &protocol::NetworkItemStack| {
        (!stack.is_empty())
            .then(|| stream.item_identifier(stack.network_id))
            .flatten()
    };
    if let Some(offhand) = ui.gameplay_hud().offhand_stack().and_then(name) {
        match &*offhand {
            "minecraft:firework_rocket" => return Some("minecraft:firework_rocket"),
            "minecraft:arrow" => return Some("minecraft:arrow"),
            _ => {}
        }
    }
    (creative
        || (0..protocol::PLAYER_INVENTORY_SLOTS)
            .filter_map(|slot| ui.inventory_ledger(player_runtime).displayed_stack(slot))
            .filter_map(name)
            .any(|identifier| &*identifier == "minecraft:arrow"))
    .then_some("minecraft:arrow")
}

#[cfg(test)]
#[path = "item_use/tests/crossbow_presentation.rs"]
mod crossbow_presentation_tests;

#[cfg(test)]
mod session_tests;
