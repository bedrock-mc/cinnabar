//! Original sparse-edge fixtures through production draw lists and CPU sampling.

use super::sprite_outline::rect;
use crate::carriers::Error;
use image::ImageEncoder;
use jsonui_editor::raster::{Canvas, Page};
use ui::{UiBlendMode, UiNode, UiNodeId, UiTree, UiVisual};

/// Shows sparse last-column art above shared edge art, with alpha at left and inverse at right.
pub(super) fn png(size: [u32; 2], crop: bool, source_alpha: u8) -> Result<Vec<u8>, Error> {
    let pages: Vec<_> = std::iter::once(Page::white())
        .chain([false, true].map(|both_edges| {
            Page {
                width: 9,
                height: 9,
                rgba: (0..81)
                    .flat_map(|at| {
                        [
                            30,
                            50,
                            70,
                            if at % 9 == 8 || both_edges && at % 9 == 0 {
                                source_alpha
                            } else {
                                0
                            },
                        ]
                    })
                    .collect(),
                repeat_content: None,
            }
        }))
        .collect();
    let mut nodes = vec![
        UiNode::new(
            UiNodeId::new(0),
            None,
            rect([0., 0., size[0] as f32, size[1] as f32])?,
        )
        .with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [64, 80, 96, 255],
        }),
    ];
    for (row, page) in [1, 2].into_iter().enumerate() {
        for (column, blend) in [UiBlendMode::Alpha, UiBlendMode::Invert]
            .into_iter()
            .enumerate()
        {
            for index in 0..3 {
                let x = 208. + column as f32 * 300. - index as f32 * 64.;
                let y = 28. + row as f32 * 112.;
                let right = if crop && index > 0 { 8. } else { 9. };
                nodes.push(
                    UiNode::new(
                        UiNodeId::new(nodes.len() as u32),
                        None,
                        rect([x, y, x + right * 8., y + 72.])?,
                    )
                    .with_visual(UiVisual::SpriteOutline {
                        texture_page: page,
                        uv: [0., 0., right, 9.],
                        source: [0., 0., 9., 9.],
                        overlap_offset: if crop || index == 0 {
                            [0.; 2]
                        } else {
                            [8. / 9., 0.]
                        },
                        color: if blend == UiBlendMode::Invert {
                            [255; 4]
                        } else {
                            [255, 198, 0, 180]
                        },
                        blend,
                    }),
                );
            }
        }
    }
    let draw = UiTree::new(nodes)
        .map_err(|error| Error::Invalid(format!("{error:?}")))?
        .build_draw_list()
        .map_err(|error| Error::Invalid(format!("{error:?}")))?;
    let mut canvas = Canvas::new(size[0], size[1]);
    canvas.draw(&draw, |page| pages.get(usize::from(page)));
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png)
        .write_image(
            &canvas.into_rgba(),
            size[0],
            size[1],
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|error| Error::Invalid(error.to_string()))?;
    Ok(png)
}
