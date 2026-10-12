//! Keyboard and gamepad focus movement for an engine-drawn form, through the
//! engine's vanilla focus navigation.

use json_ui::{FocusDirection, FocusMove, HitKind, HitRegion, RectOut};

use semantic_input::KeyboardKey;

use super::values::EngineFrame;
use crate::ui_runtime::UiRuntime;

/// The focus direction an arrow key moves.
pub fn direction_of(key: KeyboardKey) -> Option<FocusDirection> {
    Some(match key {
        KeyboardKey::ArrowUp => FocusDirection::Up,
        KeyboardKey::ArrowDown => FocusDirection::Down,
        KeyboardKey::ArrowLeft => FocusDirection::Left,
        KeyboardKey::ArrowRight => FocusDirection::Right,
        _ => return None,
    })
}

/// Move focus toward a `direction` the dispatcher left unconsumed; a control
/// claiming the direction already received it as `button.menu_*`.
pub fn step(runtime: &mut UiRuntime, frame: &EngineFrame, direction: FocusDirection) {
    let view = &runtime.server_forms().engine().view;
    if let FocusMove::Moved(key) =
        json_ui::navigate(&frame.hits, view, direction, screen(&frame.hits))
    {
        focus(runtime, frame, key);
    }
}

/// Tab: the next focusable control in document order.
pub fn tab(runtime: &mut UiRuntime, frame: &EngineFrame) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_focus_reveals_the_controls_horizontal_span() {
        use crate::ui_runtime::presentation::forms::{
            pack_harness, tests::mini_engine_presentation,
        };
        let mut presentation = mini_engine_presentation();
        let mut player_runtime = player_state::PlayerState::new(1);
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
