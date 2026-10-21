//! Asset-free comparisons through the production draw-list and CPU rasterizer.

use crate::carriers::Error;
use image::ImageEncoder;
use jsonui_editor::raster::{Canvas, Page};
use render_api::sprite_mask::{MaskLayout, MaskPatch, SpriteMask};
use std::sync::Arc;
use ui::{SafeArea, UiBlendMode, UiNode, UiNodeId, UiPoint, UiRect, UiScale, UiTree, UiVisual};

/// Converts finite fixture bounds into the same checked geometry used by native UI.
pub(super) fn rect(bounds: [f32; 4]) -> Result<UiRect, Error> {
    let point = |x, y| UiPoint::new(x, y).map_err(|error| Error::Invalid(format!("{error:?}")));
    UiRect::new(point(bounds[0], bounds[1])?, point(bounds[2], bounds[3])?)
        .map_err(|error| Error::Invalid(format!("{error:?}")))
}

/// Shows full, cropped and inverted masks with unrelated texture RGB and neighboring atlas art.
pub(super) fn png(size: [u32; 2], outline: bool) -> Result<Vec<u8>, Error> {
    let mut pixels = [40, 110, 190, 255].repeat(13 * 13);
    for y in 2..11 {
        for x in 2..11 {
            let at = (y * 13 + x) * 4;
            pixels[at + 3] = if (x as i32 - 6).abs() + (y as i32 - 6).abs() <= 4 {
                255
            } else {
                0
            };
        }
    }
    let mut pages = vec![
        Page::white(),
        Page {
            width: 13,
            height: 13,
            rgba: Arc::from(pixels),
            repeat_content: None,
        },
    ];
    let mask = Arc::new(SpriteMask::outline([9, 9], [9, 9], |x, y| {
        pages[1].rgba[(((y + 2) * 13 + x + 2) * 4 + 3) as usize]
    }));
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
    for (index, (cropped, inverse)) in [(false, false), (true, false), (false, true)]
        .into_iter()
        .enumerate()
    {
        let x = 80. + index as f32 * 240.;
        let color = if inverse {
            [255; 4]
        } else {
            [255, 198, 0, 180]
        };
        let uv = [if cropped { 6.5 } else { 2. }, 2., 11., 11.];
        let visual = if outline {
            let width = if cropped { 72 } else { 144 };
            let layout = MaskLayout {
                source: mask.clone(),
                extent: [width as f32, 144.],
                size: [width, 144],
                patches: vec![MaskPatch {
                    bounds: [0., 0., width as f32, 144.],
                    uv: [if cropped { 0.5 } else { 0. }, 0., 1., 1.],
                    opacity: 255,
                }],
            };
            let page = pages.len() as u16;
            pages.push(Page {
                width,
                height: 144,
                rgba: layout.rasterize().unwrap().into(),
                repeat_content: None,
            });
            UiVisual::Mesh(Arc::new(
                ui::UiMesh::sprite(
                    page,
                    [0., 0., width as f32, 144.],
                    color,
                    if inverse {
                        UiBlendMode::Invert
                    } else {
                        UiBlendMode::Alpha
                    },
                )
                .unwrap(),
            ))
        } else if inverse {
            UiVisual::InvertedSprite {
                texture_page: 1,
                uv: uv.map(|v| v as u16),
            }
        } else {
            UiVisual::Sprite {
                texture_page: 1,
                uv: uv.map(|v| v as u16),
                color,
            }
        };
        nodes.push(
            UiNode::new(
                UiNodeId::new(index as u32 + 1),
                None,
                rect([x, 48., x + if cropped { 72. } else { 144. }, 192.])?,
            )
            .with_visual(visual),
        );
    }
    let bounds = rect([0., 0., size[0] as f32, size[1] as f32])?;
    let mut tree = UiTree::new(nodes).map_err(|error| Error::Invalid(format!("{error:?}")))?;
    tree.layout(bounds, UiScale::default(), SafeArea::ZERO)
        .map_err(|error| Error::Invalid(format!("{error:?}")))?;
    let draw = tree
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
        .map_err(|error| Error::Invalid(format!("{error:?}")))?;
    Ok(png)
}
