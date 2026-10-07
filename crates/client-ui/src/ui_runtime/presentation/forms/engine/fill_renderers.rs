//! Custom renderers vanilla draws as flat fills: `progress_bar_renderer`
//! and `gradient_renderer`, as 1.26.50 draws them.

use std::collections::BTreeMap;

use serde_json::Value;
use ui::UiVisual;

use super::Painter;

type Data = BTreeMap<String, Value>;

/// A renderer option read from its `property_bag`, where vanilla packs put
/// them, or the control itself.
fn option<'a>(data: &'a Data, key: &str) -> Option<&'a Value> {
    data.get("property_bag")
        .and_then(|bag| bag.get(key))
        .or_else(|| data.get(key))
}

fn flag(data: &Data, key: &str) -> bool {
    option(data, key).and_then(Value::as_bool).unwrap_or(false)
}

impl Painter<'_> {
    /// Background then fill; a durability bar's fill sweeps red to green and a
    /// full storage bar takes `full_storage_color`. Returns nothing to push.
    pub(super) fn progress_bar(
        &mut self,
        data: &Data,
        dest: [f32; 4],
        alpha: &dyn Fn([u8; 4]) -> [u8; 4],
    ) -> Option<()> {
        let number = |key: &str| data.get(key).and_then(Value::as_f64);
        let storage = flag(data, "is_storage_bar");
        let durability = flag(data, "is_durability");
        // Durability bars (and every non-storage bar on touch) use the touch flag.
        let visible = if !storage && durability {
            "#touch_progress_bar_visible"
        } else {
            "#progress_bar_visible"
        };
        if data.get(visible) != Some(&Value::Bool(true)) {
            return None;
        }
        let total = number("#progress_bar_total_amount").unwrap_or(0.0);
        let current = number("#progress_bar_current_amount").unwrap_or(0.0);
        let fraction = if total > 0.0 {
            (current / total).min(1.0)
        } else {
            0.0
        };
        let px = self.px;
        let snap = |value: f32| (value / px).floor() * px;
        let (x0, y0) = (snap(dest[0]), snap(dest[1]));
        let (w, h) = (dest[2] - dest[0], dest[3] - dest[1]);
        if flag(data, "drop_shadow") {
            self.solid([x0, y0, x0 + w + px, y0 + h + px], alpha([0, 0, 0, 255]))
                .ok()?;
        }
        let color = |key: &str| option(data, key).and_then(json_ui::color_value);
        let mut fill = snap(x0 + fraction as f32 * w) - x0;
        if flag(data, "round_value") {
            fill = (fraction as f32 * w / px).round() * px;
        }
        let (primary, secondary) = if durability {
            (durability_color(fraction), [0, 0, 0, 255])
        } else {
            (
                color("primary_color").unwrap_or([255; 4]),
                color("secondary_color").unwrap_or([255; 4]),
            )
        };
        let mut primary = primary;
        if storage {
            fill = (((w / px - 1.0) * fraction as f32 + 1.0).round() * px).min(w);
            if fraction == 1.0
                && let Some(full) = color("full_storage_color")
            {
                primary = full;
            }
        }
        self.solid([x0, y0, x0 + w, y0 + h], alpha(secondary))
            .ok()?;
        self.solid([x0, y0, x0 + fill.max(0.0), y0 + h], alpha(primary))
            .ok()
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

/// Durability colour: the HSV hue `fraction / 3` turns, red worn to green full.
fn durability_color(fraction: f64) -> [u8; 4] {
    let hue = (fraction / 3.0).fract() * 6.0;
    let x = hue.fract() as f32;
    let (r, g) = if hue < 1.0 {
        (1.0, x)
    } else if hue < 2.0 {
        (1.0 - x, 1.0)
    } else {
        (0.0, 1.0)
    };
    [(r * 255.0).round() as u8, (g * 255.0).round() as u8, 0, 255]
}
