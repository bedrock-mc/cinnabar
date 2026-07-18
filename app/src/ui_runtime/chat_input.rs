use bevy::{
    ecs::system::SystemParam,
    input::{
        gamepad::{Gamepad, GamepadButton},
        touch::Touches,
    },
    prelude::{ButtonInput, MouseButton, Query, Res, ResMut, Single, Time, With},
    time::Real,
    window::{PrimaryWindow, Window},
};
use ui::{PointerPhase, UiAction, UiPoint};

use super::{
    UiRuntime, dispatch_chat_ui_action, gamepad_chat_action, presentation::UiPresentationRuntime,
};
use crate::semantic_controls::SemanticTouchTargets;

#[derive(SystemParam)]
pub(crate) struct ChatUiInputs<'w, 's> {
    time: Res<'w, Time<Real>>,
    window: Single<'w, 's, &'static Window, With<PrimaryWindow>>,
    mouse_buttons: Res<'w, ButtonInput<MouseButton>>,
    touches: Res<'w, Touches>,
    gamepads: Query<'w, 's, &'static Gamepad>,
    presentation: Res<'w, UiPresentationRuntime>,
}

pub(crate) fn drive_chat_ui_actions(
    inputs: ChatUiInputs,
    mut runtime: ResMut<UiRuntime>,
    mut touch_targets: ResMut<SemanticTouchTargets>,
) {
    let ChatUiInputs {
        time,
        window,
        mouse_buttons,
        touches,
        gamepads,
        presentation,
    } = inputs;
    if !runtime.chat_focused() || !window.focused {
        return;
    }
    let logical_size = [window.width(), window.height()];
    let now_millis = u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX);

    if mouse_buttons.just_pressed(MouseButton::Left)
        && let Some(position) = window.cursor_position()
        && let Ok(position) = UiPoint::new(position.x, position.y)
    {
        dispatch_chat_ui_action(
            &mut runtime,
            UiAction::PointerPrimary {
                position,
                phase: PointerPhase::Pressed,
            },
            presentation.hit_test_chat_suggestion(position, logical_size),
            now_millis,
        );
    }
    for touch in touches.iter_just_pressed() {
        let position = touch.position();
        if let Ok(position) = UiPoint::new(position.x, position.y) {
            let suggestion_hit = presentation.hit_test_chat_suggestion(position, logical_size);
            if suggestion_hit.is_some() {
                touch_targets.set(touch.id(), semantic_input::touch::UI_ACCEPT);
            } else {
                touch_targets.clear(touch.id());
            }
            dispatch_chat_ui_action(
                &mut runtime,
                UiAction::PointerPrimary {
                    position,
                    phase: PointerPhase::Pressed,
                },
                suggestion_hit,
                now_millis,
            );
        }
    }
    for gamepad in &gamepads {
        for button in [
            GamepadButton::DPadUp,
            GamepadButton::DPadDown,
            GamepadButton::South,
            GamepadButton::East,
            GamepadButton::RightTrigger,
            GamepadButton::LeftTrigger,
        ] {
            if gamepad.just_pressed(button) {
                dispatch_chat_ui_action(
                    &mut runtime,
                    gamepad_chat_action(button).expect("the mapped button list is exhaustive"),
                    None,
                    now_millis,
                );
            }
        }
    }
}
