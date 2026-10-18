//! Publishes canvas samples to the shared gameplay input owners.
use crate::{runtime::world, semantic_controls};
use bevy::prelude::*;

/// In-memory settings used by gameplay until a menu host replaces them.
#[derive(Resource, Default)]
pub(crate) struct BrowserOptions(
    pub(crate) std::sync::Arc<launcher::menu::settings_options::SettingsOptions>,
);

/// Canvas observations owned by the DOM edge, sampled once per game update.
pub(crate) struct InputSource(pub Box<dyn FnMut() -> semantic_input::DeviceFrame>);

/// Whether the canvas owns the device sample used by this gameplay frame.
#[derive(Resource, Default)]
pub(crate) struct CanvasCapture(bool);

/// Publishes UI ownership and canvas capture for every gameplay input consumer.
pub(crate) fn screen_focus(
    capture: Res<CanvasCapture>,
    player: Res<client_state::PlayerRuntime>,
    ui: Res<client_shell::ui_resources::UiRuntimeResource>,
    presentation: Res<client_shell::ui_resources::UiPresentationResource>,
    world: Res<world::ClientWorld>,
    mut focus: ResMut<client_state::ScreenFocus>,
) {
    let focused =
        ui.ui_focused(&player) || presentation.loading_stage().is_some() || world.stream.is_none();
    focus.set_if_neq(client_state::ScreenFocus {
        ui_focused: focused,
        absorbs_input: focused || !capture.0,
        steals_mouse: Some(focused),
        ..Default::default()
    });
}

/// Samples the browser canvas without also collecting the native physical input source.
pub(crate) fn sample_input(
    mut source: NonSendMut<InputSource>,
    mut capture: ResMut<CanvasCapture>,
    mut pending: ResMut<semantic_controls::PendingDeviceFrame>,
    world: Res<world::ClientWorld>,
    options: Res<BrowserOptions>,
    mut controls: ResMut<client_state::GameplayScreenControls>,
) {
    let mut frame = (source.0)();
    frame.window_focus_lost |= world.stream.is_none();
    capture.0 = !frame.window_focus_lost;
    let pressed = |control| match control {
        semantic_input::PhysicalControl::KeyboardUsage(key) => frame
            .keyboard_mouse
            .as_ref()
            .is_some_and(|keyboard| keyboard.key_edges.pressed.contains(&key)),
        semantic_input::PhysicalControl::MouseButton(button) => frame
            .keyboard_mouse
            .as_ref()
            .is_some_and(|keyboard| keyboard.mouse_edges.pressed.contains(&button)),
        _ => false,
    };
    let binding = launcher::menu::settings_options::BindingId::PickItem;
    controls.pick_block_pressed = !frame.window_focus_lost
        && [
            options.0.named_key_control(binding),
            options.0.secondary_key_control(binding),
        ]
        .into_iter()
        .flatten()
        .any(pressed);
    pending.replace(frame);
}

#[cfg(test)]
mod tests;
