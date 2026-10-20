//! Rounded renderer shadow properties, in GUI pixels before paint transforms.
//!
//! `shadow_color` enables an RGBA shadow; `shadow_offset` translates it by `[x, y]`.
//! `shadow_blur` is its Gaussian standard deviation, and `shadow_spread` expands or
//! contracts its silhouette. The last three properties default to zero. Binding
//! overrides use `#shadow_color`, `#shadow_offset`, `#shadow_blur` and `#shadow_spread`.
//! Surface literals outside the bounds are rejected; dynamic values clamp to them.

use crate::Rect;
use serde_json::Value;
use std::collections::BTreeMap;

pub const RENDERER: &str = "cinnabar_rounded_rectangle";
pub const DEFAULT_RADIUS: f32 = 6.0;
/// Largest Gaussian standard deviation and absolute spread in GUI pixels.
pub const MAX_SHADOW_BLUR: f64 = 48.0;
pub const MAX_SHADOW_SPREAD: f64 = MAX_SHADOW_BLUR;
/// Largest absolute shadow translation on either axis, in GUI pixels.
pub const MAX_SHADOW_OFFSET: f64 = 4096.0;
/// Truncate the Gaussian tail at three standard deviations.
pub const SHADOW_SUPPORT: f32 = 3.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shadow {
    pub color: [u8; 4],
    pub offset: [f32; 2],
    pub blur: f32,
    pub spread: f32,
}

impl Shadow {
    /// Reads literals or binding overrides, clamping dynamic values to the surface bounds.
    /// Missing or transparent colors disable the shadow entirely.
    pub fn read(data: &BTreeMap<String, Value>) -> Option<Self> {
        let color = value(data, "shadow_color", "#shadow_color").and_then(crate::color_value)?;
        if color[3] == 0 {
            return None;
        }
        let offset = value(data, "shadow_offset", "#shadow_offset")
            .and_then(Value::as_array)
            .filter(|items| items.len() == 2);
        Some(Self {
            color,
            offset: std::array::from_fn(|axis| {
                clamp(
                    offset.and_then(|items| items[axis].as_f64()),
                    -MAX_SHADOW_OFFSET,
                    MAX_SHADOW_OFFSET,
                )
            }),
            blur: clamp(
                value(data, "shadow_blur", "#shadow_blur").and_then(Value::as_f64),
                0.0,
                MAX_SHADOW_BLUR,
            ),
            spread: clamp(
                value(data, "shadow_spread", "#shadow_spread").and_then(Value::as_f64),
                -MAX_SHADOW_SPREAD,
                MAX_SHADOW_SPREAD,
            ),
        })
    }

    /// Shadow coverage, including the Gaussian support or a half-pixel sharp edge fringe.
    pub fn bounds(self, dest: Rect, pixel: f64) -> Rect {
        self.bounds_at(dest, pixel, [1.0; 2])
    }

    /// Keeps authored bounds and allows both size ceiling and position truncation at paint time.
    pub(crate) fn subtree_bounds(self, dest: Rect, pixel: f64) -> Rect {
        let enlarged = Rect::new(dest.x, dest.y, dest.w + pixel, dest.h + pixel);
        dest.union(self.bounds(enlarged, pixel)).expanded(pixel)
    }

    /// Unbakes the renderer's geometric-mean length scaling for conservative culling.
    fn bounds_at(self, dest: Rect, pixel: f64, axes: [f64; 2]) -> Rect {
        let scale = (axes[0] * axes[1]).sqrt();
        let padding = f64::from(self.spread) * scale
            + (f64::from(self.blur * SHADOW_SUPPORT) * scale).max(pixel * 0.5);
        Rect::new(
            dest.x + (f64::from(self.offset[0]) * scale - padding) / axes[0],
            dest.y + (f64::from(self.offset[1]) * scale - padding) / axes[1],
            (dest.w + padding * 2.0 / axes[0]).max(0.0),
            (dest.h + padding * 2.0 / axes[1]).max(0.0),
        )
    }
}

/// A binding override wins over its literal without allocating a temporary property name.
fn value<'a>(data: &'a BTreeMap<String, Value>, name: &str, bound: &str) -> Option<&'a Value> {
    data.get(bound).or_else(|| data.get(name))
}

/// A finite dynamic length clamped to its range, or zero for invalid or missing values.
fn clamp(value: Option<f64>, min: f64, max: f64) -> f32 {
    value
        .filter(|value| value.is_finite())
        .unwrap_or(0.0)
        .clamp(min, max) as f32
}

/// Conservatively includes the shadow when culling a rounded node or its subtree.
pub(crate) fn paint_bounds(
    data: &BTreeMap<String, Value>,
    dest: Rect,
    pixel: f64,
    transform: Option<crate::transform::Affine>,
) -> Rect {
    let axes = transform.map_or([1.0; 2], |transform| {
        let [a, b, c, d] = transform.linear;
        [a.hypot(b), c.hypot(d)]
    });
    if axes.iter().any(|axis| *axis < 1e-6) {
        return dest;
    }
    Shadow::read(data).map_or(dest, |shadow| {
        dest.union(shadow.bounds_at(dest, pixel, axes))
    })
}

#[cfg(test)]
mod tests;
