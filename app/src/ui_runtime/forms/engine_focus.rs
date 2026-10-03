//! Keyboard and gamepad focus movement for an engine-drawn form, through the
//! engine's vanilla focus navigation.

use bevy::input::{
    gamepad::{Gamepad, GamepadButton},
    keyboard::KeyCode,
};
use json_ui::{FocusDirection, FocusMove, HitKind, HitRegion, RectOut};

use super::values::EngineFrame;
use crate::ui_runtime::UiRuntime;

/// Stick deflection that counts as a directional press (needs native measurement).
const STICK_PRESS: f32 = 0.5;

/// The focus direction an arrow key moves.
pub(super) fn direction_of(key: KeyCode) -> Option<FocusDirection> {
    Some(match key {
        KeyCode::ArrowUp => FocusDirection::Up,
        KeyCode::ArrowDown => FocusDirection::Down,
        KeyCode::ArrowLeft => FocusDirection::Left,
        KeyCode::ArrowRight => FocusDirection::Right,
        _ => return None,
    })
}

/// Move focus toward a `direction` the dispatcher left unconsumed; a control
/// claiming the direction already received it as `button.menu_*`.
pub(super) fn step(runtime: &mut UiRuntime, frame: &EngineFrame, direction: FocusDirection) {
    let view = &runtime.server_forms().engine().view;
    if let FocusMove::Moved(key) =
        json_ui::navigate(&frame.hits, view, direction, screen(&frame.hits))
    {
        focus(runtime, frame, key);
    }
}

/// Tab: the next focusable control in document order.
pub(super) fn tab(runtime: &mut UiRuntime, frame: &EngineFrame) {
    let focused = runtime.server_forms().engine().view.focused.clone();
    if let Some(next) = json_ui::next_in_order(&frame.hits, focused.as_deref(), false) {
        focus(runtime, frame, next.key.clone());
    }
}

/// Focus `key`, showing its hover look and keeping it inside its scroll view.
fn focus(runtime: &mut UiRuntime, frame: &EngineFrame, key: String) {
    let engine = runtime.server_forms_mut().engine_mut();
    json_ui::set_focus(&mut engine.view, &frame.hits, Some(key.clone()));
    engine.view.hovered = Some(key.clone());
    let Some(region) = frame.hits.iter().find(|region| region.key == key) else {
        return;
    };
    if let Some(view) = owning_view(&frame.hits, region)
        && let Some(metrics) = frame.report.scrolls.get(&view.key)
    {
        let (start, length) = if metrics.horizontal {
            (region.rect.x, region.rect.w)
        } else {
            (region.rect.y, region.rect.h)
        };
        let offset = metrics.offset_revealing(start, start + length);
        engine.view.scroll.insert(view.key.clone(), offset);
    }
}

/// The innermost scroll view whose key prefixes `region`'s key.
fn owning_view<'a>(hits: &'a [HitRegion], region: &HitRegion) -> Option<&'a HitRegion> {
    hits.iter()
        .filter(|view| view.kind == HitKind::ScrollView && region.key.starts_with(&view.key))
        .max_by_key(|view| view.key.len())
}

/// The virtual screen: every region's clip descends from the root's.
fn screen(hits: &[HitRegion]) -> RectOut {
    let (x1, y1) = hits.iter().fold((0.0f64, 0.0f64), |(x, y), region| {
        (
            x.max(region.clip.x + region.clip.w),
            y.max(region.clip.y + region.clip.h),
        )
    });
    RectOut {
        x: 0.0,
        y: 0.0,
        w: x1,
        h: y1,
    }
}

/// Gamepad presses as the keys the engine path reads: the d-pad and left
/// stick as arrows, South as Enter (`button.menu_select`) and East as Escape
/// (`button.menu_cancel`). `stick` keeps last frame's deflected directions.
pub(crate) fn gamepad_keys<'a>(
    pads: impl Iterator<Item = &'a Gamepad>,
    stick: &mut [bool; 4],
) -> Vec<(KeyCode, Option<String>)> {
    const BUTTONS: [(GamepadButton, KeyCode); 6] = [
        (GamepadButton::DPadUp, KeyCode::ArrowUp),
        (GamepadButton::DPadDown, KeyCode::ArrowDown),
        (GamepadButton::DPadLeft, KeyCode::ArrowLeft),
        (GamepadButton::DPadRight, KeyCode::ArrowRight),
        (GamepadButton::South, KeyCode::Enter),
        (GamepadButton::East, KeyCode::Escape),
    ];
    let mut keys = Vec::new();
    let mut deflected = [false; 4];
    for pad in pads {
        keys.extend(
            BUTTONS
                .iter()
                .filter(|(button, _)| pad.just_pressed(*button))
                .map(|(_, key)| (*key, None)),
        );
        let axes = pad.left_stick();
        deflected[0] |= axes.y > STICK_PRESS;
        deflected[1] |= axes.y < -STICK_PRESS;
        deflected[2] |= axes.x < -STICK_PRESS;
        deflected[3] |= axes.x > STICK_PRESS;
    }
    let arrows = [
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
    ];
    for ((now, was), key) in deflected.iter().zip(stick.iter()).zip(arrows) {
        if *now && !*was {
            keys.push((key, None));
        }
    }
    *stick = deflected;
    keys
}

#[cfg(test)]
mod tests {
    use bevy::input::gamepad::GamepadAxis;

    use super::*;

    // X14: d-pad, stick edges and face buttons reach the engine path as keys, without stick repeat.
    #[test]
    fn gamepad_presses_become_engine_keys() {
        let mut pad = Gamepad::default();
        pad.digital_mut().press(GamepadButton::DPadDown);
        pad.digital_mut().press(GamepadButton::South);
        pad.analog_mut().set(GamepadAxis::LeftStickX, 0.9);
        let mut stick = [false; 4];
        let keys = |pad: &Gamepad, stick: &mut [bool; 4]| -> Vec<KeyCode> {
            gamepad_keys(std::iter::once(pad), stick)
                .into_iter()
                .map(|(key, _)| key)
                .collect()
        };
        assert_eq!(
            keys(&pad, &mut stick),
            [KeyCode::ArrowDown, KeyCode::Enter, KeyCode::ArrowRight]
        );
        pad.digital_mut().clear();
        assert!(keys(&pad, &mut stick).is_empty());
    }
    #[test]
    fn horizontal_focus_reveals_the_controls_horizontal_span() {
        use crate::ui_runtime::presentation::forms::{
            pack_harness, tests::mini_engine_presentation,
        };
        let mut presentation = mini_engine_presentation();
        let mut player_runtime = crate::player_runtime::PlayerRuntime::new(1);
        let mut runtime = pack_harness::action_form(&mut player_runtime, "Test", &["Button"]);
        presentation
            .build(
                &player_runtime,
                &runtime,
                0,
                [800, 600],
                ui::DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let identity = runtime.server_forms().active().unwrap().identity;
        let mut frame = presentation.form_engine_frame(identity).unwrap().clone();
        let mut region = frame
            .hits
            .iter()
            .find(|region| region.kind == HitKind::Button)
            .unwrap()
            .clone();
        region.key = "scroll/button".into();
        region.rect = RectOut {
            x: 150.0,
            y: 10.0,
            w: 20.0,
            h: 20.0,
        };
        let mut scroll = region.clone();
        scroll.key = "scroll".into();
        scroll.kind = HitKind::ScrollView;
        frame.hits = vec![scroll, region].into();
        frame.report.scrolls.insert(
            "scroll".into(),
            json_ui::ScrollMetrics {
                content: 300.0,
                viewport: 100.0,
                horizontal: true,
                ..Default::default()
            },
        );
        focus(&mut runtime, &frame, "scroll/button".into());
        assert_eq!(runtime.server_forms().engine().view.scroll["scroll"], 70.0);
    }
}
