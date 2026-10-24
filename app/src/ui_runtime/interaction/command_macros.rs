//! Macro presses consume one control even when the chat queue cannot accept a send.
use bevy::prelude::MouseButton;
use client_ui::ui_runtime::UiRuntime;
use semantic_input::PhysicalControl;

use crate::semantic_controls::SemanticInputRuntime;
use client_shell::menu::MenuRuntime;

/// Sends the matching gameplay macro and retires only its sampled control.
pub(super) fn dispatch(
    menu: Option<&MenuRuntime>,
    runtime: &mut UiRuntime,
    modifiers: semantic_input::ModifierChord,
    control: Option<PhysicalControl>,
    now_millis: u64,
    semantic: Option<&mut SemanticInputRuntime>,
) -> bool {
    let Some(control) = control else {
        return false;
    };
    let Some(command) = command(menu, runtime, modifiers, Some(control)) else {
        return false;
    };
    let _ = runtime.queue_chat_message(&command, now_millis);
    if let Some(semantic) = semantic {
        semantic.consume_control(control);
    }
    true
}

/// Resolves an armed gameplay shortcut before ordinary bindings claim its press.
pub(in crate::ui_runtime) fn command(
    menu: Option<&MenuRuntime>,
    runtime: &UiRuntime,
    modifiers: semantic_input::ModifierChord,
    control: Option<PhysicalControl>,
) -> Option<String> {
    if menu.is_some_and(MenuRuntime::is_visible)
        || runtime.chat_focused()
        || runtime.inventory_open()
        || runtime.local_sleeping()
        || runtime.sign_editor().is_open()
        || runtime.emotes().is_open()
        || runtime.server_forms().owns_input()
        || runtime.data_driven_ui().owns_input()
        || runtime.credits().owns_input()
    {
        return None;
    }
    client_shell::menu::settings_options::control_bindings::binding_macro_command(
        menu, control?, modifiers,
    )
}

/// Matches pointer presses using the same binding codes as capture and gameplay.
pub(super) fn dispatch_mouse(
    menu: Option<&MenuRuntime>,
    runtime: &mut UiRuntime,
    modifiers: semantic_input::ModifierChord,
    button: MouseButton,
    now_millis: u64,
    semantic: Option<&mut SemanticInputRuntime>,
) -> bool {
    dispatch(
        menu,
        runtime,
        modifiers,
        client_shell::physical_input::mouse_button_code(button).map(PhysicalControl::MouseButton),
        now_millis,
        semantic,
    )
}
