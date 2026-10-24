//! Menu content and scrollbars follow one captured finger; slider thumbs retain priority.

use bevy::input::touch::Touches;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use ui::UiPoint;

/// Returns the finger whose scroll gesture must not activate a setting on press or release.
pub(super) fn drive(
    presentation: &mut UiPresentationRuntime,
    touches: &Touches,
    enabled: bool,
    owner: Option<u64>,
    captured: &mut Option<u64>,
) -> Option<u64> {
    if !enabled {
        presentation.move_menu_touch_scroll(None, false);
        *captured = None;
        return None;
    }
    if captured.is_none() {
        for touch in touches
            .iter_just_pressed()
            .filter(|touch| owner == Some(touch.id()))
        {
            let position = touch.position();
            if let Ok(point) = UiPoint::new(position.x, position.y)
                && presentation.settings_slider_thumb_hit_test(point).is_none()
                && !shader_slider_at(presentation, point)
                && !engine_slider_at(presentation, point)
                && presentation.begin_menu_touch_scroll(point)
            {
                *captured = Some(touch.id());
                break;
            }
        }
    }
    let id = (*captured)?;
    let touch = touches.get_pressed(id).or_else(|| touches.get_released(id));
    let point = touch.and_then(|touch| {
        let position = touch.position();
        UiPoint::new(position.x, position.y).ok()
    });
    let held = touches.get_pressed(id).is_some();
    let consumed = presentation.move_menu_touch_scroll(point, held);
    if !held {
        *captured = None;
    }
    consumed.then_some(id)
}

/// Shader sliders own their whole track, independently of the built-in thumb hit regions.
fn shader_slider_at(presentation: &UiPresentationRuntime, point: UiPoint) -> bool {
    use launcher::{
        menu::MenuAction,
        shader_options::{Action, Input},
    };
    matches!(presentation.hit_test_menu(point), Some(MenuAction::ShaderOptions(Action::Element {
        screen, index, input: Input::Select(_),
    })) if presentation.shader_option_slider_drag_action(screen, index, point).is_some())
}

/// JSON-UI slider tracks retain the finger instead of starting a parent content pan.
fn engine_slider_at(presentation: &UiPresentationRuntime, point: UiPoint) -> bool {
    if presentation.uses_oreui_settings() {
        return false;
    }
    match presentation.hit_test_menu(point) {
        Some(launcher::menu::MenuAction::SettingsOption(index, _)) => {
            launcher::menu::settings_options::SETTINGS_OPTIONS
                .get(usize::from(index))
                .is_some_and(|option| {
                    matches!(
                        option.kind,
                        launcher::menu::settings_options::SettingKind::Slider
                    )
                })
        }
        Some(launcher::menu::MenuAction::SettingsScale(_)) => true,
        _ => false,
    }
}
