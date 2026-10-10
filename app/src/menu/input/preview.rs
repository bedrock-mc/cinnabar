use bevy::input::touch::Touches;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use ui::UiPoint;

use launcher::menu::MenuScreen;

#[allow(clippy::too_many_arguments)]
pub(super) fn drive(
    presentation: &mut UiPresentationRuntime,
    screen: MenuScreen,
    pointer: Option<UiPoint>,
    held: bool,
    press: bool,
    touches: &Touches,
    touch_capture: &mut Option<u64>,
) -> bool {
    let touch_point = |position: bevy::prelude::Vec2| UiPoint::new(position.x, position.y).ok();
    if let Some(id) = *touch_capture {
        if let Some(touch) = touches.get_pressed(id) {
            return presentation.menu_player_preview_pointer(
                Some(screen),
                touch_point(touch.position()),
                true,
                false,
            );
        }
        *touch_capture = None;
        let point = touches
            .get_released(id)
            .and_then(|touch| touch_point(touch.position()));
        return presentation.menu_player_preview_pointer(Some(screen), point, false, false);
    }
    for touch in touches.iter_just_pressed() {
        if presentation.menu_player_preview_pointer(
            Some(screen),
            touch_point(touch.position()),
            true,
            true,
        ) {
            *touch_capture = Some(touch.id());
            return true;
        }
    }
    presentation.menu_player_preview_pointer(Some(screen), pointer, held, press)
}

#[cfg(test)]
mod tests;
