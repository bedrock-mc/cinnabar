//! Local-only visual check: rasterizes a presentation frame's UI draw input on
//! the CPU (nearest sampling, alpha blending, scissors) into a PNG, so form
//! layouts can be inspected without a window. Written only when
//! `CINNABAR_FORM_SNAPSHOT_DIR` names a directory; images are never committed.

use std::path::Path;

use image::{Rgba, RgbaImage};
use render_model::{
    UI_BLEND_INVERT, UI_STYLE_RADIAL_GRADIENT, UiRenderInput, UiRenderVertex, UiTextureFormat,
};

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
    rasterize_offsets(input, |_| [0.; 2])
}

/// Applies the renderer's flat atlas offsets after interpolating the original source UVs.
#[cfg(test)]
pub(super) fn rasterize_resident(
    input: &UiRenderInput,
    vertices: &[render_model::FontAtlasVertex],
) -> RgbaImage {
    rasterize_offsets(input, |index| vertices[index].atlas_offset)
}

/// Samples a publication with a constant atlas relocation for each triangle.
fn rasterize_offsets(input: &UiRenderInput, offset: impl Fn(usize) -> [f32; 2]) -> RgbaImage {
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
            let [du, dv] = offset(triangle[0] as usize);
            fill(
                &mut image,
                corners,
                |[u, v], color, overlay, x, y| {
                    if x < scissor.x
                        || y < scissor.y
                        || x >= scissor.x + scissor.width
                        || y >= scissor.y + scissor.height
                    {
                        return None;
                    }
                    if corners[0].style_flags & UI_STYLE_RADIAL_GRADIENT != 0 {
                        let radius = u.hypot(v).clamp(0.0, 1.0);
                        let inner = premultiply(color);
                        let outer = [
                            overlay[0] * overlay[3],
                            overlay[1] * overlay[3],
                            overlay[2] * overlay[3],
                            overlay[3],
                        ];
                        return Some(std::array::from_fn(|channel| {
                            inner[channel] * (1.0 - radius) + outer[channel] * radius
                        }));
                    }
                    let (u, v) = (
                        ((u + du).floor() as u32).min(page_width - 1),
                        ((v + dv).floor() as u32).min(page_height - 1),
                    );
                    let at = (v * page_width + u) as usize;
                    let mut texel: [u8; 4] = match page.format() {
                        UiTextureFormat::Coverage => [255, 255, 255, pixels[at]],
                        UiTextureFormat::Rgba8 => pixels[at * 4..at * 4 + 4].try_into().unwrap(),
                    };
                    if u32::from(ui::UI_STYLE_GRAYSCALE) & corners[0].style_flags != 0 {
                        let luma = (0.299 * f32::from(texel[0])
                            + 0.587 * f32::from(texel[1])
                            + 0.114 * f32::from(texel[2]))
                        .round() as u8;
                        texel = [luma, luma, luma, texel[3]];
                    }
                    Some(premultiply(std::array::from_fn(|channel| {
                        (u16::from(texel[channel]) * u16::from(color[channel]) / 255) as u8
                    })))
                },
                batch.blend_mode == UI_BLEND_INVERT,
            );
        }
    }
    image
}

/// Converts an RGBA byte color into normalized premultiplied color for composition.
fn premultiply(color: [u8; 4]) -> [f32; 4] {
    let alpha = f32::from(color[3]) / 255.0;
    [
        f32::from(color[0]) / 255.0 * alpha,
        f32::from(color[1]) / 255.0 * alpha,
        f32::from(color[2]) / 255.0 * alpha,
        alpha,
    ]
}

/// Fill one triangle, sampling premultiplied `shade(uv, color, overlay, x, y)` at
/// each covered pixel centre and blending over the image. A centre on an edge belongs
/// only to the triangle that edge is a top or left edge of, as GPUs rasterize,
/// so a quad's shared diagonal is never blended twice.
fn fill(
    image: &mut RgbaImage,
    mut corners: [UiRenderVertex; 3],
    shade: impl Fn([f32; 2], [u8; 4], [f32; 4], u32, u32) -> Option<[f32; 4]>,
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
            let overlay = std::array::from_fn(|channel| {
                wa * corners[0].overlay_color[channel]
                    + wb * corners[1].overlay_color[channel]
                    + wc * corners[2].overlay_color[channel]
            });
            let Some(source) = shade(uv, color, overlay, x, y) else {
                continue;
            };
            let target = image.get_pixel_mut(x, y);
            let alpha = source[3];
            for channel in 0..3 {
                let over = if invert {
                    f32::from(255 - target[channel]) / 255.0 * alpha
                } else {
                    source[channel]
                };
                target[channel] =
                    (over * 255.0 + f32::from(target[channel]) * (1.0 - alpha)).round() as u8;
            }
        }
    }
}

/// Write `input` as `<dir>/<name>.png` when the snapshot directory is set.
pub fn write(input: &UiRenderInput, name: &str) {
    if std::env::var_os(SNAPSHOT_ENV).is_some() {
        write_image(&rasterize(input), name);
    }
}

/// Saves an already checked raster only when the local snapshot directory is configured.
pub(super) fn write_image(image: &RgbaImage, name: &str) {
    let Ok(dir) = std::env::var(SNAPSHOT_ENV) else {
        return;
    };
    let path = Path::new(&dir).join(format!("{name}.png"));
    image.save(&path).unwrap();
    eprintln!("snapshot: {}", path.display());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coverage_font_pages_render_like_white_rgba_pages() {
        use render_model::{
            UI_BLEND_ALPHA, UiRenderBatch, UiRenderTextureArray, UiScissor, UiTexturePage,
        };
        use std::sync::Arc;

        let coverage = [0, 73, 128, 255];
        let rgba: Vec<_> = coverage
            .iter()
            .flat_map(|&alpha| [255, 255, 255, alpha])
            .collect();
        let render = |page| {
            let vertices =
                [[0., 0.], [2., 0.], [2., 2.], [0., 2.]].map(|position| UiRenderVertex {
                    uv: position,
                    color: [100, 150, 200, 255],
                    ..vertex(position[0], position[1])
                });
            rasterize(&UiRenderInput {
                revision: 1,
                viewport_size: [2, 2],
                safe_area: [0; 4],
                vertices: vertices.into(),
                indices: [0, 1, 2, 0, 2, 3].into(),
                batches: [UiRenderBatch::new(
                    0,
                    UiScissor::new(0, 0, 2, 2),
                    0,
                    6,
                    UI_BLEND_ALPHA,
                )]
                .into(),
                textures: Arc::new(UiRenderTextureArray::new(vec![page], 1).unwrap()),
            })
        };
        let actual = render(UiTexturePage::coverage([2, 2], coverage.into()).unwrap());
        let expected = render(UiTexturePage::owned([2, 2], rgba.into()).unwrap());
        assert!(
            actual == expected,
            "coverage texels preserve font color and alpha"
        );
        assert_eq!(actual.get_pixel(0, 0).0, [70, 90, 110, 255]);
        assert_eq!(actual.get_pixel(1, 1).0, [100, 150, 200, 255]);
    }

    #[test]
    fn radial_snapshots_blend_premultiplied_stops_and_expanding_extent() {
        use render_model::{
            UI_BLEND_ALPHA, UI_STYLE_RADIAL_GRADIENT, UiRenderBatch, UiRenderTextureArray,
            UiScissor, UiTexturePage,
        };
        use std::sync::Arc;

        let render = |extent: f32| {
            let vertices =
                [[0., 0.], [3., 0.], [3., 1.], [0., 1.]].map(|position| UiRenderVertex {
                    uv: [(position[0] - 0.5) / (2.0 * extent), 0.0],
                    color: [240, 100, 60, 0],
                    overlay_color: [0.2, 0.4, 0.6, 0.8],
                    style_flags: UI_STYLE_RADIAL_GRADIENT,
                    ..vertex(position[0], position[1])
                });
            rasterize(&UiRenderInput {
                revision: 1,
                viewport_size: [3, 1],
                safe_area: [0; 4],
                vertices: vertices.into(),
                indices: [0, 1, 2, 0, 2, 3].into(),
                batches: [UiRenderBatch::new(
                    0,
                    UiScissor::new(0, 0, 3, 1),
                    0,
                    6,
                    UI_BLEND_ALPHA,
                )]
                .into(),
                textures: Arc::new(
                    UiRenderTextureArray::new(
                        vec![UiTexturePage::owned([1, 1], vec![255; 4].into()).unwrap()],
                        1,
                    )
                    .unwrap(),
                ),
            })
        };
        let settled = render(1.0);
        assert_eq!(
            settled.get_pixel(0, 0).0,
            [70, 90, 110, 255],
            "transparent inner stop preserves the world"
        );
        assert_eq!(
            settled.get_pixel(1, 0).0,
            [62, 95, 127, 255],
            "midpoint mixes premultiplied stops"
        );
        assert_eq!(
            settled.get_pixel(2, 0).0,
            [55, 100, 144, 255],
            "outer stop uses its own color and alpha"
        );
        let expanded = render(3.0);
        assert_eq!(
            expanded.get_pixel(1, 0).0,
            [67, 92, 116, 255],
            "expanded extent exposes more of the world"
        );
    }

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
                    |_, color, _, _, _| Some(premultiply(color)),
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
