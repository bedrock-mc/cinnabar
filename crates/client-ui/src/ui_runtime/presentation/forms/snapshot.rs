//! Local-only visual check: rasterizes a presentation frame's UI draw input on
//! the CPU (nearest sampling, alpha blending, scissors) into a PNG, so form
//! layouts can be inspected without a window. Written only when
//! `CINNABAR_FORM_SNAPSHOT_DIR` names a directory; images are never committed.

use std::path::Path;

use image::{Rgba, RgbaImage};
use render_model::{UI_BLEND_INVERT, UiRenderInput, UiRenderVertex};

const SNAPSHOT_ENV: &str = "CINNABAR_FORM_SNAPSHOT_DIR";

/// Composes a known pack texel with the loading frame's published backdrop tint.
pub fn loading_backdrop_texel(
    presentation: &super::super::UiPresentationRuntime,
    source: [u8; 4],
    at: [u32; 2],
) -> [u8; 4] {
    let node = presentation
        .last_frame
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .rev()
        .find(|node| {
            matches!(node.visual(), ui::UiVisual::Gradient { colors, .. }
            if colors.iter().all(|color| color[3] >= 200))
        })
        .expect("world loading publishes its backdrop tint");
    let ui::UiVisual::Gradient { colors, .. } = node.visual() else {
        unreachable!()
    };
    let bounds = node.bounds();
    let t = ((at[1] as f32 + 0.5 - bounds.min().y()) / (bounds.max().y() - bounds.min().y()))
        .clamp(0.0, 1.0);
    let tint: [u8; 4] = std::array::from_fn(|channel| {
        (f32::from(colors[0][channel]) * (1.0 - t) + f32::from(colors[1][channel]) * t).round()
            as u8
    });
    let alpha = f32::from(tint[3]) / 255.0;
    std::array::from_fn(|channel| {
        if channel == 3 {
            255
        } else {
            (f32::from(tint[channel]) * alpha + f32::from(source[channel]) * (1.0 - alpha)).round()
                as u8
        }
    })
}

/// The frame composited over a mid-grey backdrop.
pub fn rasterize(input: &UiRenderInput) -> RgbaImage {
    let [width, height] = input.viewport_size;
    let mut image = RgbaImage::from_pixel(width, height, Rgba([70, 90, 110, 255]));
    let pages = input.textures.pages();
    for batch in input.batches.iter() {
        let Some(page) = pages.get(batch.texture_page as usize) else {
            continue;
        };
        let [page_width, page_height] = page.dimensions();
        let pixels = page.pixels();
        let scissor = batch.scissor;
        let indices = &input.indices
            [batch.first_index as usize..(batch.first_index + batch.index_count) as usize];
        for triangle in indices.chunks_exact(3) {
            let corners: [UiRenderVertex; 3] =
                std::array::from_fn(|corner| input.vertices[triangle[corner] as usize]);
            fill(
                &mut image,
                corners,
                |[u, v], color, x, y| {
                    if x < scissor.x
                        || y < scissor.y
                        || x >= scissor.x + scissor.width
                        || y >= scissor.y + scissor.height
                    {
                        return None;
                    }
                    let (u, v) = (
                        (u.floor() as u32).min(page_width - 1),
                        (v.floor() as u32).min(page_height - 1),
                    );
                    let at = ((v * page_width + u) * 4) as usize;
                    let mut texel: [u8; 4] = pixels[at..at + 4].try_into().unwrap();
                    if u32::from(ui::UI_STYLE_GRAYSCALE) & corners[0].style_flags != 0 {
                        let luma = (0.299 * f32::from(texel[0])
                            + 0.587 * f32::from(texel[1])
                            + 0.114 * f32::from(texel[2]))
                        .round() as u8;
                        texel = [luma, luma, luma, texel[3]];
                    }
                    Some(std::array::from_fn(|channel| {
                        (u16::from(texel[channel]) * u16::from(color[channel]) / 255) as u8
                    }))
                },
                batch.blend_mode == UI_BLEND_INVERT,
            );
        }
    }
    image
}

/// Fill one triangle, sampling `shade(uv, color, x, y)` at each covered pixel
/// centre and blending the result over the image. A centre on an edge belongs
/// only to the triangle that edge is a top or left edge of, as GPUs rasterize,
/// so a quad's shared diagonal is never blended twice.
fn fill(
    image: &mut RgbaImage,
    mut corners: [UiRenderVertex; 3],
    shade: impl Fn([f32; 2], [u8; 4], u32, u32) -> Option<[u8; 4]>,
    invert: bool,
) {
    let [a, b, c] = corners.map(|corner| corner.position);
    let mut area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
    if area.abs() < f32::EPSILON {
        return;
    }
    if area < 0.0 {
        corners.swap(1, 2);
        area = -area;
    }
    let [a, b, c] = corners.map(|corner| corner.position);
    let top_left = |from: [f32; 2], to: [f32; 2]| {
        let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
        dy < 0.0 || (dy == 0.0 && dx > 0.0)
    };
    let owns = |weight: f32, from: [f32; 2], to: [f32; 2]| {
        weight > 0.0 || (weight == 0.0 && top_left(from, to))
    };
    let min_x = a[0].min(b[0]).min(c[0]).floor().max(0.0) as u32;
    let min_y = a[1].min(b[1]).min(c[1]).floor().max(0.0) as u32;
    let max_x = (a[0].max(b[0]).max(c[0]).ceil() as u32).min(image.width());
    let max_y = (a[1].max(b[1]).max(c[1]).ceil() as u32).min(image.height());
    for y in min_y..max_y {
        for x in min_x..max_x {
            let p = [x as f32 + 0.5, y as f32 + 0.5];
            let weight = |from: [f32; 2], to: [f32; 2]| {
                ((to[0] - from[0]) * (p[1] - from[1]) - (to[1] - from[1]) * (p[0] - from[0])) / area
            };
            let (wa, wb, wc) = (weight(b, c), weight(c, a), weight(a, b));
            if !(owns(wa, b, c) && owns(wb, c, a) && owns(wc, a, b)) {
                continue;
            }
            let uv = std::array::from_fn(|axis| {
                wa * corners[0].uv[axis] + wb * corners[1].uv[axis] + wc * corners[2].uv[axis]
            });
            let color = std::array::from_fn(|channel| {
                (wa * f32::from(corners[0].color[channel])
                    + wb * f32::from(corners[1].color[channel])
                    + wc * f32::from(corners[2].color[channel]))
                .round() as u8
            });
            let Some(source) = shade(uv, color, x, y) else {
                continue;
            };
            let target = image.get_pixel_mut(x, y);
            let alpha = f32::from(source[3]) / 255.0;
            for channel in 0..3 {
                let over = if invert {
                    255 - target[channel]
                } else {
                    source[channel]
                };
                target[channel] = (f32::from(over) * alpha
                    + f32::from(target[channel]) * (1.0 - alpha))
                    .round() as u8;
            }
        }
    }
}

/// Write `input` as `<dir>/<name>.png` when the snapshot directory is set.
pub fn write(input: &UiRenderInput, name: &str) {
    let Ok(dir) = std::env::var(SNAPSHOT_ENV) else {
        return;
    };
    let path = Path::new(&dir).join(format!("{name}.png"));
    rasterize(input).save(&path).unwrap();
    eprintln!("snapshot: {}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vertex(x: f32, y: f32) -> UiRenderVertex {
        UiRenderVertex {
            position: [x, y],
            clip_z: 0.0,
            clip_w: 1.0,
            uv: [0.0, 0.0],
            color: [0, 0, 0, 153],
            style_flags: 0,
            alpha_cutoff: -1.0,
            model_light: 1.0,
            overlay_color: [0.0; 4],
        }
    }

    // A translucent nine-slice corner cell (two triangles sharing a diagonal)
    // blends every pixel exactly once, whether or not it sits on pixel edges.
    #[test]
    fn a_translucent_quad_has_uniform_alpha_across_its_diagonal() {
        for (x0, y0, side) in [(0.0, 0.0, 16.0), (2.5, 3.25, 11.0)] {
            let mut image = RgbaImage::from_pixel(32, 32, Rgba([255, 255, 255, 255]));
            let quad = [
                vertex(x0, y0),
                vertex(x0 + side, y0),
                vertex(x0 + side, y0 + side),
                vertex(x0, y0 + side),
            ];
            for triangle in [[0, 1, 2], [0, 2, 3]] {
                fill(
                    &mut image,
                    triangle.map(|index| quad[index]),
                    |_, color, _, _| Some(color),
                    false,
                );
            }
            let inside: std::collections::BTreeSet<u8> = image
                .enumerate_pixels()
                .filter(|(x, y, _)| {
                    let centre = [*x as f32 + 0.5, *y as f32 + 0.5];
                    centre[0] > x0
                        && centre[0] < x0 + side
                        && centre[1] > y0
                        && centre[1] < y0 + side
                })
                .map(|(_, _, pixel)| pixel[0])
                .collect();
            assert_eq!(inside.len(), 1, "{inside:?}");
        }
    }
}
