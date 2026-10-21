//! Anonymous Chat section states rendered by the production settings painter.

use super::{
    Arc, Fixture, GUI_SCALE, MenuAction, SIZE, SettingId, SettingSection, State, settings,
};
use launcher::menu::{MenuDialog, settings_options::SettingsGroup};

/// Shows defaults or nondefault values, optionally opening one of the chat pickers.
pub(super) fn panel(changed: bool, picker: Option<SettingId>) -> Fixture {
    let mut view = settings(SettingSection::Chat.definition().name);
    if changed {
        let options = Arc::make_mut(&mut view.settings_options);
        for option in launcher::menu::settings_options::layout::SettingPanel::Chat.options() {
            let value = if option.default == option.max {
                option.min
            } else {
                option.max
            };
            options.set(option.id.index(), value);
        }
    }
    view.settings.dropdown = picker.map(|id| id.index() as u16);
    Fixture::Menu(Box::new(view))
}

/// Reveals the final toggles and reset action through the same focus scrolling as the client.
pub(super) fn bottom(changed: bool) -> Fixture {
    let Fixture::Menu(mut view) = panel(changed, None) else {
        unreachable!()
    };
    view.focused_action = Some(MenuAction::SettingsResetGroup(SettingsGroup::Chat));
    Fixture::Menu(view)
}

/// Shows the confirmation used by Reset Chat Settings before defaults are applied.
pub(super) fn reset() -> Fixture {
    let Fixture::Menu(mut view) = bottom(true) else {
        unreachable!()
    };
    view.dialog = Some(MenuDialog::SettingsResetGroup(SettingsGroup::Chat));
    Fixture::Menu(view)
}

/// Shows the lower endpoints of the chat presentation sliders.
pub(super) fn minimum() -> Fixture {
    let Fixture::Menu(mut view) = panel(false, None) else {
        unreachable!()
    };
    let options = Arc::make_mut(&mut view.settings_options);
    for id in [
        SettingId::ChatBackgroundOpacity,
        SettingId::ChatFontSize,
        SettingId::ChatLineSpacing,
    ] {
        options.set(id.index(), id.definition().min);
    }
    Fixture::Menu(view)
}

pub(super) const STATES: &[State] = &[
    State {
        name: "settings-chat",
        size: [1280, 1360],
        gui_scale: GUI_SCALE,
        fixture: || panel(false, None),
    },
    State {
        name: "settings-chat-top",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || panel(false, None),
    },
    State {
        name: "settings-chat-bottom",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || bottom(false),
    },
    State {
        name: "settings-chat-changed",
        size: [1280, 1360],
        gui_scale: GUI_SCALE,
        fixture: || panel(true, None),
    },
    State {
        name: "settings-chat-minimum",
        size: [1280, 1360],
        gui_scale: GUI_SCALE,
        fixture: minimum,
    },
    State {
        name: "settings-chat-narrow",
        size: [800, 720],
        gui_scale: GUI_SCALE,
        fixture: || panel(false, None),
    },
    State {
        name: "settings-chat-reset",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: reset,
    },
    State {
        name: "settings-chat-color",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || panel(false, Some(SettingId::ChatColor)),
    },
    State {
        name: "settings-chat-mentions",
        size: SIZE,
        gui_scale: GUI_SCALE,
        fixture: || panel(false, Some(SettingId::MentionsColor)),
    },
];
