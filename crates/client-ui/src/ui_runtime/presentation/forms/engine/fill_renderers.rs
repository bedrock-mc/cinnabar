//! Custom renderers vanilla draws as flat fills: `progress_bar_renderer`
//! and `gradient_renderer`, as 1.26.50 draws them.

use std::collections::BTreeMap;

use serde_json::Value;
use ui::UiVisual;

use super::Painter;

type Data = BTreeMap<String, Value>;

impl Painter<'_> {
    /// Background then fill; a durability bar's fill sweeps red to green and a
    /// full storage bar takes `full_storage_color`. Returns nothing to push.
    pub(super) fn progress_bar(
        &mut self,
        data: &Data,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<()> {
        let paint = view_presentation::progress::capture_progress(data, dest, self.px)?;
        for rect in paint.rects() { self.solid(rect.bounds, alpha(rect.color)).ok()?; }
        Some(())
    }

    /// `color1` to `color2` top to bottom, or left to right when
    /// `gradient_direction` is `horizontal`.
    pub(super) fn gradient(
        &self,
        data: &Data,
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<UiVisual> {
        let color = |key: &str| data.get(key).and_then(json_ui::color_value);
        Some(UiVisual::Gradient {
            texture_page: self.solid_page,
            colors: [
                alpha(color("color1").unwrap_or([255; 4])),
                alpha(color("color2").unwrap_or([255; 4])),
            ],
            horizontal: data.get("gradient_direction").and_then(Value::as_str)
                == Some("horizontal"),
        })
    }
}
