//! Settings content and scrollbars follow one captured finger; slider thumbs retain priority.

use bevy::input::touch::Touches;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use ui::UiPoint;

/// Returns the finger whose scroll gesture must not activate a setting on press or release.
pub(super) fn drive(
    presentation: &mut UiPresentationRuntime,
    touches: &Touches,
    enabled: bool,
    captured: &mut Option<u64>,
) -> Option<u64> {
    if !enabled {
        presentation.move_menu_touch_scroll(None, false);
        *captured = None;
        return None;
    }
    if captured.is_none() {
        for touch in touches.iter_just_pressed() {
            let position = touch.position();
            if let Ok(point) = UiPoint::new(position.x, position.y)
                && presentation.settings_slider_thumb_hit_test(point).is_none()
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
