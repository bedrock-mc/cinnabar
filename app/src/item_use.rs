//! Air item use: the click-air transaction every held item sends, and holding, releasing and
//! throwing.
//!
//! Follows vanilla's build action, air use, release and use completion; projectiles,
//! food effects and ammunition stay server-owned.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Query, Real, Res, ResMut, Resource, Time, Window, With},
    window::PrimaryWindow,
};
use chunk_pipeline::WorldStream;
use client_world::LocalItemUse;
use protocol::PlayerGameMode;
use semantic_input::Action;

use crate::{
    block_use::{BlockUseRuntime, verified_use_selection},
    melee::{MeleeRuntime, SwingTracker, swing_duration},
    menu::MenuRuntime,
    movement::{LocalMovementEffectTimeline, MovementTicker},
    runtime::{network::NetworkHandle, world::ClientWorld},
    semantic_controls::SemanticInputSnapshot,
};
use client_ui::ui_runtime::UiRuntime;

pub(crate) use gameplay::item_use::UseFrame;
pub(crate) use gameplay::item_use::{AirUse, Needs, classify, crossbow_animation_frame};
use gameplay::item_use::{QUICK_CHARGE_ENCHANTMENT_ID, admit_on_tick};
use inventory::ranged_animation_frame;

/// A successful extension request is valid only in its originating world scope.
#[derive(Resource, Debug, Default)]
pub(crate) struct ModItemUsePolicy {
    pub scope: Option<(u64, i32)>,
}

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
            // A crossbow's maximum use duration remains its charge duration
            // when loaded; Instant describes the next action, not that query.
            || match selected_air_use_with_projectile(player_runtime, stream, Some(None)) {
                Some(AirUse::Hold { max_ticks, .. }) => max_ticks,
                _ => 0,
            },
            |(_, max_ticks)| max_ticks,
        );
        let use_elapsed_ticks = self.active_timing().map(|(started_tick, max_ticks)| {
            tick.saturating_sub(started_tick).min(u64::from(max_ticks)) as u32
        });
        let selected = player_runtime
            .selected_stack()
            .and_then(|stack| stream.authority().canonical_item_stack(stack));
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
                .and_then(|stack| stream.authority().canonical_item_stack(stack))
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
        let canonical = stream.authority().canonical_item_stack(stack)?;
        if canonical.identifier.as_deref() != Some("minecraft:crossbow") {
            return None;
        }
        if player_runtime.selected_hotbar_slot() == Some(slot) {
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
        let slot = player_runtime.selected_hotbar_slot()?;
        self.predicted_projectile(
            slot,
            ui.inventory_ledger(player_runtime)
                .authoritative_slot_revision(slot)?,
            player_runtime.selected_stack()?,
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
) -> Option<AirUse> {
    selected_air_use_with_projectile(player_runtime, stream, None)
}

fn selected_air_use_with_projectile(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &WorldStream,
    projectile_override: Option<Option<&str>>,
) -> Option<AirUse> {
    let stack = player_runtime.selected_stack()?;
    let canonical = stream.authority().canonical_item_stack(stack)?;
    let identifier = canonical.identifier.as_deref()?;
    let quick_charge =
        protocol::item_enchantment_level(&stack.extra_data, QUICK_CHARGE_ENCHANTMENT_ID)
            .unwrap_or(0);
    let pack_ticks = stream
        .authority()
        .item_max_use_ticks(identifier)
        .or_else(|| {
            classify::pack_identifier(identifier)
                .and_then(|pack| stream.authority().item_max_use_ticks(pack))
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
) -> Option<u32> {
    let canonical = stream
        .authority()
        .canonical_item_stack(player_runtime.selected_stack()?)?;
    match selected_air_use(player_runtime, stream)? {
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
                .authority()
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
    delay_fix: Option<Res<'w, ModItemUsePolicy>>,
    ui: Res<'w, UiRuntime>,
    menu: Res<'w, MenuRuntime>,
    windows: Query<'w, 's, &'static Window, With<PrimaryWindow>>,
    client_world: Res<'w, ClientWorld>,
    melee: Res<'w, MeleeRuntime>,
    block_use: Res<'w, BlockUseRuntime>,
    effects: Res<'w, LocalMovementEffectTimeline>,
    network: Res<'w, NetworkHandle>,
    time: Res<'w, Time<Real>>,
    aim: Res<'w, client_presentation::aim_assist::AimAssistFrame>,
    camera: Res<'w, crate::camera::ServerCameraView>,
}

/// Runs after block use so a press that interacted with a block starts no item use.
pub(crate) fn produce_item_use(
    mut player_runtime: ResMut<crate::player_runtime::PlayerRuntime>,
    context: ItemUseContext,
    mut runtime: ResMut<ItemUseRuntime>,
    mut movement: ResMut<MovementTicker>,
    mut swings: ResMut<SwingTracker>,
    mut view: ResMut<crate::local_player::LocalViewPose>,
) {
    swings.sync_ticks_for_item(
        movement.interaction_authority_identity(),
        movement.completed_tick(),
        &context.effects,
        crate::melee::selected_attack_timing(&player_runtime, &context.client_world)
            .and_then(|timing| timing.swing_duration_ticks),
    );

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
    } else if player_runtime
        .facts
        .game_mode_capabilities()
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
    let scope = context.client_world.stream.as_ref().map(|stream| {
        let authority = stream.authority();
        (authority.actor_session_id(), authority.current_dimension())
    });
    let delay_fix_enabled = admitted
        && scope.is_some()
        && context
            .delay_fix
            .as_ref()
            .is_some_and(|policy| policy.scope == scope);
    runtime.set_delay_fix(delay_fix_enabled);
    runtime.observe_selected_slot(player_runtime.selected_hotbar_slot());
    runtime.observe_press(admitted && use_phase.pressed);
    movement.send_held_release(|packets| context.network.send_inventory_packets(packets));
    if movement.has_held_release() {
        return;
    }
    let held = admitted && use_phase.held;
    let Some(stream) = context.client_world.stream.as_ref() else {
        return;
    };
    if !runtime.has_work(held) {
        return;
    }
    let aim_rotation = crate::camera::aim_assist::action_rotation(&context.aim, &context.camera);
    // An aim-assist facing needs an unsent tick to carry it.
    let Some(sample) = runtime.frame_sample(&movement, held, aim_rotation.is_none()) else {
        return;
    };
    // One press resolves once: item use waits while block use still holds it.
    if context.block_use.press_pending() {
        return;
    }
    let now_millis = u64::try_from(context.time.elapsed().as_millis()).unwrap_or(u64::MAX);
    let air_use = selected_air_use_with_projectile(
        &player_runtime,
        stream,
        runtime.selected_projectile(&player_runtime, &context.ui),
    );
    let creative = player_runtime.facts.player_game_mode() == Some(PlayerGameMode::Creative);
    let inventory_revision = player_runtime.selected_hotbar_slot().and_then(|slot| {
        context
            .ui
            .inventory_ledger(&player_runtime)
            .authoritative_slot_revision(slot)
    });
    let selection = verified_use_selection(&player_runtime, &context.ui).map(|server| {
        context
            .block_use
            .inventory
            .predicted_selection(&server, inventory_revision.unwrap_or(0))
            .unwrap_or(server)
    });
    let frame = UseFrame {
        tick: sample.tick,
        now_millis,
        position: sample.position,
        held,
        selection,
        air_use,
        ready: match air_use {
            Some(AirUse::Hold { needs, .. }) => {
                creative || needs_met(&player_runtime, stream, &context.ui, needs)
            }
            _ => false,
        },
        creative,
        inventory_revision,
        charge_projectile: loading_projectile(&player_runtime, stream, &context.ui, creative),
        press_consumed: (!delay_fix_enabled && context.melee.blocks_use_at(now_millis))
            || context.block_use.press_interacted(),
    };
    if let Some(reason) = runtime.press_drop_reason(&frame) {
        crate::movement::note_click_drop("use", reason);
    }
    let duration = swing_duration(
        context
            .effects
            .mining_tick(sample.tick, movement.completed_tick())
            .0,
    );
    admit_with_action_aim(
        &mut runtime,
        &mut swings,
        &mut movement,
        &mut view,
        &frame,
        stream.local_player_runtime_id(),
        duration,
        aim_rotation,
        &context.network,
    );
    if let Some((slot, revision)) = runtime.take_emptied_slot() {
        player_runtime
            .inventory
            .ledger_mut()
            .settle_use_emptied_slot(slot, revision);
    }
}

/// Admits this tick's use. A release aim assist turns the player for waits until this tick's
/// input carries the new facing, since the server launches with the facing it last received.
#[allow(clippy::too_many_arguments)]
fn admit_with_action_aim(
    runtime: &mut gameplay::item_use::ItemUseRuntime,
    swings: &mut gameplay::melee::SwingTracker,
    movement: &mut MovementTicker,
    view: &mut crate::local_player::LocalViewPose,
    frame: &UseFrame,
    local_runtime_id: u64,
    swing_duration: i32,
    aim_rotation: Option<bevy::prelude::Quat>,
    network: &NetworkHandle,
) {
    let mut assisted = None;
    admit_on_tick(
        runtime,
        swings,
        movement,
        frame,
        local_runtime_id,
        swing_duration,
        |packets| {
            if aim_rotation.is_some() && packets.iter().any(protocol::is_aim_assist_rotation_action)
            {
                assisted = Some(packets);
                return Ok(());
            }
            network.send_inventory_packets(packets)
        },
    );
    if let (Some(rotation), Some(packets)) = (aim_rotation, assisted) {
        crate::camera::aim_assist::apply_action_rotation(rotation, view, movement, frame.tick);
        movement.hold_release_after_tick(frame.tick, packets);
    }
}

/// Release checks the offhand for either projectile first, then inventory
/// arrows, and synthesizes an arrow only in creative.
fn loading_projectile(
    player_runtime: &crate::player_runtime::PlayerRuntime,
    stream: &chunk_pipeline::WorldStream,
    ui: &UiRuntime,
    creative: bool,
) -> Option<&'static str> {
    let name = |stack: &protocol::NetworkItemStack| {
        (!stack.is_empty())
            .then(|| stream.authority().item_identifier(stack.network_id))
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
