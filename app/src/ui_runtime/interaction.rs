use launcher::menu::settings_options::BindingId;
mod chat;
mod chat_coordinates;
pub(crate) mod chat_modifiers;
pub(super) mod command_macros;
mod drop_swing;
pub(crate) mod input_capture;
pub(crate) mod inventory;
pub(crate) use chat::drive_chat_ui_actions;

use bevy::{
    ecs::message::{MessageCursor, Messages},
    input::{
        ButtonState,
        gamepad::{Gamepad, GamepadButton},
        keyboard::KeyboardInput,
        mouse::{
            AccumulatedMouseMotion, AccumulatedMouseScroll, MouseButtonInput, MouseScrollUnit,
            MouseWheel,
        },
        touch::Touches,
    },
    math::Vec2,
    prelude::{
        ButtonInput, KeyCode, Local, MessageReader, MouseButton, Query, Res, ResMut, Single, Time,
        With,
    },
    time::Real,
    window::{CursorGrabMode, CursorOptions, PrimaryWindow, Window, WindowEvent},
};

use client_shell::menu::settings_options::control_bindings::{
    binding_gamepad, binding_key, binding_mouse, binding_mouse_button, hotbar_control_slots,
};
use ui::{ChatEditor, PointerPhase, UiAction, UiPoint};

use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use client_ui::ui_runtime::{PlatformClipboard, UiRuntime};
use ui_screens::container_actions::InventoryCellHit;

use client_shell::{
    physical_input::{keyboard_key, keyboard_usage},
    ui_resources::{UiPresentationResource, UiRuntimeResource},
};
use client_ui::ui_runtime::interaction::{
    PointerRouter, PressRoute, dispatch_chat_ui_action, dispatch_inventory_hotbar,
    is_chat_edit_shortcut, paste_chat_shortcut,
};
use input_capture::{
    ordered_pointer_presses, restore_gameplay_input_after_chat, suppress_gameplay_input_for_chat,
    suppress_gameplay_input_for_inventory,
};

/// Dispatches inventory hotbar shortcuts bound to one physical press.
pub(crate) fn dispatch_bound_inventory_hotbar(
    menu: Option<&client_shell::menu::MenuRuntime>,
    control: semantic_input::PhysicalControl,
    player: &mut player_state::PlayerState,
    runtime: &mut UiRuntime,
    hit: Option<InventoryCellHit>,
) -> bool {
    let mut matched = false;
    for slot in hotbar_control_slots(menu, control) {
        let _ = dispatch_inventory_hotbar(player, runtime, hit, slot);
        matched = true;
    }
    matched
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_inventory_ui_actions(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    time: Option<Res<Time<Real>>>,
    window: Single<&Window, With<PrimaryWindow>>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mouse_messages: Option<Res<Messages<MouseButtonInput>>>,
    mut release_cursor: Local<MessageCursor<MouseButtonInput>>,
    wheel_messages: Option<Res<Messages<MouseWheel>>>,
    mut wheel_cursor: Local<MessageCursor<MouseWheel>>,
    presentation: Res<UiPresentationResource>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    driven: Option<Res<client_state::DrivenInput>>,
) {
    let input_available = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    let notches: Vec<(f32, MouseScrollUnit)> = wheel_messages
        .as_deref()
        .map(|messages| {
            wheel_cursor
                .read(messages)
                .map(|event| (event.y, event.unit))
                .collect()
        })
        .unwrap_or_default();
    // Button state is reset below while the inventory owns the pointer, so a later
    // physical release no longer surfaces as `just_released`; read raw releases too.
    let (mut raw_primary_release, mut raw_secondary_release) = (false, false);
    if let Some(messages) = mouse_messages.as_deref() {
        for input in release_cursor.read(messages) {
            if input.state == ButtonState::Released {
                match input.button {
                    MouseButton::Left => raw_primary_release = true,
                    MouseButton::Right => raw_secondary_release = true,
                    _ => {}
                }
            }
        }
    }
    // Presses are this frame's only; modifiers stay held across frames.
    let (presses, shift, control) = runtime.inventory_keys_mut().take_frame();
    if runtime.credits().owns_input() || runtime.server_forms().owns_input() {
        runtime.screen_state_mut().container_close_armed = false;
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible())
        || !runtime.inventory_open()
        || !input_available
    {
        runtime.screen_state_mut().container_close_armed = false;
        runtime.set_inventory_pointer_gui(None);
        runtime.screen_state_mut().hover = None;
        runtime.screen_state_mut().pointer.reset();
        if !runtime.inventory_open() {
            runtime.screen_state_mut().book = None;
        }
        return;
    }
    let now_millis = time.map_or(0, |time| {
        u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)
    });
    inventory::apply_inventory_pointer(
        &mut player_runtime,
        &mut runtime,
        &window,
        &presentation,
        menu.as_deref(),
        &mut mouse_buttons,
        (raw_primary_release, raw_secondary_release),
        (presses, shift, control),
        &notches,
        focus.as_deref_mut(),
        now_millis,
    );
}

/// Routes one press against the screen in force now. A press bound to a screen toggle triggers
/// that binding and is consumed by it; the flag reports an inventory toggle.
fn route_press(
    button: MouseButton,
    router: &mut PointerRouter<MouseButton>,
    menu: Option<&client_shell::menu::MenuRuntime>,
    player: (
        &mut client_state::PlayerRuntime,
        Option<&client_world::WorldAuthority>,
    ),
    runtime: &mut UiRuntime,
    mut presentation: Option<&mut UiPresentationRuntime>,
    pointer: (Option<[f32; 2]>, f64),
) -> (PressRoute, bool) {
    let (player_runtime, authority) = player;
    if runtime.inventory_open()
        && button == MouseButton::Left
        && let Some(view) = presentation.as_deref_mut()
    {
        view.mod_ui_mut()
            .focus_mod_screen_pointer(pointer.0, pointer.1);
    }
    let mod_text = presentation
        .as_deref()
        .is_some_and(|view| view.mod_ui().mod_text_focused());
    let mod_view = presentation
        .as_deref()
        .is_some_and(|view| view.mod_ui().mod_view_shown());
    if button == MouseButton::Back
        && inventory::cancel_from_mouse_back(player_runtime, runtime, presentation.as_deref())
    {
        return (PressRoute::Binding, true);
    }
    if button == MouseButton::Back && chat::cancel_from_mouse_back(runtime, presentation) {
        return (PressRoute::Binding, false);
    }
    if !runtime.chat_focused() && (!runtime.screen_state().text_focused() || mod_view) && !mod_text
    {
        if binding_mouse_button(menu, BindingId::Inventory, button) {
            runtime.toggle_inventory(player_runtime, authority);
            return (PressRoute::Binding, true);
        }
        if !runtime.inventory_open() {
            let chat = binding_mouse_button(menu, BindingId::Chat, button);
            if chat || binding_mouse_button(menu, BindingId::Command, button) {
                runtime.open_chat(player_runtime);
                if !chat {
                    let _ = runtime.insert_chat_text("/");
                }
                return (PressRoute::Binding, false);
            }
        }
    }
    (router.route(runtime.inventory_open()), false)
}

/// Reserves vanilla container keys from mod declarations. While a mod view hides the
/// container, only the inventory key and Escape stay reserved.
pub(crate) fn inventory_consumes_key(
    menu: Option<&client_shell::menu::MenuRuntime>,
    key: KeyCode,
    view: bool,
) -> bool {
    if binding_key(menu, BindingId::Inventory, key) || key == KeyCode::Escape {
        return true;
    }
    !view
        && (binding_key(menu, BindingId::Drop, key)
            || keyboard_usage(key).is_some_and(|code| {
                hotbar_control_slots(menu, semantic_input::PhysicalControl::KeyboardUsage(code))
                    .next()
                    .is_some()
            })
            || matches!(
                key,
                KeyCode::ArrowUp | KeyCode::ArrowDown | KeyCode::PageUp | KeyCode::PageDown
            ))
}

/// Drops keyboard input another owner handles while keeping inventory modifiers in step
/// with the physical keys: observed edges still update them, and when input is unavailable
/// (an unfocused window) every modifier counts as released.
fn discard_keyboard(
    runtime: &mut UiRuntime,
    keyboard_messages: &mut MessageReader<KeyboardInput>,
    input_available: bool,
) {
    let inventory_keys = runtime.inventory_keys_mut();
    if input_available {
        for input in keyboard_messages.read() {
            inventory_keys.track_modifier(
                keyboard_key(input.key_code),
                input.state == ButtonState::Pressed,
            );
        }
    } else {
        keyboard_messages.clear();
        inventory_keys.release_modifiers();
    }
}

#[allow(
    clippy::too_many_arguments,
    clippy::type_complexity,
    reason = "Bevy limits systems to 16 parameters, so related inputs share tuples."
)]
pub(crate) fn drive_chat_keyboard_input(
    mut player_runtime: bevy::prelude::ResMut<client_state::PlayerRuntime>,
    gamepads: Query<&Gamepad>,
    mut keyboard_messages: MessageReader<KeyboardInput>,
    time: Res<Time<Real>>,
    window: Single<(&Window, &mut CursorOptions), With<PrimaryWindow>>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse_buttons: ResMut<ButtonInput<MouseButton>>,
    mut mouse_motion: ResMut<AccumulatedMouseMotion>,
    mut runtime: ResMut<UiRuntimeResource>,
    mut presentation: Option<ResMut<UiPresentationResource>>,
    mut clipboard: Option<ResMut<client_shell::menu::MenuClipboard>>,
    mut modifiers: Local<ButtonInput<KeyCode>>,
    (emote_input, driven, world): (
        Option<Res<client_state::EmoteInputConsumed>>,
        Option<Res<client_state::DrivenInput>>,
        Option<Res<crate::runtime::world::ClientWorld>>,
    ),
    mut focus: Option<ResMut<client_state::CursorFocus>>,
    (window_events, mut window_cursor, mut semantic): (
        Option<Res<Messages<WindowEvent>>>,
        Local<MessageCursor<WindowEvent>>,
        Option<ResMut<crate::semantic_controls::SemanticInputRuntime>>,
    ),
) {
    // Only the window event stream keeps keyboard and pointer events in arrival order.
    let pointer_presses = window_events
        .as_deref()
        .map(|events| ordered_pointer_presses(window_cursor.read(events)))
        .unwrap_or_default();
    let (window, mut cursor) = window.into_inner();
    let authority = world
        .as_deref()
        .and_then(|world| world.stream.as_ref())
        .map(|stream| stream.authority());
    let input_available = driven.is_some()
        || (window.focused && focus.as_ref().is_none_or(|focus| focus.available()));
    if runtime.credits().owns_input() {
        let now = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
        let finished = runtime.credits().active().is_some_and(|active| {
            presentation
                .as_ref()
                .is_some_and(|view| view.credits_finished(runtime.session_id(), active.sequence))
        });
        runtime.credits_mut().observe(now, finished);
        let had_active_credits = runtime.credits().active().is_some();
        if input_available {
            let cancel = keys.just_pressed(KeyCode::Escape)
                || mouse_buttons.just_pressed(MouseButton::Back)
                || gamepads
                    .iter()
                    .any(|pad| pad.just_pressed(GamepadButton::East));
            let select = keys.just_pressed(KeyCode::Enter)
                || keys.just_pressed(KeyCode::Space)
                || gamepads
                    .iter()
                    .any(|pad| pad.just_pressed(GamepadButton::South));
            if mouse_buttons.just_pressed(MouseButton::Left) {
                let skip_hit = runtime.credits().active().is_some_and(|active| {
                    active.skip_visible(now)
                        && window.cursor_position().is_some_and(|point| {
                            presentation.as_ref().is_some_and(|view| {
                                view.credits_skip_contains(
                                    runtime.session_id(),
                                    active.sequence,
                                    point.to_array(),
                                )
                            })
                        })
                });
                if skip_hit {
                    runtime.credits_mut().skip(now);
                } else {
                    runtime.credits_mut().select(now, false);
                }
            } else if cancel || select {
                runtime.credits_mut().select(now, cancel);
            }
        }
        if had_active_credits
            && runtime.credits().active().is_none()
            && let Some(focus) = focus.as_deref_mut()
        {
            focus.authorize_screen_return();
        }
        modifiers.reset_all();
        discard_keyboard(&mut runtime, &mut keyboard_messages, input_available);
        keys.reset_all();
        mouse_buttons.reset_all();
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    if !input_available {
        modifiers.reset_all();
        discard_keyboard(&mut runtime, &mut keyboard_messages, false);
        keys.reset_all();
        mouse_buttons.reset_all();
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    if runtime.sign_editor().is_open()
        || runtime.server_forms().owns_input()
        || runtime.emotes().is_open()
        || emote_input.is_some_and(|consumed| consumed.0)
        || presentation
            .as_ref()
            .is_some_and(|view| view.mod_ui().mod_panel_open())
    {
        modifiers.reset_all();
        discard_keyboard(&mut runtime, &mut keyboard_messages, true);
        return;
    }
    if menu.as_ref().is_some_and(|menu| menu.is_visible()) {
        modifiers.reset_all();
        if runtime.inventory_open() {
            runtime.close_inventory(&mut player_runtime);
        }
        discard_keyboard(&mut runtime, &mut keyboard_messages, true);
        // The menu system runs next and must see the original button state.
        // It consumes keyboard/pointer input after handling its own actions.
        mouse_motion.delta = Vec2::ZERO;
        cursor.grab_mode = CursorGrabMode::None;
        cursor.visible = true;
        return;
    }
    let keyboard_events: Vec<_> = keyboard_messages.read().cloned().collect();
    chat_modifiers::capture_before_events(&mut modifiers, &keys, &keyboard_events);
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
    // Without pointer arrival events, use the physical modifier state for mouse presses.
    if pointer_presses.is_empty() {
        let buttons: Vec<_> = mouse_buttons.get_just_pressed().copied().collect();
        for button in buttons {
            if command_macros::dispatch_mouse(
                menu.as_deref(),
                &mut runtime,
                chat_modifiers::held_chord(&keys),
                button,
                now_millis,
                semantic.as_deref_mut(),
            ) {
                mouse_buttons.clear_just_pressed(button);
            }
        }
    }
    // An already-open inventory owns this frame's pointer edge. Keyboard
    // transitions below may close it or open a new UI, so both sides of the
    // transition are checked before preserving that edge for the inventory
    // system later in the production chain.
    let inventory_owned_pointer = runtime.inventory_open();
    let mut inventory_ownership_changed = false;
    let mut dismissed = false;
    let mut consumed_gameplay = runtime.ui_focused(&player_runtime);
    // With ordered window events, mouse bindings act at their press's place in the sequence.
    let mouse_bindings = pointer_presses.is_empty();
    let pointer = (
        window.cursor_position().map(|point| point.to_array()),
        time.elapsed_secs_f64(),
    );
    if mouse_bindings
        && runtime.inventory_open()
        && mouse_buttons.just_pressed(MouseButton::Left)
        && let Some(view) = presentation.as_deref_mut()
    {
        view.mod_ui_mut()
            .focus_mod_screen_pointer(pointer.0, pointer.1);
    }
    let mod_text = presentation
        .as_deref()
        .is_some_and(|view| view.mod_ui().mod_text_focused());
    let mod_view = presentation
        .as_deref()
        .is_some_and(|view| view.mod_ui().mod_view_shown());
    if mouse_bindings && mouse_buttons.just_pressed(MouseButton::Back) {
        let inventory_cancelled = inventory::cancel_from_mouse_back(
            &mut player_runtime,
            &mut runtime,
            presentation.as_deref().map(|view| &view.0),
        );
        if inventory_cancelled
            || chat::cancel_from_mouse_back(
                &mut runtime,
                presentation.as_deref_mut().map(|view| &mut view.0),
            )
        {
            dismissed |= !runtime.ui_focused(&player_runtime);
            inventory_ownership_changed |= inventory_cancelled;
            consumed_gameplay = true;
            mouse_buttons.clear_just_pressed(MouseButton::Back);
        }
    }
    if !runtime.chat_focused() && (!runtime.screen_state().text_focused() || mod_view) && !mod_text
    {
        if (mouse_bindings && binding_mouse(menu.as_deref(), BindingId::Inventory, &mouse_buttons))
            || binding_gamepad(menu.as_deref(), BindingId::Inventory, &gamepads)
        {
            runtime.toggle_inventory(&mut player_runtime, authority);
            dismissed |= !runtime.inventory_open();
            inventory_ownership_changed = true;
            consumed_gameplay = true;
        } else if !runtime.inventory_open()
            && ((mouse_bindings && binding_mouse(menu.as_deref(), BindingId::Chat, &mouse_buttons))
                || binding_gamepad(menu.as_deref(), BindingId::Chat, &gamepads))
        {
            runtime.open_chat(&mut player_runtime);
            consumed_gameplay = true;
        } else if !runtime.inventory_open()
            && mouse_bindings
            && binding_mouse(menu.as_deref(), BindingId::Command, &mouse_buttons)
        {
            runtime.open_chat(&mut player_runtime);
            let _ = runtime.insert_chat_text("/");
            consumed_gameplay = true;
        }
    }
    let routes_presses = !pointer_presses.is_empty();
    let mut router = PointerRouter::new(pointer_presses, runtime.inventory_open());
    // Presses a screen consumed, and presses whose edge later owners still need.
    let (mut consumed, mut kept) = (Vec::new(), Vec::new());
    for (arrival, input) in keyboard_events.iter().enumerate() {
        // Presses that arrived before this key meet the screen the earlier inputs left.
        router.observe(runtime.inventory_open());
        while let Some(button) = router.next_before_key(arrival) {
            if command_macros::dispatch_mouse(
                menu.as_deref(),
                &mut runtime,
                chat_modifiers::held_chord(&modifiers),
                button,
                now_millis,
                semantic.as_deref_mut(),
            ) {
                mouse_buttons.clear_just_pressed(button);
                continue;
            }
            let (route, toggled_inventory) = route_press(
                button,
                &mut router,
                menu.as_deref(),
                (&mut player_runtime, authority),
                &mut runtime,
                presentation.as_deref_mut().map(|view| &mut view.0),
                pointer,
            );
            match route {
                PressRoute::Gameplay => kept.push(button),
                PressRoute::Binding => {
                    // The binding's transition holds for every later input.
                    router.observe(runtime.inventory_open());
                    dismissed |= !runtime.ui_focused(&player_runtime);
                    consumed.push(button);
                    consumed_gameplay = true;
                    if toggled_inventory {
                        inventory_ownership_changed = true;
                        dismissed |= !runtime.inventory_open();
                    }
                }
                PressRoute::Screen { .. } => {
                    consumed.push(button);
                    inventory::apply_screen_presses(
                        &[button],
                        &mut player_runtime,
                        &mut runtime,
                        window,
                        presentation.as_deref().map(|view| &view.0),
                        menu.as_deref(),
                        focus.as_deref_mut(),
                        &time,
                    );
                }
            }
        }
        chat_modifiers::track(&mut modifiers, input);
        runtime.inventory_keys_mut().track_modifier(
            keyboard_key(input.key_code),
            input.state == ButtonState::Pressed,
        );
        if input.state != ButtonState::Pressed {
            continue;
        }
        if let Some(presentation) = presentation.as_deref_mut()
            && presentation.chat_link_confirmation_open()
        {
            consumed_gameplay = true;
            if input.key_code == KeyCode::Escape && !input.repeat {
                presentation.cancel_chat_link();
            }
            continue;
        }
        if let Some(presentation) = presentation.as_deref_mut()
            && presentation.chat_settings_open()
        {
            consumed_gameplay = true;
            if input.key_code == KeyCode::Escape && !input.repeat {
                presentation.set_chat_settings_open(false);
            }
            continue;
        }
        if runtime.inventory_open() {
            consumed_gameplay = true;
            if presentation
                .as_deref()
                .is_some_and(|view| view.mod_ui().mod_text_focused())
            {
                // A player mod's edit box takes typing and Escape (`modding`).
                continue;
            }
            if runtime.screen_state().text_focused() && !mod_view {
                // A text field owns typed text, including `e`.
                match input.key_code {
                    KeyCode::Escape if !input.repeat => {
                        if runtime.commit_book(false) {
                            runtime.close_inventory(&mut player_runtime);
                            inventory_ownership_changed = true;
                        }
                    }
                    // An OS repeat of a held key is not a new press, so it never closes a screen.
                    KeyCode::Escape => {}
                    KeyCode::Backspace => runtime.screen_state_mut().backspace_text(),
                    key if runtime.book_key(&mut player_runtime, keyboard_key(key)) => {}
                    _ => {
                        let modified = chat_modifiers::text_modified(&modifiers);
                        if !modified && let Some(text) = input.text.as_deref() {
                            runtime.screen_state_mut().type_text(text);
                        }
                    }
                }
                dismissed |= !runtime.inventory_open();
                continue;
            }
            match input.key_code {
                // An OS repeat of a held key is not a new press, so it never toggles the screen.
                key if input.repeat
                    && (key == KeyCode::Escape
                        || binding_key(menu.as_deref(), BindingId::Inventory, key)) => {}
                key if binding_key(menu.as_deref(), BindingId::Inventory, key) => {
                    runtime.toggle_inventory(&mut player_runtime, authority);
                    inventory_ownership_changed = true;
                }
                // Escape returns from a player mod's view to the container, and the hidden
                // screen's own keys do nothing under the view (`modding`).
                KeyCode::Escape if mod_view => {}
                _ if mod_view => {}
                KeyCode::Escape => {
                    runtime.close_inventory(&mut player_runtime);
                    inventory_ownership_changed = true;
                }
                key => runtime.inventory_keys_mut().press(keyboard_key(key)),
            }
            dismissed |= !runtime.inventory_open();
            continue;
        }
        if runtime.local_sleeping() && !runtime.chat_focused() {
            // The bed screen: Escape leaves the bed, T opens chat over it. Key repeats do nothing.
            match input.key_code {
                _ if input.repeat => {}
                KeyCode::Escape => {
                    runtime.request_wake();
                    if let Some(focus) = focus.as_deref_mut() {
                        focus.authorize_screen_return();
                    }
                }
                key if binding_key(menu.as_deref(), BindingId::Chat, key) => {
                    runtime.open_chat(&mut player_runtime);
                }
                _ if chat_modifiers::command_key(menu.as_deref(), input) => {
                    runtime.open_chat(&mut player_runtime);
                    let _ = runtime.insert_chat_text("/");
                }
                _ => {}
            }
            continue;
        }
        if !runtime.chat_focused() {
            if input.repeat {
                continue;
            }
            if command_macros::dispatch(
                menu.as_deref(),
                &mut runtime,
                chat_modifiers::held_chord(&modifiers),
                keyboard_usage(input.key_code).map(semantic_input::PhysicalControl::KeyboardUsage),
                now_millis,
                semantic.as_deref_mut(),
            ) {
                keys.clear_just_pressed(input.key_code);
                continue;
            }
            match input.key_code {
                key if binding_key(menu.as_deref(), BindingId::Inventory, key) => {
                    runtime.toggle_inventory(&mut player_runtime, authority);
                    inventory_ownership_changed = true;
                    consumed_gameplay = true;
                }
                key if binding_key(menu.as_deref(), BindingId::Chat, key) => {
                    runtime.open_chat(&mut player_runtime);
                    consumed_gameplay = true;
                }
                _ if chat_modifiers::command_key(menu.as_deref(), input) => {
                    runtime.open_chat(&mut player_runtime);
                    let _ = runtime.insert_chat_text("/");
                    consumed_gameplay = true;
                }
                _ => {}
            }
            continue;
        }

        consumed_gameplay = true;
        let selecting =
            modifiers.pressed(KeyCode::ShiftLeft) || modifiers.pressed(KeyCode::ShiftRight);
        let held: Vec<_> = modifiers
            .get_pressed()
            .map(|key| keyboard_key(*key))
            .collect();
        if is_chat_edit_shortcut(&held) {
            match input.key_code {
                KeyCode::KeyA => {
                    runtime.mutate_chat_editor(|editor| {
                        editor.move_home(false);
                        editor.move_end(true);
                    });
                    continue;
                }
                KeyCode::KeyC => {
                    if let Some(selection) = runtime.chat_editor().selection() {
                        let text = runtime.chat_editor().as_str()[selection].to_owned();
                        if let Some(clipboard) = clipboard.as_deref_mut() {
                            clipboard.write_text(text);
                        } else {
                            let _ = PlatformClipboard.write_text(text);
                        }
                    }
                    continue;
                }
                _ => {}
            }
        }
        let pasted = if let Some(clipboard) = clipboard.as_deref_mut() {
            paste_chat_shortcut(&mut runtime, keyboard_key(input.key_code), &held, clipboard)
        } else {
            paste_chat_shortcut(
                &mut runtime,
                keyboard_key(input.key_code),
                &held,
                &mut PlatformClipboard,
            )
        };
        if pasted {
            continue;
        }
        match input.key_code {
            // Repeats keep editing the draft, but only a fresh press cancels or sends it.
            KeyCode::Escape | KeyCode::Enter | KeyCode::NumpadEnter if input.repeat => {}
            KeyCode::Escape => {
                runtime.close_chat();
            }
            KeyCode::Enter | KeyCode::NumpadEnter => {
                // Enter always sends; Tab completes.
                let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);
                if runtime.queue_chat_send(now_millis).is_ok() {
                    runtime.close_chat();
                }
            }
            key if is_chat_edit_shortcut(&held) && chat_modifiers::is_word_edit(key) => {
                runtime
                    .mutate_chat_editor(|editor| chat_modifiers::word_edit(editor, key, selecting));
            }
            KeyCode::Backspace => runtime.backspace_chat_text(),
            KeyCode::Delete => runtime.delete_chat_text(),
            KeyCode::ArrowLeft => {
                if selecting {
                    runtime.mutate_chat_editor(ChatEditor::select_left);
                } else {
                    runtime.move_chat_cursor_left();
                }
            }
            KeyCode::ArrowRight => {
                if selecting {
                    runtime.mutate_chat_editor(ChatEditor::select_right);
                } else {
                    runtime.move_chat_cursor_right();
                }
            }
            KeyCode::Home => runtime.move_chat_cursor_home(selecting),
            KeyCode::End => runtime.move_chat_cursor_end(selecting),
            KeyCode::ArrowUp => {
                if runtime.chat_suggestions().is_empty() {
                    runtime.show_older_chat_history();
                } else {
                    runtime.handle_chat_ui_action(UiAction::Navigate([0, -1]));
                }
            }
            KeyCode::ArrowDown => {
                if runtime.chat_suggestions().is_empty() {
                    runtime.show_newer_chat_history();
                } else {
                    runtime.handle_chat_ui_action(UiAction::Navigate([0, 1]));
                }
            }
            KeyCode::Tab => {
                runtime.handle_chat_ui_action(if selecting {
                    UiAction::TabPrevious
                } else {
                    UiAction::TabNext
                });
            }
            _ => {
                let modified = chat_modifiers::text_modified(&modifiers);
                if !modified
                    && let Some(text) = input.text.as_deref()
                    && !text.chars().any(char::is_control)
                {
                    let _ = runtime.insert_chat_text(text);
                }
            }
        }
        dismissed |= !runtime.chat_focused();
    }
    if routes_presses {
        // The frame's last key changed the screen for every press after it.
        router.observe(runtime.inventory_open());
        while let Some(button) = router.next_rest() {
            if command_macros::dispatch_mouse(
                menu.as_deref(),
                &mut runtime,
                chat_modifiers::held_chord(&modifiers),
                button,
                now_millis,
                semantic.as_deref_mut(),
            ) {
                mouse_buttons.clear_just_pressed(button);
                continue;
            }
            let (route, toggled_inventory) = route_press(
                button,
                &mut router,
                menu.as_deref(),
                (&mut player_runtime, authority),
                &mut runtime,
                presentation.as_deref_mut().map(|view| &mut view.0),
                pointer,
            );
            match route {
                PressRoute::Binding => {
                    // The binding's transition holds for every later input.
                    router.observe(runtime.inventory_open());
                    dismissed |= !runtime.ui_focused(&player_runtime);
                    consumed.push(button);
                    consumed_gameplay = true;
                    if toggled_inventory {
                        inventory_ownership_changed = true;
                        dismissed |= !runtime.inventory_open();
                    }
                }
                // Apply screen presses in arrival order before a later press can close it.
                PressRoute::Screen { .. } => {
                    consumed.push(button);
                    inventory::apply_screen_presses(
                        &[button],
                        &mut player_runtime,
                        &mut runtime,
                        window,
                        presentation.as_deref().map(|view| &view.0),
                        menu.as_deref(),
                        focus.as_deref_mut(),
                        &time,
                    );
                }
                PressRoute::Gameplay => kept.push(button),
            }
        }
        // A press a screen consumed cannot be replayed; gameplay presses keep their edge.
        for button in consumed {
            if !kept.contains(&button) {
                mouse_buttons.clear_just_pressed(button);
            }
        }
    }

    if dismissed && let Some(focus) = focus.as_deref_mut() {
        focus.authorize_screen_return();
    }
    if consumed_gameplay {
        if inventory_owned_pointer && !inventory_ownership_changed && runtime.inventory_open() {
            suppress_gameplay_input_for_inventory(
                &runtime,
                &mut cursor,
                &mut keys,
                &mut mouse_motion,
            );
        } else {
            suppress_gameplay_input_for_chat(
                &player_runtime,
                &runtime,
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
        // A send/cancel closes chat before suppression, but that same physical
        // key must still be consumed for the current frame.
        if !runtime.ui_focused(&player_runtime) {
            restore_gameplay_input_after_chat(
                &mut cursor,
                &mut keys,
                &mut mouse_buttons,
                &mut mouse_motion,
            );
        }
    }
}

#[cfg(test)]
mod consumes_key_tests {
    use super::*;

    /// Over a container screen its keys are vanilla's; over a player mod's view, which hides the
    /// screen, only the keys that close it are.
    #[test]
    fn a_shown_view_frees_the_container_screens_keys() {
        for key in [KeyCode::PageUp, KeyCode::Digit1, KeyCode::KeyQ] {
            assert!(inventory_consumes_key(None, key, false), "{key:?}");
            assert!(!inventory_consumes_key(None, key, true), "{key:?}");
        }
        assert!(inventory_consumes_key(None, KeyCode::Escape, true));
        assert!(inventory_consumes_key(None, KeyCode::KeyE, true));
        assert!(!inventory_consumes_key(None, KeyCode::KeyR, false));
        assert!(!inventory_consumes_key(None, KeyCode::Backspace, false));
    }
}
