//! Inventory input routing for world shortcuts, open screens and pointer gestures.
use super::dispatch_bound_inventory_hotbar;
use super::drop_swing::DropSwing;
use crate::runtime::network::resource_packs::PackApplication;
use bevy::input::gamepad::Gamepad;
use bevy::prelude::{KeyCode, Query, Res, ResMut, Single, With};
use bevy::window::PrimaryWindow;
use bevy::{
    input::mouse::MouseScrollUnit,
    prelude::{ButtonInput, MouseButton, Time},
    time::Real,
    window::Window,
};
use client_runtime::network::NetworkHandle;
use client_shell::menu::settings_options::control_bindings::binding_keyboard_key;
use client_shell::menu::settings_options::control_bindings::{binding_gamepad, binding_pressed};
use client_shell::ui_resources::{UiPresentationResource, UiRuntimeResource};
use client_ui::ui_runtime::interaction::dispatch_inventory_key;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};
use ui::UiPoint;
use ui_screens::container_actions::{InventoryCellHit, Widget};
use {
    client_ui::ui_runtime::{
        UiRuntime,
        presentation::{
            UiPresentationRuntime, forms::recipe_book_shown, inventory_pointer::InventoryScreen,
        },
    },
    inventory::inventory_ledger::DropSource,
};

use launcher::menu::settings_options::BindingId;

/// Consumes Back on a vanilla inventory, saving book edits before closing it.
pub(super) fn cancel_from_mouse_back(
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    presentation: Option<&UiPresentationRuntime>,
) -> bool {
    if !runtime.inventory_open()
        || presentation
            .is_some_and(|view| view.mod_ui().mod_view_shown() || view.mod_ui().mod_text_focused())
    {
        return false;
    }
    if runtime.commit_book(false) {
        runtime.close_inventory(player);
    }
    true
}

/// In-world inventory keys: Q drops one item from the selected hotbar cell
/// (Control+Q the whole stack) and a right-click with a book in hand opens it.
#[allow(
    clippy::too_many_arguments,
    reason = "Player authority is borrowed separately from UI state."
)]
pub(crate) fn drive_world_inventory_keys(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    gamepads: Query<&Gamepad>,
    window: Single<&Window, With<PrimaryWindow>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    presentation: Option<Res<UiPresentationResource>>,
    mut runtime: ResMut<UiRuntimeResource>,
    emote_input: Option<Res<client_state::EmoteInputConsumed>>,
    swing: DropSwing,
    touch: Option<Res<crate::semantic_controls::SemanticTouchTargets>>,
    book_transport: (
        Option<Res<client_interaction::movement::MovementTicker>>,
        Option<Res<NetworkHandle<PackApplication>>>,
    ),
) {
    if emote_input.is_some_and(|consumed| consumed.0) {
        return;
    }
    let drop = binding_pressed(menu.as_deref(), BindingId::Drop, &keys, &mouse)
        || binding_gamepad(menu.as_deref(), BindingId::Drop, &gamepads);
    let use_book = binding_pressed(menu.as_deref(), BindingId::Use, &keys, &mouse)
        || touch.is_some_and(|touch| touch.pressed(semantic_input::touch::USE));
    if !window.focused
        || client_shell::screen_policy::absorbs_input(
            &player_runtime,
            Some(&runtime),
            menu.as_deref(),
            presentation.as_deref().map(|presentation| &presentation.0),
        )
        || !(drop || use_book)
        || player_runtime
            .facts
            .player_game_mode()
            .is_some_and(|mode| !mode.shows_hotbar())
    {
        return;
    }
    let (movement, network) = book_transport;
    if (use_book
        && runtime.open_held_book(
            &mut player_runtime,
            movement
                .as_deref()
                .and_then(|movement| movement.build_action_state())
                .map(|state| state.position),
            |packet| {
                network
                    .as_deref()
                    .is_some_and(|network| network.send_inventory_packet(packet).is_ok())
            },
        ))
        || !drop
    {
        return;
    }
    let Some(slot) = player_runtime.selected_hotbar_slot() else {
        return;
    };
    let control = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    if runtime
        .inventory_ledger_mut(&mut player_runtime)
        .begin_world_drop(slot, (!control).then_some(1))
        .is_ok()
    {
        swing.swing(&player_runtime);
    }
}

/// Applies presses routed to the open inventory screen, after any keys already queued for it.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_screen_presses(
    buttons: &[MouseButton],
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    window: &Window,
    presentation: Option<&UiPresentationRuntime>,
    menu: Option<&client_shell::menu::MenuRuntime>,
    focus: Option<&mut client_state::CursorFocus>,
    time: &Time<Real>,
) {
    let Some(presentation) =
        presentation.filter(|_| !buttons.is_empty() && runtime.inventory_open())
    else {
        return;
    };
    let mut pressed = ButtonInput::<MouseButton>::default();
    for button in buttons {
        pressed.press(*button);
    }
    let frame = runtime.inventory_keys_mut().take_frame();
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    apply_inventory_pointer(
        player_runtime,
        runtime,
        window,
        presentation,
        menu,
        &mut pressed,
        (false, false),
        frame,
        &[],
        focus,
        now_millis,
    );
}

/// Applies one frame's buttons, key presses and wheel notches to the open inventory screen.
#[allow(clippy::too_many_arguments)]
pub(super) fn apply_inventory_pointer(
    player_runtime: &mut client_state::PlayerRuntime,
    runtime: &mut UiRuntime,
    window: &Window,
    presentation: &UiPresentationRuntime,
    menu: Option<&client_shell::menu::MenuRuntime>,
    mouse_buttons: &mut ButtonInput<MouseButton>,
    (raw_primary_release, raw_secondary_release): (bool, bool),
    (presses, shift, control): (Vec<semantic_input::KeyboardKey>, bool, bool),
    notches: &[(f32, MouseScrollUnit)],
    mut focus: Option<&mut client_state::CursorFocus>,
    now_millis: u64,
) {
    let primary_pressed = mouse_buttons.just_pressed(MouseButton::Left);
    let secondary_pressed = mouse_buttons.just_pressed(MouseButton::Right);
    let primary_released = mouse_buttons.just_released(MouseButton::Left) || raw_primary_release;
    let secondary_released =
        mouse_buttons.just_released(MouseButton::Right) || raw_secondary_release;
    let mouse_hotbar: Vec<_> = mouse_buttons
        .get_just_pressed()
        .filter_map(|button| {
            client_shell::physical_input::mouse_button_code(*button)
                .map(semantic_input::PhysicalControl::MouseButton)
        })
        .collect();
    // Resolve inventory pointer edges before clearing the buttons for gameplay.
    mouse_buttons.reset_all();
    runtime.observe_inventory_window(player_runtime);
    let screen = InventoryScreen::of_runtime(player_runtime, runtime);
    runtime.observe_inventory_search_focus(
        player_runtime,
        presentation.engine_container_frame().is_some(),
    );
    let Some(position) = window.cursor_position() else {
        // A cancelled touch withdraws the pointer, so its release closes nothing.
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
        runtime.screen_state_mut().container_close_armed = false;
        return;
    };
    let Ok(point) = UiPoint::new(position.x, position.y) else {
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
        runtime.screen_state_mut().container_close_armed = false;
        return;
    };
    let physical_size = [window.physical_width(), window.physical_height()];
    let gui = presentation.inventory_gui_point(point, physical_size, window.scale_factor());
    runtime.set_inventory_pointer_gui(gui);
    // A player mod's view, or a control of its overlay, owns the pointer (`modding`).
    let mod_owned = presentation
        .mod_ui()
        .mod_screens_own([point.x(), point.y()]);
    if mod_owned || screen == InventoryScreen::Book {
        runtime.screen_state_mut().container_close_armed = false;
    }
    // The screen's close button closes it on a release over the button it was pressed on.
    // A mod's view hides the container, so the hidden close button must not arm.
    let on_close = gui
        .filter(|_| !mod_owned)
        .and_then(|gui| presentation.engine_container_hit(gui))
        == Some(InventoryCellHit::Widget(Widget::Close));
    if primary_pressed {
        runtime.screen_state_mut().container_close_armed = on_close;
    }
    if runtime.screen_state().container_close_armed {
        if primary_released {
            runtime.screen_state_mut().container_close_armed = false;
            if on_close {
                let _ = runtime.close_inventory(player_runtime);
                if let Some(focus) = focus.as_deref_mut() {
                    focus.authorize_screen_return();
                }
            }
        }
        return;
    }
    let book_open = recipe_book_shown(player_runtime, runtime);
    let reader_mode = runtime
        .screen_state()
        .book
        .as_ref()
        .map(|book| (book.editable, book.signing));
    let hit = gui.filter(|_| !mod_owned).and_then(|gui| {
        if let Some((editable, signing)) = reader_mode {
            return presentation.inventory_reader_hit(
                gui,
                physical_size,
                window.scale_factor(),
                editable,
                signing,
            );
        }
        presentation
            .inventory_book_hit(gui, physical_size, window.scale_factor(), screen, book_open)
            .or_else(|| {
                presentation.inventory_cell_hit(gui, physical_size, window.scale_factor(), screen)
            })
    });
    runtime.screen_state_mut().hover = hit;
    if let (Some(gui), Some(frame), false) = (gui, presentation.engine_container_frame(), mod_owned)
    {
        scroll_container(runtime, frame, gui, notches);
    }
    for key in presses {
        if key.usage().is_some_and(|code| {
            dispatch_bound_inventory_hotbar(
                menu,
                semantic_input::PhysicalControl::KeyboardUsage(code),
                player_runtime,
                runtime,
                hit,
            )
        }) {
            continue;
        }
        let drop = binding_keyboard_key(menu, BindingId::Drop, key);
        let _ = dispatch_inventory_key(player_runtime, runtime, hit, key, control, None, drop);
    }
    for control in mouse_hotbar {
        dispatch_bound_inventory_hotbar(menu, control, player_runtime, runtime, hit);
    }
    let frame = crate::ui_runtime::inventory_drag::PointerFrame {
        primary_pressed,
        primary_released,
        secondary_pressed,
        secondary_released,
        shift,
        holding: runtime
            .inventory_ledger(player_runtime)
            .cursor_stack()
            .is_some(),
        hit,
        now_millis,
    };
    let actions = runtime.screen_state_mut().pointer.step(frame);
    for action in actions {
        let was_open = runtime.inventory_open();
        runtime.perform_pointer_action(player_runtime, action);
        if was_open
            && !runtime.inventory_open()
            && let Some(focus) = focus.as_deref_mut()
        {
            focus.authorize_screen_return();
        }
    }
    if hit.is_none() && !mod_owned {
        // A held stack clicked off the screen's panels is dropped: all of it
        // on a primary click, one item on a secondary click.
        let outside = gui.is_some_and(|gui| {
            presentation.inventory_press_drops(gui, physical_size, window.scale_factor(), screen)
        });
        if outside && (primary_pressed || secondary_pressed) {
            let amount = (!primary_pressed).then_some(1);
            let _ = runtime
                .inventory_ledger_mut(player_runtime)
                .begin_drop(DropSource::Cursor, amount);
        }
    }
}

#[cfg(test)]
mod tests;

/// Wheel notches over an engine-drawn screen scroll the view under the pointer.
pub(super) fn scroll_container(
    runtime: &mut UiRuntime,
    frame: &ui_engine::frame::EngineFrame,
    gui: [f32; 2],
    notches: &[(f32, MouseScrollUnit)],
) {
    let point = [f64::from(gui[0]), f64::from(gui[1])];
    let Some(view) = json_ui::scroll_target(&frame.hits, &frame.report, point) else {
        return;
    };
    let Some(metrics) = frame.report.scrolls.get(&view.key) else {
        return;
    };
    let mut offset = runtime
        .screen_state()
        .container_scroll
        .get(&view.key)
        .copied()
        .unwrap_or(metrics.offset);
    for (notch, unit) in notches {
        let at = json_ui::ScrollMetrics {
            offset,
            ..metrics.clone()
        };
        offset = match unit {
            MouseScrollUnit::Line => at.offset_for_wheel(f64::from(*notch)),
            MouseScrollUnit::Pixel => {
                (offset - f64::from(*notch / frame.scale)).clamp(0.0, metrics.max_offset())
            }
        };
    }
    if !notches.is_empty() {
        runtime
            .screen_state_mut()
            .container_scroll
            .insert(view.key.clone(), offset);
    }
}

/// Syncs `swings` to the movement `authority` before swinging for `tick`, so the later swing
/// producers' sync this frame keeps the swing rather than resetting it.
pub(super) fn swing_before_tick(
    swings: &mut client_interaction::melee::SwingTracker,
    authority: (SessionGeneration, PositionAuthorityGeneration),
    completed_tick: u64,
    effects: &client_interaction::movement::LocalMovementEffectTimeline,
    item_duration: Option<u32>,
    tick: u64,
) -> bool {
    swings.sync_ticks_for_item(authority, completed_tick, effects, item_duration);
    swings.try_swing_before_tick_for_item(
        tick,
        effects.mining_tick(tick, completed_tick).0,
        item_duration,
    )
}

#[cfg(test)]
mod drop_swing_tests {
    use client_interaction::{melee::SwingTracker, movement::LocalMovementEffectTimeline};
    use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

    fn authority(session: u64) -> (SessionGeneration, PositionAuthorityGeneration) {
        (
            SessionGeneration::new(session),
            PositionAuthorityGeneration::new(0),
        )
    }

    /// A drop swing under a new movement authority survives the block-use producer's sync.
    #[test]
    fn a_drop_swing_survives_the_next_sync_under_a_new_authority() {
        let effects = LocalMovementEffectTimeline::default();
        let mut swings = SwingTracker::default();
        swings.sync_ticks(authority(1), 4, &effects);

        assert!(super::swing_before_tick(
            &mut swings,
            authority(2),
            4,
            &effects,
            None,
            5
        ));
        swings.sync_ticks_for_item(authority(2), 4, &effects, None);

        assert!(swings.take_started().is_some());
    }

    /// A drop keeps the selected item's Bedrock swing duration and the default Java timing.
    #[test]
    fn a_drop_swing_keeps_the_selected_items_swing_duration() {
        const ITEM_DURATION: u32 = 19;
        let effects = LocalMovementEffectTimeline::default();
        let mut authored = SwingTracker::default();
        let mut default = SwingTracker::default();

        for (swings, duration) in [(&mut authored, Some(ITEM_DURATION)), (&mut default, None)] {
            assert!(super::swing_before_tick(
                swings,
                authority(1),
                4,
                &effects,
                duration,
                5,
            ));
        }

        assert_eq!(authored.take_started(), Some(ITEM_DURATION as i32));
        let authored = authored.published_progress(6);
        let default = default.published_progress(6);
        assert_eq!(authored.bedrock[1], 1.0 / ITEM_DURATION as f32);
        assert_eq!(authored.java, default.java);
    }
}
