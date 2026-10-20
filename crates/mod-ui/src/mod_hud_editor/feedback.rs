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
    pub dragging: bool,
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
        if self.dragging {
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
        }
        Ok(true)
    }
}

/// Writes two nearest-edge measurements in GUI pixels, hidden when less than 12 px long.
pub(super) fn measurements(
    data: &mut DataSource,
    hud: &Hud,
    viewport: [f64; 2],
    selected: Option<usize>,
) {
    let rect = selected
        .and_then(|index| hud.cards.get(index))
        .map(|card| (cards::origin(card, viewport), cards::dimensions(card)));
    for axis in 0..2 {
        let mut offset = [0.; 2];
        let mut size = [GUIDE_WIDTH; 2];
        let mut pill = [0.; 2];
        let mut distance = 0.;
        if let Some((at, extent)) = rect {
            let far = (viewport[axis] - at[axis] - extent[axis]).max(0.);
            let near = at[axis] <= far;
            distance = if near { at[axis] } else { far };
            offset[axis] = if near { 0. } else { at[axis] + extent[axis] };
            offset[1 - axis] = at[1 - axis] + extent[1 - axis] * 0.5;
            size[axis] = distance;
            pill = offset;
            pill[axis] += distance * 0.5;
            for axis in 0..2 {
                pill[axis] = (pill[axis] - MEASUREMENT_PILL[axis] * 0.5)
                    .clamp(0., (viewport[axis] - MEASUREMENT_PILL[axis]).max(0.));
            }
        }
        for (suffix, value) in [
            ("visible", Scalar::Bool(distance >= 12.)),
            ("offset", Scalar::Json(serde_json::json!(offset))),
            ("width", Scalar::Num(size[0])),
            ("height", Scalar::Num(size[1])),
            ("pill", Scalar::Json(serde_json::json!(pill))),
            ("label", Scalar::Text(format!("{distance:.0} px"))),
        ] {
            data.set_global(format!("#measurement_{axis}_{suffix}"), value);
        }
    }
}
