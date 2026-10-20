//! Layout and texture preparation, followed by painting only for visible screens.

use std::{borrow::Borrow, cell::RefCell};

use json_ui::{Draw, DrawNode, FormRender, HitRegion, LayoutEnv};
use ui::FONT_DESIGN_PIXEL_TEXELS;

use super::{
    Art, Canvas, FormEngine, ScreenArt, dependencies, host_edit,
    screen::{PaintError, PaintFrame},
    text_paint::{Measure, label_font, label_metrics, label_positioned},
    tooltip,
};
use crate::{
    frame::{EditText, EngineFrame},
    text_metrics::TextMetrics,
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
    let base = textures
        .dependencies
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .get(&laid_render.nodes, art);
    let mut atlas = textures.set.lock();
    let paths = dependencies::paths(&base, art, frame.renderers);
    let drawn = Textures {
        assets: textures.assets,
        set: textures.set,
        atlas: &atlas,
        images: art.images,
    }
    .atlas_keys(paths.into_iter());
    atlas.require(drawn.iter().map(std::ops::Deref::deref));
    drop(base);
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
        length_scale: 1.0,
        residual: None,
        paint_bounds: None,
        filter: None,
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
            canvas.enter_filter(node)?;
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
    canvas.close_filter()?;
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
            frame.metrics,
            frame.font,
        ),
        top,
    }))
}

/// Each edit box's text label as painted, including its effective scale and named face.
fn edit_texts(
    hits: &[HitRegion],
    nodes: &[DrawNode],
    left: f32,
    metrics: TextMetrics,
    font: &assets::RuntimeFontCatalog,
) -> Vec<EditText> {
    let px = metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    hits.iter()
        .filter_map(|region| {
            let (target, _) = region.widget.edit.as_ref()?.text_target.as_ref()?;
            nodes.iter().find_map(|node| match &node.draw {
                Draw::Text { scale, options, .. } if node.key == *target => {
                    let label = label_metrics(metrics, font, label_font(font, options), *scale);
                    let name = font.resolve_font_name(
                        options.font_type.as_deref().unwrap_or("default"),
                        options.backup_font_type.as_deref(),
                    );
                    Some(EditText {
                        key: region.key.clone(),
                        left: left
                            + label_positioned(
                                [node.dest.x, node.dest.y, node.dest.w, node.dest.h],
                                font,
                                options,
                                metrics.gui_scale,
                                px,
                            )[0],
                        scale: label.factor(),
                        effect_scale_1024: label.effect_scale_1024(),
                        font: (name != "default").then(|| name.to_owned()),
                    })
                }
                _ => None,
            })
        })
        .collect()
}
