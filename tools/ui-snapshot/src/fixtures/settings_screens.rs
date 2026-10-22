//! Keyboard and General settings screens at fixed, anonymous states.

use super::*;

/// Shows the keyboard options and command macro modifier without a live session.
pub(super) fn settings_keyboard() -> Fixture {
    Fixture::Menu(Box::new(settings("keyboard_and_mouse_forced_index")))
}

/// Scrolls the General section to toggle `id` through the client's focus scrolling; a toggle's
/// action carries the value it switches to.
pub(super) fn settings_general_toggle(id: SettingId) -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    view.focused_action = Some(MenuAction::SettingsOption(
        id.index() as u16,
        1 - view.settings_options.value(id),
    ));
    Fixture::Menu(Box::new(view))
}

/// The Advanced rows with Verbose logging and Record network session turned on.
pub(super) fn settings_general_diagnostics_on() -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    let mut options = (*view.settings_options).clone();
    for id in [SettingId::VerboseLogging, SettingId::RecordNetworkSession] {
        options.set(id.index(), 1);
    }
    view.settings_options = Arc::new(options);
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::RecordNetworkSession.index() as u16,
        0,
    ));
    Fixture::Menu(Box::new(view))
}

/// Shows the same physical hotbar binding with a fallback or an observed AZERTY label.
pub(super) fn settings_keyboard_layout(observed: bool) -> Fixture {
    use launcher::menu::settings_options::{
        BindingId, KEY_BINDINGS, control_bindings::decode_control,
    };

    let mut view = settings("keyboard_and_mouse_forced_index");
    let row = KEY_BINDINGS
        .iter()
        .position(|(_, name)| *name == BindingId::Hotbar1.name())
        .unwrap();
    let settings = Arc::make_mut(&mut view.settings_options);
    assert!(settings.remap(row, decode_control(0x14).unwrap()));
    if observed {
        settings.set_key_label(0x14, "a");
    }
    Fixture::Menu(Box::new(view))
}

/// Focuses the occlusion toggle at its saved value in the Video settings viewport.
pub(super) fn settings_video_culling(enabled: bool) -> Fixture {
    let mut view = settings("video_forced_index");
    Arc::make_mut(&mut view.settings_options)
        .set(SettingId::ChunkOcclusionCulling.index(), i32::from(enabled));
    view.focused_action = Some(MenuAction::SettingsOption(
        SettingId::ChunkOcclusionCulling.index() as u16,
        i32::from(!enabled),
    ));
    Fixture::Menu(Box::new(view))
}
