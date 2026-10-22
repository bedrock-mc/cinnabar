//! Bound distance annotations and host-owned drag-only keep-out shading.

use super::*;
use ui_engine::engine::{
    Canvas,
    screen::{CustomDraw, PaintError},
};

/// Paints clipped parallel hatch lines without storing or tessellating geometry.
/// Strokes stay at least one physical pixel wide at every GUI scale and DPI.
pub(super) struct KeepOutRenderer<'a> {
    pub rects: &'a [[f64; 4]],
}

impl CustomRenderer for KeepOutRenderer<'_> {
    fn paint(
        &mut self,
        node: &CustomDraw<'_>,
        canvas: &mut Canvas<'_>,
    ) -> Result<bool, PaintError> {
        if node.renderer != "cinnabar_hud_keep_out" {
            return Ok(false);
        }
        let px = f64::from(canvas.px());
        let hatch_width = GUIDE_WIDTH.max(1. / f64::from(canvas.metrics().gui_scale));
        for rect in self.rects {
            let x = rect[0].max(0.);
            let y = rect[1].max(0.);
            let right = (rect[0] + rect[2]).min(f64::from(node.dest[2] - node.dest[0]) / px);
            let bottom = (rect[1] + rect[3]).min(f64::from(node.dest[3] - node.dest[1]) / px);
            if x >= right || y >= bottom {
                continue;
            }
            let bounds = |a: f64, b: f64, c: f64, d: f64| {
                [
                    node.dest[0] + (a * px) as f32,
                    node.dest[1] + (b * px) as f32,
                    node.dest[0] + (c * px) as f32,
                    node.dest[1] + (d * px) as f32,
                ]
            };
            canvas.solid(bounds(x, y, right, bottom), node.alpha([255, 191, 105, 12]))?;
            let mut line = y;
            while line < bottom {
                canvas.solid(
                    bounds(x, line, right, (line + hatch_width).min(bottom)),
                    node.alpha([255, 191, 105, 48]),
                )?;
                line += snapping::GRID;
            }
        }
        Ok(true)
    }
}

/// Pill size that fits `label` at the measurement font scale.
pub(super) fn pill_size(label: &str) -> [f64; 2] {
    [
        label.chars().count() as f64 * MEASUREMENT_DIGIT_ADVANCE + 2. * MEASUREMENT_PILL_PADDING,
        MEASUREMENT_PILL_HEIGHT,
    ]
}

/// Writes two nearest-edge measurements in GUI pixels, hidden when less than 12 px long.
pub(super) fn measurements(
    data: &mut DataSource,
    viewport: [f64; 2],
    rect: Option<([f64; 2], [f64; 2])>,
    obstacles: &[[f64; 4]],
    scale: f64,
) {
    for axis in 0..2 {
        let mut offset = [0.; 2];
        let mut size = [GUIDE_WIDTH; 2];
        let mut pill = [0.; 2];
        let mut distance = 0.;
        let mut caption_visible = false;
        let mut label = String::new();
        let mut extent_of_pill = pill_size("").map(|size| size * scale);
        if let Some((at, extent)) = rect {
            let far = (viewport[axis] - at[axis] - extent[axis]).max(0.);
            let near = at[axis] <= far;
            distance = if near { at[axis] } else { far };
            offset[axis] = if near { 0. } else { at[axis] + extent[axis] };
            offset[1 - axis] = at[1 - axis] + extent[1 - axis] * 0.5;
            size[axis] = distance;
            label = format!("{distance:.0}");
            extent_of_pill = pill_size(&label).map(|size| size * scale);
            pill = offset;
            pill[axis] += distance * 0.5;
            for axis in 0..2 {
                pill[axis] = (pill[axis] - extent_of_pill[axis] * 0.5)
                    .clamp(0., (viewport[axis] - extent_of_pill[axis]).max(0.));
            }
            if let Some(at) =
                caption_position(viewport, pill, extent_of_pill, at, extent, obstacles)
            {
                pill = at;
                caption_visible = true;
            }
        }
        for (suffix, value) in [
            ("visible", Scalar::Bool(distance >= 12. && caption_visible)),
            ("offset", Scalar::Json(serde_json::json!(offset))),
            ("width", Scalar::Num(size[0])),
            ("height", Scalar::Num(size[1])),
            ("pill", Scalar::Json(serde_json::json!(pill))),
            ("pill_width", Scalar::Num(extent_of_pill[0])),
            ("label", Scalar::Text(label)),
        ] {
            data.set_global(format!("#measurement_{axis}_{suffix}"), value);
        }
    }
}

/// Keeps a compact distance caption near its guide without painting over any widget.
fn caption_position(
    viewport: [f64; 2],
    preferred: [f64; 2],
    pill_size: [f64; 2],
    at: [f64; 2],
    size: [f64; 2],
    obstacles: &[[f64; 4]],
) -> Option<[f64; 2]> {
    let [w, h] = pill_size;
    let left = at[0] - w - 3.;
    let right = at[0] + size[0] + 3.;
    let top = at[1] - h - 3.;
    let bottom = at[1] + size[1] + 3.;
    [
        preferred,
        [preferred[0], top],
        [preferred[0], bottom],
        [left, preferred[1]],
        [right, preferred[1]],
        [left, top],
        [right, top],
        [left, bottom],
        [right, bottom],
    ]
    .into_iter()
    .map(|point| {
        std::array::from_fn(|axis| {
            point[axis].clamp(0., (viewport[axis] - pill_size[axis]).max(0.))
        })
    })
    .find(|point| {
        point[0] + w <= viewport[0]
            && point[1] + h <= viewport[1]
            && obstacles.iter().all(|rect| {
                point[0] + w + 1. <= rect[0]
                    || point[0] >= rect[0] + rect[2] + 1.
                    || point[1] + h + 1. <= rect[1]
                    || point[1] >= rect[1] + rect[3] + 1.
            })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Short edge gaps must not put their wider captions over the selected or adjacent widget.
    #[test]
    fn short_measurements_clear_neighboring_keystrokes_and_hide_when_no_space_remains() {
        let selected = [12., 37., 56., 18.];
        let keys = [0., 60., 70., 82.];
        let mut data = DataSource::new();
        measurements(
            &mut data,
            [400., 240.],
            Some(([12., 37.], [56., 18.])),
            &[selected, keys],
            1.,
        );
        for axis in 0..2 {
            assert_eq!(
                data.global_value(&format!("#measurement_{axis}_visible")),
                Some(&Scalar::Bool(true))
            );
            let Scalar::Json(value) = data
                .global_value(&format!("#measurement_{axis}_pill"))
                .unwrap()
            else {
                panic!("caption position")
            };
            let point: [f64; 2] = serde_json::from_value(value.clone()).unwrap();
            for rect in [selected, keys] {
                assert!(
                    point[0] + pill_size(if axis == 0 { "12" } else { "37" })[0] <= rect[0]
                        || point[0] >= rect[0] + rect[2]
                        || point[1] + MEASUREMENT_PILL_HEIGHT <= rect[1]
                        || point[1] >= rect[1] + rect[3]
                );
            }
        }
        measurements(
            &mut data,
            [400., 240.],
            Some(([12., 37.], [56., 18.])),
            &[[0., 0., 400., 240.]],
            1.,
        );
        assert_eq!(
            data.global_value("#measurement_0_visible"),
            Some(&Scalar::Bool(false))
        );
    }
}
