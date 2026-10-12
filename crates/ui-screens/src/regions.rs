//! Engine hit regions in window-logical pixels, as the screens' input adapters hit-test them.

use json_ui::HitRegion;
use ui::UiRect;

/// A region's clipped rect in window-logical pixels.
pub fn window_rect(region: &HitRegion, scale: f32, origin: [f32; 2]) -> Option<UiRect> {
    let x0 = region.rect.x.max(region.clip.x);
    let y0 = region.rect.y.max(region.clip.y);
    let x1 = (region.rect.x + region.rect.w).min(region.clip.x + region.clip.w);
    let y1 = (region.rect.y + region.rect.h).min(region.clip.y + region.clip.h);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    let to = |value: f64, axis: usize| value as f32 * scale + origin[axis];
    ui_engine::engine::rect(to(x0, 0), to(y0, 1), to(x1, 0), to(y1, 1)).ok()
}

/// Regions select the nearest slider anchor, including anchors at both ends
/// of the track. The end values therefore occupy half an interior interval.
pub fn segments(
    region: &HitRegion,
    steps: usize,
    scale: f32,
    origin: [f32; 2],
) -> Vec<(usize, UiRect)> {
    let interval = region.rect.w / steps.saturating_sub(1).max(1) as f64;
    (0..steps)
        .filter_map(|step| {
            let mut part = region.clone();
            let left = if step == 0 {
                0.0
            } else {
                (step as f64 - 0.5) * interval
            };
            let right = if step + 1 == steps {
                region.rect.w
            } else {
                (step as f64 + 0.5) * interval
            };
            part.rect.x = region.rect.x + left;
            part.rect.w = right - left;
            window_rect(&part, scale, origin).map(|bounds| (step, bounds))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use json_ui::{HitKind, RectOut};
    use ui::UiPoint;

    use super::*;

    #[test]
    fn gui_scale_slider_regions_choose_the_nearest_native_step() {
        let rect = RectOut {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 12.0,
        };
        let region = HitRegion {
            key: "gui_scale".into(),
            name: "gui_scale".into(),
            kind: HitKind::Slider,
            rect,
            clip: rect,
            layer: 0,
            order: 0,
            pressed: None,
            control_name: Some("gui_scale".into()),
            collection_index: None,
            collection: None,
            enabled: true,
            checked: None,
            max_length: None,
            group_index: None,
            renderer: None,
            drag_axes: [false; 2],
            sound: None,
            input: Default::default(),
            focus: None,
            collections: Vec::new(),
            widget: Default::default(),
            modal_root: None,
        };
        let hits = segments(&region, 3, 2.0, [5.0, 7.0]);
        for (track_x, expected) in [
            (0.0, 0),
            (24.0, 0),
            (25.0, 1),
            (30.0, 1),
            (70.0, 1),
            (75.0, 2),
            (99.0, 2),
        ] {
            let point = UiPoint::new(5.0 + 2.0 * (10.0 + track_x), 7.0 + 2.0 * 26.0).unwrap();
            let selected = hits
                .iter()
                .rev()
                .find(|(_, bounds)| bounds.contains(point))
                .map(|(step, _)| *step);
            assert_eq!(selected, Some(expected), "track position {track_x}");
        }
    }
}
