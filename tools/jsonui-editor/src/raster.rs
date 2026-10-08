//! A small CPU rasterizer for the client's [`UiDrawList`]: textured, tinted
//! triangles with nearest sampling, clipped per batch, alpha or invert blended.
//! Axis-aligned quads (nearly everything) take a direct span fill.

use std::sync::Arc;

use ui::{UiBlendMode, UiDrawBatch, UiDrawList, UiVertex};

/// An RGBA8 (straight alpha) image a batch samples, in texel coordinates.
#[derive(Clone, Debug)]
pub struct Page {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

impl Page {
    /// A single opaque white texel, the page solids sample.
    pub fn white() -> Self {
        Self {
            width: 1,
            height: 1,
            rgba: Arc::from(vec![255u8; 4]),
        }
    }

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
            if is_quad && axis_aligned(&corners) {
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
        for y in top..bottom {
            let fy = (y as f32 + 0.5 - y0) / h;
            let tv = v0 + (v1 - v0) * fy;
            let row = y as usize * self.width as usize;
            for x in left..right {
                let fx = (x as f32 + 0.5 - x0) / w;
                let texel = texture.sample(u0 + (u1 - u0) * fx, tv);
                blend_into(&mut self.pixels[row + x as usize], texel, color, blend);
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
                let texel = texture.sample(u, t);
                let at = y as usize * self.width as usize + x as usize;
                blend_into(&mut self.pixels[at], texel, v[0].color, blend);
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

fn axis_aligned(v: &[&UiVertex; 4]) -> bool {
    let p = v.map(|vertex| vertex.position);
    p[0][1] == p[1][1] && p[1][0] == p[2][0] && p[2][1] == p[3][1] && p[3][0] == p[0][0]
}

/// The pixel columns (or rows) whose centres fall in `[low, high)` within the clip.
fn span(low: f32, high: f32, clip_low: f32, clip_high: f32) -> [i64; 2] {
    let low = low.max(clip_low);
    let high = high.min(clip_high);
    [(low - 0.5).ceil() as i64, (high - 0.5).ceil() as i64]
}

/// Weights of `point` inside triangle `p` (edges included), or `None` outside.
fn barycentric(p: [[f32; 2]; 3], point: [f32; 2]) -> Option<[f32; 3]> {
    let area = edge(p[0], p[1], p[2]);
    if area == 0.0 {
        return None;
    }
    let w = [
        edge(p[1], p[2], point) / area,
        edge(p[2], p[0], point) / area,
        edge(p[0], p[1], point) / area,
    ];
    w.iter().all(|weight| *weight >= 0.0).then_some(w)
}

fn edge(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn mul(a: u8, b: u8) -> u32 {
    (u32::from(a) * u32::from(b) + 127) / 255
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ui::{UiNode, UiNodeId, UiPoint, UiRect, UiTree, UiVisual};

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> UiRect {
        UiRect::new(UiPoint::new(x0, y0).unwrap(), UiPoint::new(x1, y1).unwrap()).unwrap()
    }

    // A half-covered translucent solid blends once per pixel, including the diagonal.
    #[test]
    fn solids_fill_their_pixel_centres_exactly_once() {
        let node = UiNode::new(UiNodeId::new(1), None, rect(1.0, 1.0, 3.0, 3.0)).with_visual(
            UiVisual::Solid {
                texture_page: 0,
                color: [255, 0, 0, 128],
            },
        );
        let list = UiTree::new(vec![node]).unwrap().build_draw_list().unwrap();
        let white = Page::white();
        let mut canvas = Canvas::new(4, 4);
        canvas.draw(&list, |_| Some(&white));
        let rgba = canvas.into_rgba();
        let pixel = |x: usize, y: usize| &rgba[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4];
        assert_eq!(pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(pixel(1, 1), [255, 0, 0, 128]);
        assert_eq!(pixel(2, 2), [255, 0, 0, 128]);
        assert_eq!(pixel(3, 3), [0, 0, 0, 0]);
    }
}
