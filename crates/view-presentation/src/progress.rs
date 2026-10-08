//! Native authored progress-bar paint decisions shared by presentation adapters.

use std::collections::BTreeMap;
use serde_json::Value;

type Data = BTreeMap<String, Value>;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressRect { pub bounds: [f32; 4], pub color: [u8; 4] }

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProgressPaint { pub shadow: Option<ProgressRect>, pub background: ProgressRect, pub fill: ProgressRect }

impl ProgressPaint {
    pub fn rects(self) -> impl Iterator<Item = ProgressRect> { self.shadow.into_iter().chain([self.background, self.fill]) }
}

fn option<'a>(data: &'a Data, key: &str) -> Option<&'a Value> {
    data.get("property_bag").and_then(|bag| bag.get(key)).or_else(|| data.get(key))
}
fn flag(data: &Data, key: &str) -> bool { option(data, key).and_then(Value::as_bool).unwrap_or(false) }

pub fn capture_progress(data: &Data, dest: [f32; 4], gui_pixel_scale: f32) -> Option<ProgressPaint> {
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
        let px = gui_pixel_scale;
        let snap = |value: f32| (value / px).floor() * px;
        let (x0, y0) = (snap(dest[0]), snap(dest[1]));
        let (w, h) = (dest[2] - dest[0], dest[3] - dest[1]);
        let shadow = if flag(data, "drop_shadow") {
            Some(ProgressRect { bounds: [x0, y0, x0 + w + px, y0 + h + px], color: [0, 0, 0, 255] })
        } else { None };
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
        Some(ProgressPaint {
            shadow,
            background: ProgressRect { bounds: [x0, y0, x0 + w, y0 + h], color: secondary },
            fill: ProgressRect { bounds: [x0, y0, x0 + fill.max(0.0), y0 + h], color: primary },
        })
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
