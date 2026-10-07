use super::{MenuRuntime, UiPoint, UiPresentationRuntime};

pub(super) fn update_slider(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    pointer: Option<UiPoint>,
    pointer_pressed: bool,
    pointer_just_pressed: bool,
    native_settings: bool,
) {
    if !pointer_pressed || menu.screen() != crate::menu::MenuScreen::Settings {
        menu.settings_slider_drag = None;
        menu.settings_slider_pointer = None;
    }
    if pointer_just_pressed {
        menu.settings_slider_drag = match menu.hovered {
            Some(crate::menu::MenuAction::SettingsOption(index, _))
                if matches!(
                    crate::menu::settings_options::SETTINGS_OPTIONS[usize::from(index)].kind,
                    crate::menu::settings_options::SettingKind::Slider
                ) && (!native_settings
                    || pointer.is_some_and(|point| {
                        presentation.settings_slider_thumb_contains(index, point)
                    })) =>
            {
                Some(index)
            }
            _ => None,
        };
    }
    if pointer_pressed
        && let Some(index) = menu.settings_slider_drag
        && let Some(action @ crate::menu::MenuAction::SettingsOption(_, value)) =
            pointer.and_then(|point| presentation.settings_slider_drag_action(index, point))
    {
        menu.hovered = Some(action);
        menu.pressed = Some(action);
        if !pointer_just_pressed {
            menu.set_option(index, value);
        }
        menu.settings_slider_pointer = pointer.and_then(|point| {
            presentation
                .settings_slider_drag_fraction(index, point)
                .map(|fraction| launcher::menu::view::SettingsSliderPointer {
                    option: index,
                    fraction,
                    mouse_input: menu.input_mode.mouse(),
                })
        });
    }
}

pub(super) fn native_release_action(
    pressed: crate::menu::MenuAction,
    hovered: Option<crate::menu::MenuAction>,
) -> Option<crate::menu::MenuAction> {
    let hovered = hovered?;
    if hovered == pressed {
        return Some(hovered);
    }
    match (pressed, hovered) {
        (
            crate::menu::MenuAction::SettingsOption(index, _),
            crate::menu::MenuAction::SettingsOption(at, _),
        ) if index == at
            && crate::menu::settings_options::SETTINGS_OPTIONS
                .get(usize::from(index))
                .is_some_and(|option| {
                    matches!(
                        option.kind,
                        crate::menu::settings_options::SettingKind::Slider
                            | crate::menu::settings_options::SettingKind::Toggle
                    )
                }) =>
        {
            Some(hovered)
        }
        (
            crate::menu::MenuAction::SettingsFullscreen(_),
            crate::menu::MenuAction::SettingsFullscreen(_),
        ) => Some(hovered),
        _ => None,
    }
}
