use super::{MenuRuntime, UiPoint, UiPresentationRuntime};
use launcher::{
    menu::MenuAction,
    shader_options::{Action, Input},
};

/// Updates native slider capture and applies movement until the pointer releases.
pub(super) fn update_slider(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    pointer: Option<UiPoint>,
    pointer_pressed: bool,
    pointer_just_pressed: bool,
    native_settings: bool,
) {
    update_shader_slider(
        menu,
        presentation,
        pointer,
        pointer_pressed,
        pointer_just_pressed,
    );
    if !pointer_pressed || menu.screen() != launcher::menu::MenuScreen::Settings {
        menu.settings_input.slider_drag = None;
        menu.settings.slider_pointer = None;
    }
    if pointer_just_pressed {
        menu.settings_input.slider_drag = match menu.hovered {
            Some(launcher::menu::MenuAction::SettingsOption(index, _))
                if matches!(
                    launcher::menu::settings_options::SETTINGS_OPTIONS[usize::from(index)].kind,
                    launcher::menu::settings_options::SettingKind::Slider
                ) && (!native_settings
                    || pointer.is_some_and(|point| {
                        presentation.settings_slider_thumb_contains(index, point)
                    })) =>
            {
                Some(index)
            }
            _ => None,
        };
        if let Some(index) = menu.settings_input.slider_drag {
            menu.capture_settings_slider(index);
        }
    }
    if pointer_pressed
        && let Some(index) = menu.settings_input.slider_drag
        && let Some(action @ launcher::menu::MenuAction::SettingsOption(_, value)) =
            pointer.and_then(|point| presentation.settings_slider_drag_action(index, point))
    {
        menu.hovered = Some(action);
        menu.pressed = Some(action);
        if !pointer_just_pressed {
            menu.set_option(index, value);
        }
        menu.settings.slider_pointer = pointer.and_then(|point| {
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

/// Captures shader slider steps and keeps their pack-defined choice range during a drag.
fn update_shader_slider(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    pointer: Option<UiPoint>,
    held: bool,
    just_pressed: bool,
) {
    if menu.screen() != launcher::menu::MenuScreen::Settings || menu.dialog.is_some() {
        menu.settings_input.shader_slider_drag = None;
        return;
    }
    if just_pressed {
        menu.settings_input.shader_slider_drag = match menu.hovered {
            Some(MenuAction::ShaderOptions(Action::Element {
                screen,
                index,
                input: Input::Select(step),
            })) => Some((screen, index, step)),
            _ => None,
        };
    }
    if let Some((screen, index, previous)) = menu.settings_input.shader_slider_drag {
        let action = menu
            .shader_options_view()
            .filter(|view| view.screen == screen)
            .and(pointer)
            .and_then(|point| presentation.shader_option_slider_drag_action(screen, index, point));
        if let Some(action) = action {
            let MenuAction::ShaderOptions(Action::Element {
                input: Input::Select(step),
                ..
            }) = action
            else {
                return;
            };
            if just_pressed || step != previous {
                menu.activate_from_input(action);
            }
            menu.settings_input.shader_slider_drag = Some((screen, index, step));
            menu.hovered = Some(action);
            menu.pressed = Some(action);
        } else {
            menu.settings_input.shader_slider_drag = None;
        }
    }
    if !held {
        menu.settings_input.shader_slider_drag = None;
    }
}

/// Keeps a shader slider's captured touch on its own track, including beyond the track edges.
pub(super) fn shader_touch_drag(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    captured: &mut MenuAction,
    point: UiPoint,
    released: bool,
) -> bool {
    let MenuAction::ShaderOptions(Action::Element {
        screen,
        index,
        input: Input::Select(_),
    }) = *captured
    else {
        return false;
    };
    if menu
        .shader_options_view()
        .is_some_and(|view| view.screen == screen)
        && let Some(action) = presentation.shader_option_slider_drag_action(screen, index, point)
    {
        if released || *captured != action {
            menu.activate_from_input(action);
        }
        *captured = action;
        menu.pressed = Some(action);
    }
    true
}

/// Resolves a release within the same control while rejecting unrelated controls.
pub(super) fn native_release_action(
    pressed: launcher::menu::MenuAction,
    hovered: Option<launcher::menu::MenuAction>,
) -> Option<launcher::menu::MenuAction> {
    let hovered = hovered?;
    if hovered == pressed {
        return Some(hovered);
    }
    match (pressed, hovered) {
        (
            MenuAction::ShaderOptions(Action::Element {
                screen,
                index,
                input: Input::Select(_),
            }),
            MenuAction::ShaderOptions(Action::Element {
                screen: at,
                index: element,
                input: Input::Select(_),
            }),
        ) if screen == at && index == element => Some(hovered),
        (
            launcher::menu::MenuAction::SettingsOption(index, _),
            launcher::menu::MenuAction::SettingsOption(at, _),
        ) if index == at
            && launcher::menu::settings_options::SETTINGS_OPTIONS
                .get(usize::from(index))
                .is_some_and(|option| {
                    matches!(
                        option.kind,
                        launcher::menu::settings_options::SettingKind::Slider
                            | launcher::menu::settings_options::SettingKind::Toggle
                    )
                }) =>
        {
            Some(hovered)
        }
        (
            launcher::menu::MenuAction::SettingsFullscreen(_),
            launcher::menu::MenuAction::SettingsFullscreen(_),
        ) => Some(hovered),
        _ => None,
    }
}
