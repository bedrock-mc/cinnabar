//! Vanilla entity shadows: a polygonal volume hanging under each caster's feet, darkening every
//! opaque surface inside it. Rules are tabulated in `docs/reference/entity-shadows.md`.
use std::sync::Arc;

/// Sides of the volume's cross-section.
pub const SHADOW_VOLUME_SIDES: usize = 13;
/// Volume heights and ring radii, in multiples of the caster's shadow radius about its feet.
pub const SHADOW_VOLUME_TOP_Y: f32 = 0.01;
pub const SHADOW_VOLUME_TOP_RADIUS: f32 = 0.75;
pub const SHADOW_VOLUME_BOTTOM_Y: f32 = -3.0;
pub const SHADOW_VOLUME_BOTTOM_RADIUS: f32 = 0.25;
/// Side quads plus a triangle fan on each cap.
pub const SHADOW_VOLUME_VERTICES: usize =
    (SHADOW_VOLUME_SIDES * 2 + (SHADOW_VOLUME_SIDES - 2) * 2) * 3;
/// Most casters one frame draws; bounded by the tracked actor table.
pub const MAX_ENTITY_SHADOWS: usize = 8_192;

/// Neutral multiplier the scene colour is scaled by inside a shadow.
const SHADOW_GREY: f32 = 0.7;
/// Largest per-channel tint the sky adds around the grey.
const SHADOW_TINT_SPAN: f32 = 0.03;
const LUMINANCE: [f32; 3] = [0.2126, 0.7152, 0.0722];

/// One caster as the GPU reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityShadow {
    pub feet: [f32; 3],
    pub radius: f32,
}

impl EntityShadow {
    /// Whether `point` lies inside the volume.
    #[must_use]
    pub fn contains(&self, point: [f32; 3]) -> bool {
        if self.radius.partial_cmp(&0.0) != Some(std::cmp::Ordering::Greater) {
            return false;
        }
        let local = std::array::from_fn(|axis| (point[axis] - self.feet[axis]) / self.radius);
        unit_volume_contains(local)
    }

    /// World-space `(min, max)` of the volume.
    #[must_use]
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let [x, y, z] = self.feet;
        let reach = SHADOW_VOLUME_TOP_RADIUS * self.radius;
        (
            [
                x - reach,
                y + SHADOW_VOLUME_BOTTOM_Y * self.radius,
                z - reach,
            ],
            [x + reach, y + SHADOW_VOLUME_TOP_Y * self.radius, z + reach],
        )
    }
}

/// Containment in radius-1 space about the feet; the shader evaluates the same planes.
#[must_use]
pub fn unit_volume_contains([x, y, z]: [f32; 3]) -> bool {
    if !(SHADOW_VOLUME_BOTTOM_Y..=SHADOW_VOLUME_TOP_Y).contains(&y) {
        return false;
    }
    let params = EntityShadowParams::new([1.0; 4]);
    let apothem = params.volume[2] + params.volume[3] * y;
    side_normals()
        .iter()
        .all(|[nx, nz]| x * nx + z * nz <= apothem)
}

/// Outward horizontal normals of the side faces, `(x, z)`.
fn side_normals() -> [[f32; 2]; SHADOW_VOLUME_SIDES] {
    std::array::from_fn(|side| {
        let (sine, cosine) = mid_angle(side).sin_cos();
        [cosine, -sine]
    })
}

fn ring_angle(index: usize) -> f32 {
    index as f32 / SHADOW_VOLUME_SIDES as f32 * std::f32::consts::TAU
}

fn mid_angle(side: usize) -> f32 {
    (side as f32 + 0.5) / SHADOW_VOLUME_SIDES as f32 * std::f32::consts::TAU
}

fn ring_point(index: usize, top: bool) -> [f32; 3] {
    let (y, radius) = if top {
        (SHADOW_VOLUME_TOP_Y, SHADOW_VOLUME_TOP_RADIUS)
    } else {
        (SHADOW_VOLUME_BOTTOM_Y, SHADOW_VOLUME_BOTTOM_RADIUS)
    };
    let (sine, cosine) = ring_angle(index % SHADOW_VOLUME_SIDES).sin_cos();
    [cosine * radius, y, -sine * radius]
}

/// Radius-1 triangle list, wound counter-clockwise seen from outside.
#[must_use]
pub fn shadow_volume_mesh() -> [[f32; 3]; SHADOW_VOLUME_VERTICES] {
    let mut triangles = Vec::with_capacity(SHADOW_VOLUME_VERTICES / 3);
    for side in 0..SHADOW_VOLUME_SIDES {
        let (low, high) = (ring_point(side, false), ring_point(side + 1, false));
        let (top_low, top_high) = (ring_point(side, true), ring_point(side + 1, true));
        triangles.push([low, high, top_high]);
        triangles.push([low, top_high, top_low]);
    }
    for index in 1..SHADOW_VOLUME_SIDES - 1 {
        let ring = |top| {
            [
                ring_point(0, top),
                ring_point(index, top),
                ring_point(index + 1, top),
            ]
        };
        triangles.push(ring(true));
        let [first, middle, last] = ring(false);
        triangles.push([first, last, middle]);
    }
    let corners: Vec<_> = triangles.into_iter().flatten().collect();
    corners
        .try_into()
        .expect("mesh size matches SHADOW_VOLUME_VERTICES")
}

/// Uniform block shared by every shadow in a frame.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct EntityShadowParams {
    /// Encoded-colour multiplier applied inside a shadow.
    pub colour: [f32; 4],
    /// Top y, bottom y, then the side apothem as `volume[2] + volume[3] * y`.
    pub volume: [f32; 4],
    /// Side normals, two `(x, z)` pairs per vector.
    pub normals: [[f32; 4]; SHADOW_VOLUME_SIDES.div_ceil(2)],
    pub sides: [u32; 4],
}

impl EntityShadowParams {
    #[must_use]
    pub fn new(colour: [f32; 4]) -> Self {
        let apothem = (std::f32::consts::PI / SHADOW_VOLUME_SIDES as f32).cos();
        let slope = apothem * (SHADOW_VOLUME_TOP_RADIUS - SHADOW_VOLUME_BOTTOM_RADIUS)
            / (SHADOW_VOLUME_TOP_Y - SHADOW_VOLUME_BOTTOM_Y);
        let mut normals = [[0.0; 4]; SHADOW_VOLUME_SIDES.div_ceil(2)];
        for (side, [x, z]) in side_normals().into_iter().enumerate() {
            normals[side / 2][(side % 2) * 2] = x;
            normals[side / 2][(side % 2) * 2 + 1] = z;
        }
        Self {
            colour,
            volume: [
                SHADOW_VOLUME_TOP_Y,
                SHADOW_VOLUME_BOTTOM_Y,
                apothem * SHADOW_VOLUME_BOTTOM_RADIUS - slope * SHADOW_VOLUME_BOTTOM_Y,
                slope,
            ],
            normals,
            sides: [SHADOW_VOLUME_SIDES as u32, 0, 0, 0],
        }
    }
}

/// The encoded-colour multiplier: a 0.7 grey nudged up to 0.03 per channel toward the hue of the
/// sky (half strength over a 0.4 floor) blended with the sunrise glow by its alpha.
#[must_use]
pub fn entity_shadow_colour(sky: [f32; 3], sunrise: [f32; 4]) -> [f32; 4] {
    let glow = if sunrise[3].is_finite() {
        sunrise[3].clamp(0.0, 1.0)
    } else {
        0.0
    };
    let tint: [f32; 3] = std::array::from_fn(|channel| {
        (sky[channel] * 0.5 + 0.4) * (1.0 - glow) + sunrise[channel] * glow
    });
    let luminance: f32 = tint
        .iter()
        .zip(LUMINANCE)
        .map(|(value, weight)| value * weight)
        .sum();
    let deviation = tint.map(|value| value - luminance);
    let largest = deviation
        .iter()
        .fold(0.0_f32, |largest, value| largest.max(value.abs()));
    // A grey tint's rounding noise reads as grey rather than an arbitrary hue.
    if !(largest > 1.0e-6 && largest.is_finite()) {
        return [SHADOW_GREY, SHADOW_GREY, SHADOW_GREY, 1.0];
    }
    let scale = SHADOW_TINT_SPAN / largest;
    let [r, g, b] = deviation.map(|value| SHADOW_GREY + value * scale);
    [r, g, b, 1.0]
}

/// Pixel rectangle `[x0, y0, x1, y1)` covering every volume inside `viewport` (`[x, y, w, h]`),
/// or `None` when none reaches it. A volume touching the camera plane covers the viewport.
#[must_use]
pub fn shadow_screen_rect(
    clip_from_world: glam::Mat4,
    shadows: &[EntityShadow],
    viewport: [u32; 4],
) -> Option<[u32; 4]> {
    let [vx, vy, width, height] = viewport.map(|value| value as f32);
    let mut low = [f32::INFINITY; 2];
    let mut high = [f32::NEG_INFINITY; 2];
    for shadow in shadows {
        let (min, max) = shadow.bounds();
        let corners: [glam::Vec4; 8] = std::array::from_fn(|corner| {
            let pick = |axis: usize| {
                if corner >> axis & 1 == 0 {
                    min[axis]
                } else {
                    max[axis]
                }
            };
            clip_from_world * glam::Vec4::new(pick(0), pick(1), pick(2), 1.0)
        });
        let behind = corners
            .iter()
            .filter(|clip| clip.w.partial_cmp(&1.0e-4) != Some(std::cmp::Ordering::Greater))
            .count();
        if behind == corners.len() {
            continue;
        }
        if behind > 0 {
            low = [vx, vy];
            high = [vx + width, vy + height];
            break;
        }
        for clip in corners {
            let x = vx + (clip.x / clip.w * 0.5 + 0.5) * width;
            let y = vy + (0.5 - clip.y / clip.w * 0.5) * height;
            low = [low[0].min(x), low[1].min(y)];
            high = [high[0].max(x), high[1].max(y)];
        }
    }
    let x0 = low[0].floor().max(vx);
    let y0 = low[1].floor().max(vy);
    let x1 = high[0].ceil().min(vx + width);
    let y1 = high[1].ceil().min(vy + height);
    (x0 < x1 && y0 < y1).then_some([x0 as u32, y0 as u32, x1 as u32, y1 as u32])
}

/// This frame's casters; the revision moves only when the list changes.
#[derive(Clone, Debug, PartialEq)]
pub struct EntityShadowFrame {
    pub shadows: Arc<[EntityShadow]>,
    pub revision: u64,
}

impl Default for EntityShadowFrame {
    fn default() -> Self {
        Self {
            shadows: Arc::from([]),
            revision: 0,
        }
    }
}

impl EntityShadowFrame {
    /// Adopts `staged`, allocating only when it differs from the current list.
    pub fn publish(&mut self, staged: &[EntityShadow]) -> bool {
        let staged = &staged[..staged.len().min(MAX_ENTITY_SHADOWS)];
        if *self.shadows == *staged {
            return false;
        }
        self.shadows = Arc::from(staged);
        self.revision = self.revision.wrapping_add(1);
        true
    }
}

#[cfg(test)]
mod tests;
