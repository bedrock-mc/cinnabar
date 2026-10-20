//! CPU sprite coverage, combined before filtering so every output pixel is painted once.

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct CoverageRect {
    bounds: [u32; 4],
    alpha: u8,
}

/// An immutable inner outline derived once from a complete source image's alpha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpriteMask {
    size: [u32; 2],
    rects: Vec<CoverageRect>,
}

impl SpriteMask {
    /// Ignores source RGB and atlas neighbors, retaining holes and fractional edge alpha.
    pub fn outline(size: [u32; 2], alpha: impl Fn(u32, u32) -> u8) -> Self {
        let [width, height] = size;
        let mut rects: Vec<CoverageRect> = Vec::new();
        let mut previous: BTreeMap<(u32, u32, u8), usize> = BTreeMap::new();
        for y in 0..height {
            let mut next = BTreeMap::new();
            let mut run = None;
            for x in 0..=width {
                let coverage = if x == width {
                    0
                } else {
                    let center = alpha(x, y);
                    let mut interior = center;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
                            interior = interior.min(
                                if nx < 0
                                    || ny < 0
                                    || nx >= i64::from(width)
                                    || ny >= i64::from(height)
                                {
                                    0
                                } else {
                                    alpha(nx as u32, ny as u32)
                                },
                            );
                        }
                    }
                    center.saturating_sub(interior)
                };
                if let Some((start, value)) = run {
                    if value == coverage {
                        continue;
                    }
                    let key = (start, x, value);
                    let index = if let Some(&index) = previous.get(&key) {
                        rects[index].bounds[3] = y + 1;
                        index
                    } else {
                        rects.push(CoverageRect {
                            bounds: [start, y, x, y + 1],
                            alpha: value,
                        });
                        rects.len() - 1
                    };
                    next.insert(key, index);
                }
                run = (coverage != 0).then_some((x, coverage));
            }
            previous = next;
        }
        Self { size, rects }
    }
}

/// One cropped copy of the source mask in the layout's coordinate system.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskPatch {
    pub bounds: [f32; 4],
    /// Normalized source edges; cropping never creates another outline edge.
    pub uv: [f32; 4],
    pub opacity: u8,
}

/// A row's complete coverage and its requested output resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct MaskLayout {
    pub source: Arc<SpriteMask>,
    pub extent: [f32; 2],
    pub size: [u32; 2],
    pub patches: Vec<MaskPatch>,
}

impl MaskLayout {
    /// Unions source coverage with max, then integrates its area into ordinary white RGBA texels.
    pub fn rasterize(&self) -> Option<Vec<u8>> {
        let [width, height] = self.size;
        if width == 0 || height == 0 || self.extent.iter().any(|v| !v.is_finite() || *v <= 0.) {
            return None;
        }
        let len = (width as usize).checked_mul(height as usize)?;
        let mut pixels = Vec::new();
        pixels.try_reserve_exact(len).ok()?;
        pixels.resize(len, 0f64);
        let mut rects = Vec::new();
        let scale = [
            width as f32 / self.extent[0],
            height as f32 / self.extent[1],
        ];
        for patch in &self.patches {
            let [u0, v0, u1, v1] = patch.uv;
            let [x0, y0, x1, y1] = patch.bounds;
            if patch.opacity == 0 || u1 <= u0 || v1 <= v0 || x1 <= x0 || y1 <= y0 {
                continue;
            }
            for source in &self.source.rects {
                let [left, top, right, bottom] = source.bounds;
                let clip = [
                    (left as f32 / self.source.size[0] as f32).max(u0),
                    (top as f32 / self.source.size[1] as f32).max(v0),
                    (right as f32 / self.source.size[0] as f32).min(u1),
                    (bottom as f32 / self.source.size[1] as f32).min(v1),
                ];
                if clip[2] <= clip[0] || clip[3] <= clip[1] {
                    continue;
                }
                let bounds = [
                    ((x0 + (clip[0] - u0) / (u1 - u0) * (x1 - x0)) * scale[0])
                        .clamp(0., width as f32),
                    ((y0 + (clip[1] - v0) / (v1 - v0) * (y1 - y0)) * scale[1])
                        .clamp(0., height as f32),
                    ((x0 + (clip[2] - u0) / (u1 - u0) * (x1 - x0)) * scale[0])
                        .clamp(0., width as f32),
                    ((y0 + (clip[3] - v0) / (v1 - v0) * (y1 - y0)) * scale[1])
                        .clamp(0., height as f32),
                ];
                if bounds.iter().all(|v| v.is_finite())
                    && bounds[2] > bounds[0]
                    && bounds[3] > bounds[1]
                {
                    rects.push((bounds, u16::from(source.alpha) * u16::from(patch.opacity)));
                }
            }
        }
        let mut events: Vec<_> = rects
            .iter()
            .enumerate()
            .flat_map(|(i, (r, _))| [(r[1], i, true), (r[3], i, false)])
            .collect();
        events.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut active = BTreeSet::new();
        let mut at = 0;
        while at < events.len() {
            let y = events[at].0;
            while at < events.len() && events[at].0 == y {
                let (_, index, start) = events[at];
                if start {
                    active.insert(index);
                } else {
                    active.remove(&index);
                }
                at += 1;
            }
            let Some(next) = events.get(at) else {
                break;
            };
            let mut xs: Vec<_> = active
                .iter()
                .flat_map(|&i| {
                    let (r, alpha) = rects[i];
                    [(r[0], alpha, true), (r[2], alpha, false)]
                })
                .collect();
            xs.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
            let mut coverage = BTreeMap::<u16, usize>::new();
            let mut x_at = 0;
            while x_at < xs.len() {
                let x = xs[x_at].0;
                while x_at < xs.len() && xs[x_at].0 == x {
                    let (_, alpha, start) = xs[x_at];
                    if start {
                        *coverage.entry(alpha).or_default() += 1;
                    } else if let Some(count) = coverage.get_mut(&alpha) {
                        *count -= 1;
                        if *count == 0 {
                            coverage.remove(&alpha);
                        }
                    }
                    x_at += 1;
                }
                if let Some(&(right, _, _)) = xs.get(x_at)
                    && let Some((&alpha, _)) = coverage.last_key_value()
                {
                    accumulate(
                        &mut pixels,
                        width,
                        [x, y, right, next.0],
                        f64::from(alpha) / 255.,
                    );
                }
            }
        }
        Some(
            pixels
                .into_iter()
                .flat_map(|alpha| [255, 255, 255, alpha.round().clamp(0., 255.) as u8])
                .collect(),
        )
    }
}

/// Integrates one disjoint constant-coverage rectangle over the output pixel grid.
fn accumulate(pixels: &mut [f64], width: u32, r: [f32; 4], alpha: f64) {
    for y in r[1].floor() as u32..r[3].ceil() as u32 {
        let h = f64::from((r[3].min(y as f32 + 1.) - r[1].max(y as f32)).max(0.));
        for x in r[0].floor() as u32..r[2].ceil() as u32 {
            let w = f64::from((r[2].min(x as f32 + 1.) - r[0].max(x as f32)).max(0.));
            pixels[(y * width + x) as usize] += alpha * w * h;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unequal_edges_union_before_inversion_or_fading() {
        let source = Arc::new(SpriteMask::outline([9, 9], |x, _| {
            if x == 0 {
                128
            } else if x == 8 {
                255
            } else {
                0
            }
        }));
        for opacity in [110, 255] {
            let layout = MaskLayout {
                source: source.clone(),
                extent: [17., 9.],
                size: [17, 9],
                patches: [[0., 0., 9., 9.], [8., 0., 17., 9.]]
                    .map(|bounds| MaskPatch {
                        bounds,
                        uv: [0., 0., 1., 1.],
                        opacity,
                    })
                    .into(),
            };
            assert_eq!(layout.rasterize().unwrap()[(4 * 17 + 8) * 4 + 3], opacity);
        }
    }

    #[test]
    fn disjoint_source_edges_combine_before_minification() {
        let layout = MaskLayout {
            source: Arc::new(SpriteMask::outline([9, 9], |x, _| {
                if x == 0 || x == 8 { 255 } else { 0 }
            })),
            extent: [17., 9.],
            size: [1, 1],
            patches: [[0., 0., 9., 9.], [8., 0., 17., 9.]]
                .map(|bounds| MaskPatch {
                    bounds,
                    uv: [0., 0., 1., 1.],
                    opacity: 255,
                })
                .into(),
        };
        assert_eq!(
            layout.rasterize().unwrap()[3],
            45,
            "three distinct columns out of seventeen retain their total area"
        );
    }

    #[test]
    fn one_texel_high_resolution_border_survives_area_filtering() {
        let layout = MaskLayout {
            source: Arc::new(SpriteMask::outline([72, 72], |_, _| 255)),
            extent: [72., 72.],
            size: [18, 18],
            patches: vec![MaskPatch {
                bounds: [0., 0., 72., 72.],
                uv: [0., 0., 1., 1.],
                opacity: 255,
            }],
        };
        let rgba = layout.rasterize().unwrap();
        for (x, y) in [(0, 8), (17, 8), (8, 0), (8, 17)] {
            assert_eq!(rgba[(y * 18 + x) * 4 + 3], 64);
        }
        assert_eq!(rgba[(8 * 18 + 8) * 4 + 3], 0);
    }
}
