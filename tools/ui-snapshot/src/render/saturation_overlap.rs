//! Original row masks through production draw lists and ordinary CPU sprite sampling.

use super::sprite_outline::rect;
use crate::carriers::Error;
use image::ImageEncoder;
use jsonui_editor::raster::{Canvas, Page};
use render_api::sprite_mask::{MaskLayout, MaskPatch, SpriteMask};
use std::sync::Arc;
use ui::{UiBlendMode, UiMesh, UiNode, UiNodeId, UiTree, UiVisual};

/// One original sparse or shared-edge source, independent of any installed pack.
fn edge_mask(left: u8, right: u8) -> Arc<SpriteMask> {
    Arc::new(SpriteMask::outline([9, 9], |x, _| {
        if x == 0 {
            left
        } else if x == 8 {
            right
        } else {
            0
        }
    }))
}

/// Three complete source masks at the hunger row's eight-unit pitch.
fn layout(source: Arc<SpriteMask>, crop: bool, size: [u32; 2]) -> MaskLayout {
    MaskLayout {
        source,
        extent: [25., 9.],
        size,
        patches: (0..3)
            .map(|index| {
                let x = 16. - index as f32 * 8.;
                let right = if crop && index > 0 { 8. } else { 9. };
                MaskPatch {
                    bounds: [x, 0., x + right, 9.],
                    uv: [0., 0., right / 9., 1.],
                    opacity: 255,
                }
            })
            .collect(),
    }
}

/// Adds a prepared mask as an ordinary textured quad in the requested blend mode.
fn mask(
    nodes: &mut Vec<UiNode>,
    pages: &mut Vec<Page>,
    layout: MaskLayout,
    at: [f32; 2],
    blend: UiBlendMode,
) -> Result<(), Error> {
    let [w, h] = layout.size;
    let page = pages.len() as u16;
    pages.push(Page {
        width: w,
        height: h,
        rgba: layout
            .rasterize()
            .ok_or_else(|| Error::Invalid("invalid fixture mask".into()))?
            .into(),
        repeat_content: None,
    });
    let color = if blend == UiBlendMode::Invert {
        [255; 4]
    } else {
        [255, 198, 0, 180]
    };
    nodes.push(
        UiNode::new(
            UiNodeId::new(nodes.len() as u32),
            None,
            rect([at[0], at[1], at[0] + w as f32, at[1] + h as f32])?,
        )
        .with_visual(UiVisual::Mesh(Arc::new(
            UiMesh::sprite(page, [0., 0., w as f32, h as f32], color, blend)
                .map_err(|e| Error::Invalid(e.to_string()))?,
        ))),
    );
    Ok(())
}

/// The anonymous background common to both mask comparisons.
fn background(size: [u32; 2]) -> Result<Vec<UiNode>, Error> {
    Ok(vec![
        UiNode::new(
            UiNodeId::new(0),
            None,
            rect([0., 0., size[0] as f32, size[1] as f32])?,
        )
        .with_visual(UiVisual::Solid {
            texture_page: 0,
            color: [64, 80, 96, 255],
        }),
    ])
}

/// Encodes the production CPU rasterizer's ordinary textured draw list.
fn finish(size: [u32; 2], nodes: Vec<UiNode>, pages: Vec<Page>) -> Result<Vec<u8>, Error> {
    let draw = UiTree::new(nodes)
        .map_err(|e| Error::Invalid(format!("{e:?}")))?
        .build_draw_list()
        .map_err(|e| Error::Invalid(format!("{e:?}")))?;
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
        .map_err(|e| Error::Invalid(e.to_string()))?;
    Ok(png)
}

/// Shows sparse art above shared edges, with alpha at left and inverse at right.
pub(super) fn png(size: [u32; 2], crop: bool, source_alpha: u8) -> Result<Vec<u8>, Error> {
    let mut nodes = background(size)?;
    let mut pages = vec![Page::white()];
    for (row, left) in [0, source_alpha].into_iter().enumerate() {
        for (column, blend) in [UiBlendMode::Alpha, UiBlendMode::Invert]
            .into_iter()
            .enumerate()
        {
            mask(
                &mut nodes,
                &mut pages,
                layout(edge_mask(left, source_alpha), crop, [200, 72]),
                [80. + column as f32 * 300., 28. + row as f32 * 112.],
                blend,
            )?;
        }
    }
    finish(size, nodes, pages)
}

/// Shows unequal translucent edges above 72-texel sources minified to 18-pixel icons.
pub(super) fn regression_png(size: [u32; 2]) -> Result<Vec<u8>, Error> {
    let mut nodes = background(size)?;
    let mut pages = vec![Page::white()];
    for (column, blend) in [UiBlendMode::Alpha, UiBlendMode::Invert]
        .into_iter()
        .enumerate()
    {
        mask(
            &mut nodes,
            &mut pages,
            layout(edge_mask(128, 255), false, [200, 72]),
            [80. + column as f32 * 300., 28.],
            blend,
        )?;
        mask(
            &mut nodes,
            &mut pages,
            layout(
                Arc::new(SpriteMask::outline([72, 72], |_, _| 255)),
                false,
                [50, 18],
            ),
            [152. + column as f32 * 300., 160.],
            blend,
        )?;
    }
    finish(size, nodes, pages)
}
