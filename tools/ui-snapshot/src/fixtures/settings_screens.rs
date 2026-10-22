//! Settings screens at fixed, anonymous states.

use super::*;

/// Search results, an empty result and the compact settings viewport.
pub(super) const SEARCH_STATES: &[State] = &[
    State {
        name: "settings-search",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_search("vsync"),
    },
    State {
        name: "settings-search-empty",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || settings_search("no-such-setting"),
    },
    State {
        name: "settings-search-small",
        size: [640, 360],
        gui_scale: GUI_SCALE,
        fixture: || settings_search("vsync"),
    },
];

/// Whole settings pages, tall enough to show every row at once.
pub(super) const PAGE_STATES: &[State] = &[
    State {
        name: "settings-accessibility-page",
        size: [1280, 1700],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Accessibility, false),
    },
    State {
        name: "settings-controller-page",
        size: [1280, 1500],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::ControllerAndSwitch, false),
    },
    State {
        name: "settings-general-page",
        size: [1280, 1500],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::General, false),
    },
    State {
        name: "settings-creator-page",
        size: [1280, 1300],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Creator, false),
    },
    State {
        name: "settings-account-page",
        size: [1280, 900],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Account, true),
    },
    State {
        name: "settings-video-page",
        size: [1280, 2600],
        gui_scale: GUI_SCALE,
        fixture: || settings_page(SettingSection::Video, false),
    },
];

/// One settings page, optionally signed in, with the sidebar beside it.
fn settings_page(section: SettingSection, signed_in: bool) -> Fixture {
    let mut view = settings(section.definition().name);
    if signed_in {
        view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    }
    Fixture::Menu(Box::new(view))
}

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

/// The Advanced rows with every diagnostics toggle turned on.
pub(super) fn settings_general_diagnostics_on() -> Fixture {
    let mut view = settings(SettingSection::General.definition().name);
    let mut options = (*view.settings_options).clone();
    for id in [
        SettingId::VerboseLogging,
        SettingId::RecordNetworkSession,
        SettingId::SendCrashReports,
    ] {
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

/// Shows editable matches from outside the selected settings category.
fn settings_search(query: &str) -> Fixture {
    let mut view = settings("accessibility_forced_index");
    view.settings_query = query.to_owned();
    view.field = Some(launcher::menu::MenuField::SettingsQuery);
    view.focused_action = Some(MenuAction::SearchSettings);
    view.caret.byte = query.len();
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
