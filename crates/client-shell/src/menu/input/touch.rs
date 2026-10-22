//! Single-finger menu gestures defer control activation until a completed tap.

use super::*;

/// Applies one captured finger while preserving slider ownership and cancelling swiped taps.
pub(super) fn drive(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    touches: &Touches,
    capture: &mut GuiScaleDrag,
    native_settings: bool,
    server_list_owns_pointer: bool,
    scrolled: Option<u64>,
) -> Option<(UiPoint, MenuField)> {
    let mut caret_press = None;
    let press = |menu: &mut MenuRuntime, action| {
        let action = if native_settings {
            menu.live_settings_action(action)
        } else {
            action
        };
        if let Some(sound) = presentation.menu_sound(action) {
            client_ui::sound_requests::ui_control_sound(sound);
        }
        menu.activate_from_input(action);
    };
    if touches.any_just_pressed() {
        menu.touch_available = true;
        menu.input_mode = MenuInputMode::Touch;
    }
    for touch in touches
        .iter_just_pressed()
        .filter(|touch| capture.menu_touch == Some(touch.id()))
    {
        if server_list_owns_pointer
            || capture.preview_touch == Some(touch.id())
            || scrolled == Some(touch.id())
        {
            continue;
        }
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y)
            && let Some(action) = native_settings
                .then(|| presentation.settings_slider_thumb_hit_test(position))
                .flatten()
                .map(|index| {
                    launcher::menu::MenuAction::SettingsOption(
                        index,
                        menu.settings_options.get(usize::from(index)),
                    )
                })
                .or_else(|| presentation.hit_test_menu(position))
        {
            menu.input_mode = MenuInputMode::Touch;
            {
                if !native_settings
                    || !matches!(action, launcher::menu::MenuAction::SettingsOption(index, _)
                    if launcher::menu::settings_options::SETTINGS_OPTIONS.get(usize::from(index))
                        .is_some_and(|option| matches!(option.kind, launcher::menu::settings_options::SettingKind::Slider)))
                    || matches!(action, launcher::menu::MenuAction::SettingsOption(index, _)
                        if presentation.settings_slider_thumb_contains(index, position))
                {
                    menu.focus_pointer(action);
                }
                menu.pressed = Some(action);
                if let launcher::menu::MenuAction::SettingsOption(index, _) = action
                    && launcher::menu::settings_options::SETTINGS_OPTIONS
                        .get(usize::from(index))
                        .is_some_and(|option| {
                            matches!(
                                option.kind,
                                launcher::menu::settings_options::SettingKind::Slider
                            )
                        })
                    && (!native_settings
                        || presentation.settings_slider_thumb_contains(index, position))
                {
                    capture.touch_slider = Some((touch.id(), index));
                    capture.touch_press = None;
                } else {
                    capture.touch_press = Some((touch.id(), action, position));
                }
            }
        }
    }
    if let Some((id, index)) = capture.touch_slider {
        if let Some(touch) = touches.get_pressed(id).or_else(|| touches.get_released(id)) {
            let position = touch.position();
            if let Ok(point) = UiPoint::new(position.x, position.y)
                && let Some(action @ launcher::menu::MenuAction::SettingsOption(_, value)) =
                    presentation.settings_slider_drag_action(index, point)
            {
                menu.input_mode = MenuInputMode::Touch;
                menu.set_option(index, value);
                menu.settings.slider_pointer = presentation
                    .settings_slider_drag_fraction(index, point)
                    .map(|fraction| launcher::menu::view::SettingsSliderPointer {
                        option: index,
                        fraction,
                        mouse_input: false,
                    });
                menu.pressed = Some(action);
            }
        }
        if touches.get_pressed(id).is_none() {
            capture.touch_slider = None;
            menu.settings.slider_pointer = None;
        }
    }
    if let Some((id, action, start)) = capture.touch_press
        && let Some(touch) = touches.get_pressed(id).or_else(|| touches.get_released(id))
        && let Ok(point) = UiPoint::new(touch.position().x, touch.position().y)
        && !is_captured_slider(presentation, action, point)
        && (point.x() - start.x()).hypot(point.y() - start.y()) > ui::TOUCH_DRAG_DISTANCE
    {
        capture.touch_press = None;
        menu.pressed = None;
    }
    if let Some((id, mut action, start)) = capture.touch_press {
        if let Some(touch) = touches.get_released(id) {
            let position = touch.position();
            let hovered = UiPoint::new(position.x, position.y)
                .ok()
                .and_then(|point| presentation.hit_test_menu(point));
            if !UiPoint::new(position.x, position.y)
                .ok()
                .is_some_and(|point| slider_drag(menu, presentation, &mut action, point, true))
                && let Some(action) = native_release_action(action, hovered)
            {
                press(menu, action);
                caret_press = UiPoint::new(position.x, position.y)
                    .ok()
                    .zip(action.text_field());
            }
            capture.touch_press = None;
        } else if let Some(touch) = touches.get_pressed(id) {
            let position = touch.position();
            let hovered = UiPoint::new(position.x, position.y)
                .ok()
                .and_then(|point| presentation.hit_test_menu(point));
            let shader_dragged = UiPoint::new(position.x, position.y)
                .ok()
                .is_some_and(|point| slider_drag(menu, presentation, &mut action, point, false));
            if shader_dragged {
                capture.touch_press = Some((id, action, start));
            } else if native_release_action(action, hovered).is_some() {
                menu.pressed = Some(action);
            }
        } else {
            capture.touch_press = None;
        }
    }
    if capture
        .menu_touch
        .is_some_and(|id| touches.get_pressed(id).is_none())
    {
        capture.menu_touch = None;
    }
    caret_press
}

/// Updates the slider that captured the finger even outside its original track.
fn slider_drag(
    menu: &mut MenuRuntime,
    presentation: &UiPresentationRuntime,
    captured: &mut launcher::menu::MenuAction,
    point: UiPoint,
    released: bool,
) -> bool {
    if matches!(captured, launcher::menu::MenuAction::SettingsScale(_)) {
        if let Some(action) = presentation.gui_scale_drag_action(point) {
            if released || *captured != action {
                menu.activate_from_input(action);
            }
            *captured = action;
            menu.pressed = Some(action);
            true
        } else {
            false
        }
    } else {
        settings_pointer::shader_touch_drag(menu, presentation, captured, point, released)
    }
}

/// Distinguishes captured drag controls from ordinary taps before applying the movement slop.
fn is_captured_slider(
    presentation: &UiPresentationRuntime,
    action: launcher::menu::MenuAction,
    point: UiPoint,
) -> bool {
    use launcher::{
        menu::MenuAction,
        shader_options::{Action, Input},
    };
    match action {
        MenuAction::SettingsScale(_) => presentation.gui_scale_drag_action(point).is_some(),
        MenuAction::ShaderOptions(Action::Element {
            screen,
            index,
            input: Input::Select(_),
        }) => presentation
            .shader_option_slider_drag_action(screen, index, point)
            .is_some(),
        _ => false,
    }
}
