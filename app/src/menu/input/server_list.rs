use bevy::input::touch::Touches;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use ui::UiPoint;

use super::super::MenuRuntime;

pub(super) fn drive(
    presentation: &mut UiPresentationRuntime,
    menu: &mut MenuRuntime,
    pointer: Option<UiPoint>,
    held: bool,
    pressed: bool,
    touches: &Touches,
    captured_touch: &mut Option<u64>,
) -> bool {
    let screen = (menu.is_visible() && menu.dialog.is_none() && !menu.is_connecting())
        .then_some(menu.screen());
    let input =
        |presentation: &mut UiPresentationRuntime, menu: &mut MenuRuntime, point, held, pressed| {
            let (captured, action) =
                presentation.menu_server_list_pointer(screen, point, held, pressed);
            if let Some(action) = action {
                menu.activate_from_input(action);
            }
            captured
        };
    if let Some(id) = *captured_touch {
        let touch = touches.get_pressed(id).or_else(|| touches.get_released(id));
        let point =
            touch.and_then(|touch| UiPoint::new(touch.position().x, touch.position().y).ok());
        let held = touches.get_pressed(id).is_some();
        let captured = input(presentation, menu, point, held, false);
        if !held || !captured {
            *captured_touch = None;
        }
        return captured;
    }
    for touch in touches.iter_just_pressed() {
        let point = UiPoint::new(touch.position().x, touch.position().y).ok();
        if input(presentation, menu, point, true, true) {
            *captured_touch = Some(touch.id());
            return true;
        }
    }
    input(presentation, menu, pointer, held, pressed)
}
