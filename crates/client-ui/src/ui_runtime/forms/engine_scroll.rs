//! Scroll input for a form drawn by the JSON-UI engine, as vanilla's scroll
//! view receives it: the wheel over its viewport or track, the track button
//! centring on the press, a draggable box, and a gesture-enabled touch pan
//! with its fling stepped every frame.

use std::time::Instant;

use bevy::input::mouse::MouseScrollUnit;
use json_ui::{Draggable, HitKind, HitRegion, ScrollMetrics};

use super::values::{EngineFrame, FormDrag};
use crate::ui_runtime::UiRuntime;

/// Wheel notches or pixels over the view the pointer reaches.
pub(super) fn wheel(
    runtime: &mut UiRuntime,
    frame: &EngineFrame,
    point: [f64; 2],
    notches: &[(f32, MouseScrollUnit)],
) {
    let Some(view) = json_ui::scroll_target(&frame.hits, &frame.report, point) else {
        return;
    };
    let Some(metrics) = frame.report.scrolls.get(&view.key) else {
        return;
    };
    let view_state = &mut runtime.server_forms_mut().engine_mut().view;
    let mut offset = view_state
        .scroll
        .get(&view.key)
        .copied()
        .unwrap_or(metrics.offset);
    for (notch, unit) in notches {
        let at = ScrollMetrics {
            offset,
            ..metrics.clone()
        };
        offset = match unit {
            MouseScrollUnit::Line => at.offset_for_wheel(f64::from(*notch)),
            MouseScrollUnit::Pixel => {
                (offset - f64::from(*notch / frame.scale)).clamp(0.0, metrics.max_offset())
            }
        };
    }
    view_state.scroll.insert(view.key.clone(), offset);
}

/// A press at `point` over `region`: `true` when a scrollbar took it, so it
/// presses nothing else. A press in a gesture view also starts a touch pan.
pub(super) fn press(
    runtime: &mut UiRuntime,
    frame: &EngineFrame,
    region: Option<&HitRegion>,
    point: [f64; 2],
) -> bool {
    let engine = runtime.server_forms_mut().engine_mut();
    if let Some(region) = region.filter(|region| region.enabled)
        && let Some(view) = owning_view(frame, region)
        && let Some(metrics) = frame.report.scrolls.get(&view.key)
    {
        if region.kind == HitKind::ScrollBox && metrics.box_drag != Draggable::NotDraggable {
            engine.drag = Some(FormDrag::ScrollBox {
                view: view.key.clone(),
                last: along(metrics, point),
            });
            return true;
        }
        if region.pressed.is_some() && region.pressed == metrics.track_button {
            let offset = metrics.offset_for_track(point);
            engine.view.scroll.insert(view.key.clone(), offset);
            return true;
        }
    }
    // A `draggable` control follows the pointer from where it was grabbed.
    if let Some(region) =
        region.filter(|region| region.enabled && region.kind == HitKind::Draggable)
    {
        engine.drag = Some(FormDrag::Control {
            key: region.key.clone(),
            last: point,
        });
        return true;
    }
    // The view maps `button.menu_select` to its touch button: a press anywhere in it.
    let touched = frame.hits.iter().rev().find(|view| {
        view.kind == HitKind::ScrollView
            && view.contains(point)
            && frame.report.scrolls.get(&view.key).is_some_and(|metrics| {
                metrics.touch_button.is_some() && metrics.touch_button == view.pressed
            })
    });
    if let Some(view) = touched
        && let Some(metrics) = frame.report.scrolls.get(&view.key)
    {
        engine.view.begin_scroll_touch(&view.key, metrics);
        engine.scroll_touch = Some((view.key.clone(), point));
    }
    false
}

/// Follow a held box drag or touch pan.
pub(super) fn drag(runtime: &mut UiRuntime, frame: &EngineFrame, point: [f64; 2]) {
    let engine = runtime.server_forms_mut().engine_mut();
    match engine.drag.clone() {
        // The box moves content pixels per track pixel from where it was last.
        Some(FormDrag::ScrollBox { view, last }) => {
            if let Some(metrics) = frame.report.scrolls.get(&view) {
                let at = along(metrics, point);
                let offset = metrics.thumb_drag_target(at - last);
                engine.view.scroll.insert(view.clone(), offset);
                engine.drag = Some(FormDrag::ScrollBox { view, last: at });
            }
        }
        Some(FormDrag::Control { key, last }) => {
            let axes = frame
                .hits
                .iter()
                .find(|region| region.key == key)
                .map_or([false; 2], |region| region.drag_axes);
            let moved = engine.view.drags.entry(key.clone()).or_insert([0.0; 2]);
            for axis in 0..2 {
                if axes[axis] {
                    moved[axis] += point[axis] - last[axis];
                }
            }
            engine.drag = Some(FormDrag::Control { key, last: point });
        }
        None => {}
    }
    if let Some((view, last)) = engine.scroll_touch.clone()
        && let Some(metrics) = frame.report.scrolls.get(&view)
    {
        let delta = [point[0] - last[0], point[1] - last[1]];
        engine.view.scroll_touch_moved(&view, metrics, delta);
        engine.scroll_touch = Some((view, point));
    }
}

/// The pointer lifts; `false` when a touch pan travelled far enough that the
/// press under it no longer counts.
pub(super) fn release(runtime: &mut UiRuntime) -> bool {
    let engine = runtime.server_forms_mut().engine_mut();
    match engine.scroll_touch.take() {
        Some((view, _)) => engine.view.end_scroll_touch(&view),
        None => true,
    }
}

/// Advance flings and touch-box fades, and adopt what the last layout settled.
pub(super) fn step(runtime: &mut UiRuntime, frame: &EngineFrame) {
    let engine = runtime.server_forms_mut().engine_mut();
    let now = Instant::now();
    let dt = engine
        .scroll_clock
        .map_or(0.0, |last| now.duration_since(last).as_secs_f64());
    engine.scroll_clock = Some(now);
    engine.view.settle(&frame.report);
    engine.view.step_scrolls(&frame.report, dt);
}

/// The innermost scroll view whose key prefixes `region`'s key.
fn owning_view<'a>(frame: &'a EngineFrame, region: &HitRegion) -> Option<&'a HitRegion> {
    frame
        .hits
        .iter()
        .filter(|view| view.kind == HitKind::ScrollView && region.key.starts_with(&view.key))
        .max_by_key(|view| view.key.len())
}

/// `point` along the view's scrolling axis.
fn along(metrics: &ScrollMetrics, point: [f64; 2]) -> f64 {
    if metrics.horizontal {
        point[0]
    } else {
        point[1]
    }
}
