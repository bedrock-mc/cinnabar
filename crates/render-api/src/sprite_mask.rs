//! Prepared sprite coverage, combined before compositing so every output pixel is painted once.

use std::sync::Arc;

/// Small authored masks stay exact when overlapping copies occupy less than one output pixel.
const MIN_FILTER_SIDE: u32 = 16;

/// Immutable, area-averaged source coverage prepared at its requested draw resolution.
#[derive(Clone, Debug, PartialEq)]
pub struct SpriteMask {
    size: [u32; 2],
    alpha: Vec<f64>,
}

impl SpriteMask {
    /// The stored grid keeps small sources exact and never magnifies source pixels.
    pub fn grid_size(source_size: [u32; 2], draw_size: [u32; 2]) -> [u32; 2] {
        std::array::from_fn(|axis| source_size[axis].min(draw_size[axis].max(MIN_FILTER_SIDE)))
    }

    /// Derives the original alpha outline and area-filters it before paint, ignoring source RGB.
    /// Small authored masks stay exact for overlapping subpixel copies; magnification keeps the source grid.
    pub fn outline(
        source_size: [u32; 2],
        draw_size: [u32; 2],
        alpha: impl Fn(u32, u32) -> u8,
    ) -> Self {
        let size = Self::grid_size(source_size, draw_size);
        let mut coverage = Self {
            size,
            alpha: vec![0.; (size[0] * size[1]) as usize],
        };
        let [width, height] = source_size;
        let scale =
            std::array::from_fn::<_, 2, _>(|axis| size[axis] as f64 / source_size[axis] as f64);
        for y in 0..height {
            for x in 0..width {
                let center = alpha(x, y);
                let mut interior = center;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let (nx, ny) = (i64::from(x) + dx, i64::from(y) + dy);
                        interior = interior.min(
                            if nx < 0 || ny < 0 || nx >= i64::from(width) || ny >= i64::from(height)
                            {
                                0
                            } else {
                                alpha(nx as u32, ny as u32)
                            },
                        );
                    }
                }
                let value = f64::from(center.saturating_sub(interior));
                if value == 0. {
                    continue;
                }
                let bounds = [
                    x as f64 * scale[0],
                    y as f64 * scale[1],
                    (x + 1) as f64 * scale[0],
                    (y + 1) as f64 * scale[1],
                ];
                for py in bounds[1].floor() as u32..(bounds[3].ceil() as u32).min(size[1]) {
                    let h = (bounds[3].min(py as f64 + 1.) - bounds[1].max(py as f64)).max(0.);
                    for px in bounds[0].floor() as u32..(bounds[2].ceil() as u32).min(size[0]) {
                        let w = (bounds[2].min(px as f64 + 1.) - bounds[0].max(px as f64)).max(0.);
                        coverage.alpha[(py * size[0] + px) as usize] += value * w * h;
                    }
                }
            }
        }
        coverage
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
#[derive(Clone, Debug)]
pub struct MaskLayout {
    pub source: Arc<SpriteMask>,
    pub extent: [f32; 2],
    pub size: [u32; 2],
    pub patches: Vec<MaskPatch>,
}

impl PartialEq for MaskLayout {
    /// Cache comparisons read source identity, never the source's full-resolution pixels.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.source, &other.source)
            && self.extent == other.extent
            && self.size == other.size
            && self.patches == other.patches
    }
}

struct PreparedPatch<'a> {
    mask: &'a SpriteMask,
    bounds: [f64; 4],
    origin: [f64; 2],
    step: [f64; 2],
    opacity: f64,
}

impl<'a> PreparedPatch<'a> {
    /// Maps a cropped source onto output texels and chooses its already filtered coverage.
    fn new(layout: &'a MaskLayout, patch: &MaskPatch) -> Option<Self> {
        if patch.opacity == 0
            || patch
                .bounds
                .iter()
                .chain(&patch.uv)
                .any(|value| !value.is_finite())
        {
            return None;
        }
        let scale = std::array::from_fn::<_, 2, _>(|axis| {
            layout.size[axis] as f64 / f64::from(layout.extent[axis])
        });
        let dest =
            std::array::from_fn::<_, 4, _>(|edge| f64::from(patch.bounds[edge]) * scale[edge % 2]);
        let uv = patch.uv.map(f64::from);
        if dest[2] <= dest[0] || dest[3] <= dest[1] || uv[2] <= uv[0] || uv[3] <= uv[1] {
            return None;
        }
        let extent = std::array::from_fn::<_, 2, _>(|axis| {
            (dest[axis + 2] - dest[axis]) / (uv[axis + 2] - uv[axis])
        });
        let mask = &layout.source;
        if mask.size.contains(&0) {
            return None;
        }
        let origin = std::array::from_fn(|axis| dest[axis] - uv[axis] * extent[axis]);
        let bounds = [
            dest[0].max(origin[0]).max(0.),
            dest[1].max(origin[1]).max(0.),
            dest[2]
                .min(origin[0] + extent[0])
                .min(layout.size[0] as f64),
            dest[3]
                .min(origin[1] + extent[1])
                .min(layout.size[1] as f64),
        ];
        (bounds[2] > bounds[0] && bounds[3] > bounds[1]).then(|| Self {
            mask,
            bounds,
            origin,
            step: std::array::from_fn(|axis| extent[axis] / mask.size[axis] as f64),
            opacity: f64::from(patch.opacity) / 255.,
        })
    }

    /// Adds only the small prepared grid's edges crossing this output pixel.
    fn cuts(&self, axis: usize, pixel: f64, cuts: &mut Vec<f64>) {
        let lo = self.bounds[axis].max(pixel);
        let hi = self.bounds[axis + 2].min(pixel + 1.);
        cuts.extend([lo, hi]);
        let first = ((lo - self.origin[axis]) / self.step[axis]).floor() as u32 + 1;
        let end =
            (((hi - self.origin[axis]) / self.step[axis]).ceil() as u32).min(self.mask.size[axis]);
        for edge in first..end {
            let position = self.origin[axis] + edge as f64 * self.step[axis];
            if position > lo && position < hi {
                cuts.push(position);
            }
        }
    }

    /// Reads one prepared cell, with transparent coverage outside this cropped copy.
    fn sample(&self, x: f64, y: f64) -> f64 {
        if x < self.bounds[0] || y < self.bounds[1] || x >= self.bounds[2] || y >= self.bounds[3] {
            return 0.;
        }
        let cell: [u32; 2] = std::array::from_fn(|axis| {
            ((([x, y][axis] - self.origin[axis]) / self.step[axis]).floor() as u32)
                .min(self.mask.size[axis] - 1)
        });
        self.mask.alpha[(cell[1] * self.mask.size[0] + cell[0]) as usize] * self.opacity
    }
}

impl MaskLayout {
    /// Integrates max coverage on small prepared grids, with paint cost bounded by output pixels.
    pub fn rasterize(&self) -> Option<Vec<u8>> {
        self.rasterize_rect([0, 0, self.size[0], self.size[1]])
    }

    /// Rasterizes only these output texels, preserving the full layout's coverage coordinates.
    pub fn rasterize_rect(&self, [left, top, right, bottom]: [u32; 4]) -> Option<Vec<u8>> {
        if right > self.size[0]
            || bottom > self.size[1]
            || right <= left
            || bottom <= top
            || self.extent.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return None;
        }
        let [width, height] = [right - left, bottom - top];
        let len = (width as usize)
            .checked_mul(height as usize)?
            .checked_mul(4)?;
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(len).ok()?;
        rgba.resize(len, 255);
        let mut patches = Vec::with_capacity(self.patches.len());
        patches.extend(
            self.patches
                .iter()
                .filter_map(|patch| PreparedPatch::new(self, patch)),
        );
        let mut active = Vec::with_capacity(patches.len());
        let capacity = 2 + patches.len() * (MIN_FILTER_SIDE as usize + 3);
        let (mut xs, mut ys) = (Vec::with_capacity(capacity), Vec::with_capacity(capacity));
        for y in top..bottom {
            for x in left..right {
                let (px, py) = (x as f64, y as f64);
                active.clear();
                active.extend(patches.iter().filter(|patch| {
                    patch.bounds[0] < px + 1.
                        && patch.bounds[2] > px
                        && patch.bounds[1] < py + 1.
                        && patch.bounds[3] > py
                }));
                xs.clear();
                ys.clear();
                xs.extend([px, px + 1.]);
                ys.extend([py, py + 1.]);
                for patch in &active {
                    patch.cuts(0, px, &mut xs);
                    patch.cuts(1, py, &mut ys);
                }
                xs.sort_unstable_by(f64::total_cmp);
                ys.sort_unstable_by(f64::total_cmp);
                xs.dedup();
                ys.dedup();
                let mut alpha = 0.;
                for y in ys.windows(2) {
                    for x in xs.windows(2) {
                        let coverage = active
                            .iter()
                            .map(|patch| patch.sample((x[0] + x[1]) * 0.5, (y[0] + y[1]) * 0.5))
                            .fold(0., f64::max);
                        alpha += coverage * (x[1] - x[0]) * (y[1] - y[0]);
                    }
                }
                rgba[(((y - top) * width + x - left) * 4 + 3) as usize] =
                    alpha.round().clamp(0., 255.) as u8;
            }
        }
        Some(rgba)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unequal_edges_union_before_inversion_or_fading() {
        let source = Arc::new(SpriteMask::outline([9, 9], [9, 9], |x, _| {
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
            source: Arc::new(SpriteMask::outline([9, 9], [9, 9], |x, _| {
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
            source: Arc::new(SpriteMask::outline([72, 72], [18, 18], |_, _| 255)),
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
