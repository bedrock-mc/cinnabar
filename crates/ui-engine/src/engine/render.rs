//! Layout and texture preparation, followed by painting only for visible screens.

use std::{borrow::Borrow, cell::RefCell};

use json_ui::{Draw, DrawNode, FormRender, HitRegion, LayoutEnv};
use ui::FONT_DESIGN_PIXEL_TEXELS;

use super::{
    Art, Canvas, FormEngine, ScreenArt, banner_renderer, host_edit, pixel_snap,
    screen::{PaintError, PaintFrame},
    text_paint::Measure,
    tooltip,
};
use crate::{
    frame::{EditText, EngineFrame},
    textures::Textures,
};

impl FormEngine {
    /// Lays out a screen and requests its textures without painting or sampling animations.
    pub fn prepare<R: Borrow<FormRender>>(
        &self,
        art: ScreenArt<'_>,
        frame: &mut PaintFrame<'_>,
        draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
    ) -> Option<R> {
        prepare_with(self.art(), frame, art, draw)
    }
}

/// Measures a screen and requests all textures its laid-out nodes and renderers use.
fn prepare_with<R: Borrow<FormRender>>(
    textures: Art<'_>,
    frame: &mut PaintFrame<'_>,
    art: ScreenArt<'_>,
    draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
) -> Option<R> {
    let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let root = [
        f64::from(frame.content[0] / px),
        f64::from(frame.content[1] / px),
    ];
    let render = {
        let cache = RefCell::new(&mut *frame.layouts);
        let atlas = textures.set.lock();
        let view = Textures {
            assets: textures.assets,
            set: textures.set,
            atlas: &atlas,
            images: art.images,
        };
        let measure = Measure {
            layouts: &cache,
            font: frame.font,
            metrics: frame.metrics,
            px,
            translate: frame.translate,
        };
        let env = LayoutEnv {
            text: &measure,
            textures: &view,
        };
        draw(&env, root)
    };
    let render = render?;
    let laid_render = render.borrow();
    let mut atlas = textures.set.lock();
    let laid = || {
        laid_render
            .nodes
            .iter()
            .chain(art.overlay)
            .filter(|node| !art.omits(node))
    };
    let drawn_renderer = |name: &str| {
        laid().any(|node| matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == name))
    };
    // Only what this screen draws needs to be resident.
    // Many nodes share a texture; each path resolves once.
    let mut paths: std::collections::HashSet<&str> = laid()
        .filter_map(|node| match &node.draw {
            Draw::Sprite { texture, .. } => Some(texture.as_str()),
            Draw::Custom { renderer, .. } if renderer == tooltip::RENDERER => {
                Some(tooltip::BACKGROUND_TEXTURE)
            }
            _ => None,
        })
        .collect();
    for renderer in frame.renderers.iter() {
        renderer.textures(&drawn_renderer, &mut |path| {
            paths.insert(path);
        });
    }
    for node in laid() {
        if let Draw::Custom { renderer, data } = &node.draw
            && renderer == banner_renderer::RENDERER
        {
            paths.extend(banner_renderer::texture_paths(data));
        }
    }
    let drawn = Textures {
        assets: textures.assets,
        set: textures.set,
        atlas: &atlas,
        images: art.images,
    }
    .atlas_keys(paths.into_iter());
    atlas.require(drawn.iter().map(String::as_str));
    Some(render)
}

/// Paints a prepared screen and publishes its input geometry.
pub(super) fn render_with<R: Borrow<FormRender>>(
    textures: Art<'_>,
    frame: &mut PaintFrame<'_>,
    art: ScreenArt<'_>,
    draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
) -> Result<Option<EngineFrame>, PaintError> {
    let Some(render) = prepare_with(textures, frame, art, draw) else {
        return Ok(None);
    };
    let render = render.borrow();
    let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let atlas = textures.set.lock();
    let layouts = &mut *frame.layouts;
    let mut canvas = Canvas {
        textures: Textures {
            assets: textures.assets,
            set: textures.set,
            atlas: &atlas,
            images: art.images,
        },
        solid_page: frame.solid_page,
        edit: host_edit::Target::from_frame(render, art),
        art,
        now: frame.now,
        screen: [0.0, 0.0, frame.content[0], frame.content[1]],
        layouts,
        font: frame.font,
        metrics: frame.metrics,
        px,
        translate: frame.translate,
        nodes: &mut *frame.nodes,
        next: &mut *frame.next,
        clip: None,
        animator: textures
            .animator
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    };
    let renderers = &mut *frame.renderers;
    let view = art.view;
    let mut top: Vec<std::ops::Range<usize>> = Vec::new();
    let overlay_start = render.nodes.len();
    for (index, node) in render.nodes.iter().chain(art.overlay).enumerate() {
        if !art.omits(node) && view.is_none_or(|view| node.shown(view)) {
            let start = canvas.nodes.len();
            canvas.paint(node, renderers)?;
            let lifted = index >= overlay_start
                || matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == tooltip::RENDERER);
            if lifted && canvas.nodes.len() > start {
                match top.last_mut() {
                    Some(last) if last.end == start => last.end = canvas.nodes.len(),
                    _ => top.push(start..canvas.nodes.len()),
                }
            }
        }
    }
    let origin = [frame.safe_area.left(), frame.safe_area.top()];
    Ok(Some(EngineFrame {
        hits: render.hits.clone(),
        report: render.report.clone(),
        cancel_target: render.cancel_target.clone(),
        origin,
        scale: px,
        panel: render
            .root_panel
            .map(|rect| [rect.x, rect.y, rect.w, rect.h]),
        edit_texts: edit_texts(
            &render.hits,
            &render.nodes,
            origin[0],
            [frame.metrics.gui_scale, px],
        ),
        top,
    }))
}

/// Each edit box's text label as painted: where it starts, its scale and font; `pixels` and
/// `px` are the physical and logical pixels per GUI unit.
fn edit_texts(
    hits: &[HitRegion],
    nodes: &[DrawNode],
    left: f32,
    [pixels, px]: [f32; 2],
) -> Vec<EditText> {
    hits.iter()
        .filter_map(|region| {
            let (target, _) = region.widget.edit.as_ref()?.text_target.as_ref()?;
            nodes.iter().find_map(|node| match &node.draw {
                Draw::Text { scale, options, .. } if node.key == *target => Some(EditText {
                    key: region.key.clone(),
                    left: left
                        + pixel_snap::positioned(
                            [node.dest.x, node.dest.y, node.dest.w, node.dest.h],
                            pixels,
                            px,
                        )[0],
                    scale: *scale,
                    font: options.font_type.clone(),
                }),
                _ => None,
            })
        })
        .collect()
}
