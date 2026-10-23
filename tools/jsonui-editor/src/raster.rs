//! A small CPU rasterizer for the client's [`UiDrawList`]: textured, tinted
//! triangles with sprite filtering and font coverage, clipped per batch, alpha or invert blended.
//! Axis-aligned quads (nearly everything) take a direct span fill.

#[cfg(test)]
mod mask_tests;

use std::sync::Arc;

use ui::{UiBlendMode, UiDrawBatch, UiDrawList, UiVertex};

/// Includes model alpha testing in the shading flags, matching the GPU render adapter.
fn style_flags(vertex: &UiVertex) -> u16 {
    vertex.style_flags
        | if vertex.alpha_test {
            render_model::UI_STYLE_ALPHA_TEST as u16
        } else {
            0
        }
}

/// Applies the particle source bounds after triangle interpolation, as the GPU shader does.
fn particle_uv(uv: [f32; 2], vertex: &UiVertex) -> [f32; 2] {
    if u32::from(vertex.style_flags) & render_model::UI_STYLE_PARTICLE != 0 {
        render_model::ui_particle_uv(uv, vertex.overlay_color)
    } else {
        uv
    }
}

/// An RGBA8 (straight alpha) image a batch samples, in texel coordinates.
#[derive(Clone, Debug)]
pub struct Page {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    /// Non-overlapping ownership inside an original-image page's filtering halo.
    pub repeat_content: Option<[u32; 4]>,
}

impl Page {
    /// A single opaque white texel, the page solids sample.
    pub fn white() -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: Arc::from(vec![255u8; 4]),
            repeat_content: None,
        }
    }

    /// Samples the nearest texel, clamping at page edges.
    fn sample(&self, u: f32, v: f32) -> [u8; 4] {
        let x = (u.floor() as i64).clamp(0, i64::from(self.width) - 1) as usize;
        let y = (v.floor() as i64).clamp(0, i64::from(self.height) - 1) as usize;
        let at = (y * self.width as usize + x) * 4;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }

    /// Filters around texel centres without rounding intermediate coverage values.
    fn sample_linear(&self, u: f32, v: f32) -> [f32; 4] {
        let x = (u - 0.5).floor();
        let y = (v - 0.5).floor();
        let fx = u - 0.5 - x;
        let fy = v - 0.5 - y;
        let samples = [
            self.sample(x, y),
            self.sample(x + 1.0, y),
            self.sample(x, y + 1.0),
            self.sample(x + 1.0, y + 1.0),
        ];
        std::array::from_fn(|channel| {
            let top =
                f32::from(samples[0][channel]) * (1.0 - fx) + f32::from(samples[1][channel]) * fx;
            let bottom =
                f32::from(samples[2][channel]) * (1.0 - fx) + f32::from(samples[3][channel]) * fx;
            (top * (1.0 - fy) + bottom * fy) / 255.0
        })
    }

    /// Integrates outline coverage across a pixel with the production shader's bounded grid.
    fn sample_outline(&self, uv: [f32; 2], derivatives: [[f32; 2]; 2]) -> [f32; 4] {
        use render_model::font_shading::{outline_filter_grid, outline_filter_offset};
        let [nx, ny] = outline_filter_grid(derivatives);
        let mut total = [0.; 4];
        for y in 0..ny {
            for x in 0..nx {
                let ox = outline_filter_offset(x, nx);
                let oy = outline_filter_offset(y, ny);
                let point: [f32; 2] = std::array::from_fn(|axis| {
                    uv[axis] + derivatives[0][axis] * ox + derivatives[1][axis] * oy
                });
                let sample = self.sample_linear(point[0], point[1]);
                for channel in 0..4 {
                    total[channel] += sample[channel];
                }
            }
        }
        total.map(|value| value / (nx * ny) as f32)
    }
}

/// A premultiplied RGBA8 target.
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pixels: Vec<[u8; 4]>,
}

impl Canvas {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixels: vec![[0; 4]; width as usize * height as usize],
        }
    }

    /// Draw every batch of `list`, sampling pages through `page`.
    pub fn draw<'p>(&mut self, list: &UiDrawList, page: impl Fn(u16) -> Option<&'p Page>) {
        for batch in &list.batches {
            let Some(texture) = page(batch.texture_page) else {
                continue;
            };
            self.batch(list, batch, texture);
        }
    }

    fn batch(&mut self, list: &UiDrawList, batch: &UiDrawBatch, texture: &Page) {
        let clip = [
            batch.clip.min().x().max(0.0),
            batch.clip.min().y().max(0.0),
            batch.clip.max().x().min(self.width as f32),
            batch.clip.max().y().min(self.height as f32),
        ];
        if clip[2] <= clip[0] || clip[3] <= clip[1] {
            return;
        }
        let range = batch.index_range.start as usize..batch.index_range.end as usize;
        let indices = &list.indices[range];
        let mut at = 0;
        while at + 6 <= indices.len() {
            let quad = &indices[at..at + 6];
            let corners = [quad[0], quad[1], quad[2], quad[5]].map(|i| &list.vertices[i as usize]);
            let is_quad = quad[3] == quad[0] && quad[4] == quad[2];
            if is_quad && axis_aligned(&corners) && flat_texture(&corners) {
                self.rect(&corners, clip, texture, batch.blend);
            } else if is_quad {
                self.polygon(
                    &[[quad[0], quad[1], quad[2]], [quad[0], quad[2], quad[5]]],
                    list,
                    clip,
                    texture,
                    batch.blend,
                );
            } else {
                self.polygon(
                    &[[quad[0], quad[1], quad[2]]],
                    list,
                    clip,
                    texture,
                    batch.blend,
                );
                self.polygon(
                    &[[quad[3], quad[4], quad[5]]],
                    list,
                    clip,
                    texture,
                    batch.blend,
                );
            }
            at += 6;
        }
        if let [a, b, c] = indices[at..] {
            self.polygon(&[[a, b, c]], list, clip, texture, batch.blend);
        }
    }

    fn rect(&mut self, v: &[&UiVertex; 4], clip: [f32; 4], texture: &Page, blend: UiBlendMode) {
        let (x0, y0) = (v[0].position[0], v[0].position[1]);
        let (x1, y1) = (v[2].position[0], v[2].position[1]);
        let (w, h) = (x1 - x0, y1 - y0);
        if w == 0.0 || h == 0.0 {
            return;
        }
        let (u0, v0) = (v[0].uv[0], v[0].uv[1]);
        let (u1, v1) = (v[2].uv[0], v[2].uv[1]);
        let [left, right] = span(x0.min(x1), x0.max(x1), clip[0], clip[2]);
        let [top, bottom] = span(y0.min(y1), y0.max(y1), clip[1], clip[3]);
        let color = v[0].color;
        let flags = style_flags(v[0]);
        let derivatives = [[(u1 - u0) / w, 0.], [0., (v1 - v0) / h]];
        for y in top..bottom {
            let fy = (y as f32 + 0.5 - y0) / h;
            let tv = v0 + (v1 - v0) * fy;
            let row = y as usize * self.width as usize;
            for x in left..right {
                let fx = (x as f32 + 0.5 - x0) / w;
                let texel = shaded_sample(
                    texture,
                    particle_uv([u0 + (u1 - u0) * fx, tv], v[0]),
                    color,
                    flags,
                    derivatives,
                    Some(v[0].overlay_color),
                );
                blend_styled(
                    &mut self.pixels[row + x as usize],
                    texel,
                    color,
                    blend,
                    flags,
                );
            }
        }
    }

    /// Fill the union of `triangles`, blending each covered pixel once.
    fn polygon(
        &mut self,
        triangles: &[[u32; 3]],
        list: &UiDrawList,
        clip: [f32; 4],
        texture: &Page,
        blend: UiBlendMode,
    ) {
        let points: Vec<[f32; 2]> = triangles
            .iter()
            .flatten()
            .map(|i| list.vertices[*i as usize].position)
            .collect();
        let min_x = points.iter().map(|q| q[0]).fold(f32::MAX, f32::min);
        let max_x = points.iter().map(|q| q[0]).fold(f32::MIN, f32::max);
        let min_y = points.iter().map(|q| q[1]).fold(f32::MAX, f32::min);
        let max_y = points.iter().map(|q| q[1]).fold(f32::MIN, f32::max);
        let [left, right] = span(min_x, max_x, clip[0], clip[2]);
        let [top, bottom] = span(min_y, max_y, clip[1], clip[3]);
        for y in top..bottom {
            for x in left..right {
                let point = [x as f32 + 0.5, y as f32 + 0.5];
                let Some((v, w)) = triangles.iter().find_map(|triangle| {
                    let v = triangle.map(|i| &list.vertices[i as usize]);
                    barycentric(v.map(|vertex| vertex.position), point).map(|w| (v, w))
                }) else {
                    continue;
                };
                let u = (0..3).map(|k| w[k] * v[k].uv[0]).sum();
                let t = (0..3).map(|k| w[k] * v[k].uv[1]).sum();
                let color = std::array::from_fn(|channel| {
                    (0..3)
                        .map(|k| w[k] * f32::from(v[k].color[channel]))
                        .sum::<f32>()
                        .round() as u8
                });
                let flags = style_flags(v[0]);
                let (texel, tint) = if flags & render_api::ui_shadow::UI_STYLE_SHADOW != 0 {
                    let shape = std::array::from_fn(|channel| {
                        (0..3).map(|k| w[k] * v[k].overlay_color[channel]).sum()
                    });
                    let [dx, dy] = texel_derivatives(&v);
                    let pixel = dx[0].hypot(dx[1]).max(dy[0].hypot(dy[1]));
                    let area = render_api::ui_shadow::coverage([u, t], shape, pixel);
                    (straight([area; 4]), color)
                } else if flags & render_api::UI_STYLE_COVERAGE_PAINT != 0 {
                    let overlay = std::array::from_fn(|channel| {
                        (0..3).map(|k| w[k] * v[k].overlay_color[channel]).sum()
                    });
                    let border = (0..3).map(|k| w[k] * v[k].model_light).sum();
                    let paint = render_api::UiCoveragePaint::from_vertex_colors(color, overlay);
                    (straight(paint.sample(u, [t, border])), [255; 4])
                } else if u32::from(flags) & render_model::UI_STYLE_RADIAL_GRADIENT != 0 {
                    let radius = u.hypot(t).clamp(0.0, 1.0);
                    let outer: [f32; 4] = std::array::from_fn(|channel| {
                        (0..3).map(|k| w[k] * v[k].overlay_color[channel]).sum()
                    });
                    let alpha = f32::from(color[3]) / 255.0;
                    let mixed = std::array::from_fn(|channel| {
                        let inner = if channel == 3 {
                            alpha
                        } else {
                            f32::from(color[channel]) / 255.0 * alpha
                        };
                        let outside = if channel == 3 {
                            outer[3]
                        } else {
                            outer[channel] * outer[3]
                        };
                        inner * (1.0 - radius) + outside * radius
                    });
                    (straight(mixed), [255; 4])
                } else {
                    let derivatives = texel_derivatives(&v);
                    (
                        shaded_sample(
                            texture,
                            particle_uv([u, t], v[0]),
                            color,
                            flags,
                            derivatives,
                            Some(v[0].overlay_color),
                        ),
                        color,
                    )
                };
                let at = y as usize * self.width as usize + x as usize;
                blend_styled(&mut self.pixels[at], texel, tint, blend, flags);
            }
        }
    }

    /// The image as straight-alpha RGBA8 rows.
    pub fn into_rgba(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.pixels.len() * 4);
        for [r, g, b, a] in self.pixels {
            if a == 0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let straight =
                |c: u8| ((u32::from(c) * 255 + u32::from(a) / 2) / u32::from(a)).min(255) as u8;
            out.extend_from_slice(&[straight(r), straight(g), straight(b), a]);
        }
        out
    }
}

/// Whether four corners form a window-aligned rectangle.
fn axis_aligned(v: &[&UiVertex; 4]) -> bool {
    let p = v.map(|vertex| vertex.position);
    p[0][1] == p[1][1] && p[1][0] == p[2][0] && p[2][1] == p[3][1] && p[3][0] == p[0][0]
}

/// The span fast path only handles uniformly tinted ordinary texture quads.
fn flat_texture(v: &[&UiVertex; 4]) -> bool {
    v[0].style_flags & render_api::UI_STYLE_COVERAGE_PAINT == 0
        && v[0].style_flags & render_api::ui_shadow::UI_STYLE_SHADOW == 0
        && u32::from(v[0].style_flags) & render_model::UI_STYLE_RADIAL_GRADIENT == 0
        && v.iter().all(|vertex| vertex.color == v[0].color)
        && v[0].uv[1] == v[1].uv[1]
        && v[1].uv[0] == v[2].uv[0]
        && v[2].uv[1] == v[3].uv[1]
        && v[3].uv[0] == v[0].uv[0]
}

/// Matches the shader's screen-space UV derivatives for an affine triangle.
fn texel_derivatives(v: &[&UiVertex; 3]) -> [[f32; 2]; 2] {
    let position = v.map(|vertex| vertex.position);
    let uv = v.map(|vertex| vertex.uv);
    let area = edge(position[0], position[1], position[2]);
    let dp: [[f32; 2]; 2] =
        [1, 2].map(|i| std::array::from_fn(|axis| position[i][axis] - position[0][axis]));
    let duv: [[f32; 2]; 2] = [1, 2].map(|i| std::array::from_fn(|axis| uv[i][axis] - uv[0][axis]));
    [
        std::array::from_fn(|axis| (duv[0][axis] * dp[1][1] - duv[1][axis] * dp[0][1]) / area),
        std::array::from_fn(|axis| (duv[1][axis] * dp[0][0] - duv[0][axis] * dp[1][0]) / area),
    ]
}

/// Applies the UI shader's sampler selection, SDF smoothing and native text gamma.
fn shaded_sample(
    texture: &Page,
    uv: [f32; 2],
    color: [u8; 4],
    flags: u16,
    derivatives: [[f32; 2]; 2],
    bounds: Option<[f32; 4]>,
) -> [u8; 4] {
    let native = flags & u16::from(assets::FONT_STYLE_COVERAGE_GAMMA) != 0;
    let sdf = flags & u16::from(assets::FONT_STYLE_SDF) != 0;
    let linear = sdf || (!native && flags & u16::from(ui::UI_STYLE_BILINEAR) != 0);
    // Repeat coordinates must be wrapped before sampling, including on mixed-style vertices.
    let mut sample = if flags & render_api::ui_repeat::UI_STYLE_REPEAT != 0 {
        repeated_sample(texture, uv, bounds.unwrap_or_default(), flags, linear)
    } else if flags & assets::FONT_STYLE_OUTLINE_COVERAGE != 0 {
        texture.sample_outline(uv, derivatives)
    } else if linear {
        texture.sample_linear(uv[0], uv[1])
    } else {
        texture
            .sample(uv[0], uv[1])
            .map(|channel| f32::from(channel) / 255.0)
    };
    if sdf {
        let [dx, dy] = derivatives;
        let scale = (dx[0] * dy[1] - dx[1] * dy[0]).abs().sqrt();
        sample = [
            1.0,
            1.0,
            1.0,
            render_model::font_shading::sdf_coverage(sample[3], scale),
        ];
    }
    if native {
        let rgb = [0, 1, 2].map(|channel| f32::from(color[channel]) / 255.0);
        sample = [
            1.0,
            1.0,
            1.0,
            render_model::font_shading::coverage_gamma(sample[3], rgb),
        ];
    }
    if flags & u16::from(ui::UI_STYLE_GRAYSCALE) != 0 {
        let luma = sample[0] * 0.299 + sample[1] * 0.587 + sample[2] * 0.114;
        sample = [luma, luma, luma, sample[3]];
    }
    let texel = sample.map(|channel| (channel * 255.0).round() as u8);
    if flags & render_api::UI_STYLE_SPRITE_COLOR_MASK != 0 {
        let dye = bounds.unwrap_or_default();
        assets::color_mask_texel(
            texel,
            [dye[0], dye[1], dye[2]].map(|channel| (channel * 255.0).round() as u8),
        )
    } else {
        texel
    }
}

/// Wraps tile coordinates and keeps filtering inside the source region or page halo.
fn repeated_sample(
    texture: &Page,
    uv: [f32; 2],
    bounds: [f32; 4],
    flags: u16,
    linear: bool,
) -> [f32; 4] {
    let Some(uv) = render_api::ui_repeat::decode_uv(
        uv,
        bounds,
        flags,
        [texture.width, texture.height],
        texture.repeat_content,
    ) else {
        return [0.0; 4];
    };
    let sample = |uv: [f32; 2]| texture.sample(uv[0], uv[1]).map(|c| f32::from(c) / 255.0);
    if !linear {
        return sample(uv);
    }
    texture.sample_linear(uv[0], uv[1])
}

/// Converts a premultiplied coverage mixture to the rasterizer's straight-alpha input.
fn straight(color: [f32; 4]) -> [u8; 4] {
    let alpha = color[3];
    if alpha <= 0.0 {
        return [0; 4];
    }
    std::array::from_fn(|channel| {
        (if channel == 3 {
            alpha
        } else {
            color[channel] / alpha
        } * 255.0)
            .round() as u8
    })
}

/// The pixel columns (or rows) whose centres fall in `[low, high)` within the clip.
fn span(low: f32, high: f32, clip_low: f32, clip_high: f32) -> [i64; 2] {
    let low = low.max(clip_low);
    let high = high.min(clip_high);
    [(low - 0.5).ceil() as i64, (high - 0.5).ceil() as i64]
}

/// Weights of `point` inside triangle `p`, or `None` outside. Like the GPU's top-left rule, a
/// point on an edge belongs only to the triangle right of it (below it when horizontal), so
/// triangles sharing an edge blend it once.
fn barycentric(p: [[f32; 2]; 3], point: [f32; 2]) -> Option<[f32; 3]> {
    let area = edge(p[0], p[1], p[2]);
    if area == 0.0 {
        return None;
    }
    let mut w = [0.0; 3];
    for (k, weight) in w.iter_mut().enumerate() {
        let (a, b) = (p[(k + 1) % 3], p[(k + 2) % 3]);
        *weight = shared_edge(a, b, point) / area;
        let inward = [a[1] - b[1], b[0] - a[0]].map(|axis| axis * area.signum());
        let top_left = inward[0] > 0.0 || (inward[0] == 0.0 && inward[1] > 0.0);
        if *weight < 0.0 || (*weight == 0.0 && !top_left) {
            return None;
        }
    }
    Some(w)
}

fn edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// [`edge`] evaluated from a fixed endpoint order, so both triangles sharing `a`-`b` see exactly
/// opposite values and no point is rounded onto both sides or neither.
fn shared_edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    if (a[0], a[1]) <= (b[0], b[1]) {
        edge(a, b, c)
    } else {
        -edge(b, a, c)
    }
}

fn mul(a: u8, b: u8) -> u32 {
    (u32::from(a) * u32::from(b) + 127) / 255
}

/// Adds particle light without changing coverage; other styles retain ordinary compositing.
fn blend_styled(
    dst: &mut [u8; 4],
    mut texel: [u8; 4],
    tint: [u8; 4],
    blend: UiBlendMode,
    flags: u16,
) {
    if u32::from(flags) & render_model::UI_STYLE_PARTICLE != 0 {
        if u32::from(flags) & render_model::UI_STYLE_ALPHA_TEST != 0 {
            if texel[3] < 128 {
                return;
            }
            texel[3] = 255;
        }
        if mul(texel[3], tint[3]) < 3 {
            return;
        }
    }
    if u32::from(flags) & render_model::UI_STYLE_ADDITIVE != 0 {
        let alpha = mul(texel[3], tint[3]) as u8;
        for c in 0..3 {
            dst[c] = (u32::from(dst[c]) + mul(mul(texel[c], tint[c]) as u8, alpha)).min(255) as u8;
        }
    } else {
        blend_into(dst, texel, tint, blend);
    }
}

fn blend_into(dst: &mut [u8; 4], texel: [u8; 4], tint: [u8; 4], blend: UiBlendMode) {
    let alpha = mul(texel[3], tint[3]);
    if alpha == 0 {
        return;
    }
    let src = [0, 1, 2].map(|c| mul(mul(texel[c], tint[c]) as u8, alpha as u8));
    match blend {
        UiBlendMode::Alpha => {
            let keep = 255 - alpha;
            for c in 0..3 {
                dst[c] = (src[c] + mul(dst[c], keep as u8)).min(255) as u8;
            }
            dst[3] = (alpha + mul(dst[3], keep as u8)).min(255) as u8;
        }
        UiBlendMode::Invert => {
            for c in 0..3 {
                let s = src[c];
                let d = u32::from(dst[c]);
                dst[c] = ((s * (255 - d) + d * (255 - s)) / 255).min(255) as u8;
            }
            dst[3] = dst[3].max(alpha as u8);
        }
        // The editor has no 3-D scene for a backdrop to process, and draws filtered groups
        // unblurred.
        UiBlendMode::Backdrop | UiBlendMode::Filter => {}
    }
}

#[cfg(test)]
mod tests;
