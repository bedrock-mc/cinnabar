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

/// Settings' Game section while the host plays their own local world, with cheats off and on.
pub(super) const WORLD_STATES: &[State] = &[
    State {
        name: "settings-world-game",
        size: [1280, 3200],
        gui_scale: GUI_SCALE,
        fixture: || settings_world_game(false),
    },
    State {
        name: "settings-world-game-cheats",
        size: [1280, 3200],
        gui_scale: GUI_SCALE,
        fixture: || settings_world_game(true),
    },
];

/// The Game section of an anonymous dedicated-server world.
fn settings_world_game(cheats: bool) -> Fixture {
    use launcher::local_worlds::{in_world, options::WorldOptions, world::World};
    let mut view = settings(SettingSection::Game.definition().name);
    view.over_world = true;
    let mut options = WorldOptions::default();
    options.restore_cheats(cheats);
    let world = World {
        id: "0123456789abcdef".to_owned(),
        name: "My World".to_owned(),
        game_mode: Default::default(),
        generator: Default::default(),
        difficulty: Default::default(),
        backend: bridge::Backend::Bds,
        seed: 1_234_567,
        options,
        options_unavailable: false,
        created_unix: 0,
        last_played_unix: 0,
        size_bytes: 0,
    };
    view.world_settings = in_world::View::of(&world).map(Arc::new);
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

/// Shows editable matches from outside the selected settings category.
fn settings_search(query: &str) -> Fixture {
    let mut view = settings("accessibility_forced_index");
    view.settings_query = query.to_owned();
    view.field = Some(launcher::menu::MenuField::SettingsQuery);
    view.focused_action = Some(MenuAction::SearchSettings);
    view.caret.byte = query.len();
    Fixture::Menu(Box::new(view))
}
