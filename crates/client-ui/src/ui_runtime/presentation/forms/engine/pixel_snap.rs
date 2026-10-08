//! Draw destinations on whole physical pixels, where vanilla's UI render context puts them.
//!
//! Vanilla multiplies an image's position by the GUI scale, truncates it to a whole physical
//! pixel and scales it back by the inverse; the size is rounded up to whole pixels the same way:
//! `x' = (int)(x * s) / s`, `w' = ceil(w * s) / s`. Vanilla truncates text
//! positions the same way, and each line's alignment offset in the label's unscaled text space
//! (`(int)(offset * s) / s` per line). Without it a control centred in an odd free space starts
//! half a GUI unit off the pixel grid, and at an odd GUI scale its quad edges fall on half
//! pixels, where nearest sampling at the texel edge takes the texel outside its uv rect.

/// Float noise from layout arithmetic that must not move a position that is whole in physical
/// pixels (`2.9999998` physical pixels is 3).
const EPSILON: f32 = 1.0 / 1024.0;

/// `rect` (`x`, `y`, `w`, `h` in GUI units) as logical `[x0, y0, x1, y1]` whose physical edges
/// are whole pixels, as vanilla places an image: `pixels` physical pixels and `logical`
/// logical pixels per GUI unit (their ratio is the platform DPI).
pub(super) fn snapped(rect: [f64; 4], pixels: f32, logical: f32) -> [f32; 4] {
    let [x, y, w, h] = rect.map(|value| value as f32 * pixels);
    let size = |value: f32| (value - EPSILON).ceil().max(0.0);
    let (x0, y0) = (position(x), position(y));
    let to_logical = logical / pixels;
    [x0, y0, x0 + size(w), y0 + size(h)].map(|edge| edge * to_logical)
}

/// `rect` moved to a whole physical pixel with its size kept, as vanilla places text.
pub(super) fn positioned(rect: [f64; 4], pixels: f32, logical: f32) -> [f32; 4] {
    let [x, y, w, h] = rect.map(|value| value as f32 * pixels);
    let to_logical = logical / pixels;
    let (x0, y0) = (position(x) * to_logical, position(y) * to_logical);
    [x0, y0, x0 + w * to_logical, y0 + h * to_logical]
}

/// The grid, in 1/65536 logical pixels, that vanilla truncates a label's per-line alignment
/// offsets onto: it snaps them to whole physical pixels in the label's unscaled text space, so a
/// label at text scale `scale` steps by `scale` physical pixels.
pub(super) fn align_grid_65536(scale: f32, pixels: f32, logical: f32) -> u32 {
    (f64::from(scale) * f64::from(logical / pixels) * 65_536.0).round() as u32
}

/// A physical position truncated toward zero, as the `(int)` cast does.
fn position(value: f32) -> f32 {
    (value + EPSILON.copysign(value)).trunc()
}

#[cfg(test)]
mod tests;
