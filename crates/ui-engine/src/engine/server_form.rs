//! Server-form binding, current layout geometry and painting share one cached owner.

use std::sync::{Arc, Mutex};

use json_ui::{
    FormModel, FormRender, GeometryLayout, LayoutEnv, ResolvedControl, ViewState, bind_form_over,
    render_bound_gated,
};

use super::{
    Art, FormCache, FormEngine, ScreenArt,
    render::render_with,
    screen::{PaintError, PaintFrame},
};
use crate::frame::EngineFrame;
use ui::FONT_DESIGN_PIXEL_TEXELS;

pub(super) struct LaidForm {
    view: ViewState,
    layout: json_ui::LayoutCacheKey,
    text: [usize; 3],
    font: assets::FontCatalogIdentity,
    texture_metadata: u64,
    render: FormLayout,
}

/// Unsupported components retain their original paint-time lifecycle.
enum FormLayout {
    Static(Arc<FormRender>),
    Geometry(Box<GeometryLayout>),
}

impl FormLayout {
    /// Selects automatic entrances while other programs keep their paint lifecycle.
    fn new(bound: ResolvedControl, root: [f64; 2], env: &LayoutEnv, view: &ViewState) -> Self {
        match GeometryLayout::new(&bound, root, env, view) {
            Some(layout) => Self::Geometry(Box::new(layout)),
            None => Self::Static(Arc::new(render_bound_gated(
                bound,
                root,
                env,
                view,
                &mut Default::default(),
            ))),
        }
    }

    /// Publishes the current form's paint and input geometry; only automatic entrances pre-sample.
    fn frame(
        &mut self,
        animator: &Mutex<json_ui::Animator>,
        now: f64,
        env: &LayoutEnv,
        view: &ViewState,
    ) -> Arc<FormRender> {
        match self {
            Self::Static(render) => Arc::clone(render),
            Self::Geometry(layout) => layout.sample(
                &mut animator.lock().unwrap_or_else(|poison| poison.into_inner()),
                now,
                None,
                env,
                view,
            ),
        }
    }
}

impl FormEngine {
    /// The last form's sprite textures, and those resolving to no source.
    #[cfg(any(test, feature = "test-support"))]
    pub fn drawn_sprites(&self) -> (Vec<String>, Vec<String>) {
        let atlas = self.textures.lock();
        let view = crate::textures::Textures {
            assets: &self.assets,
            set: &self.textures,
            atlas: &atlas,
            images: None,
        };
        let mut drawn: Vec<String> = self
            .cache
            .iter()
            .flat_map(|cache| cache.laid.iter())
            .flat_map(|laid| match &laid.render {
                FormLayout::Static(render) => render.nodes.iter(),
                FormLayout::Geometry(layout) => layout.current().nodes.iter(),
            })
            .filter_map(|node| match &node.draw {
                json_ui::Draw::Sprite { texture, .. } => Some(view.canonical(texture).into_owned()),
                _ => None,
            })
            .collect();
        drawn.sort();
        drawn.dedup();
        let missing = drawn
            .iter()
            .filter(|key| view.sprite(key).is_none())
            .cloned()
            .collect();
        (drawn, missing)
    }

    /// Renders `model` with supported layout animations; returns none without its template.
    pub fn render(
        &mut self,
        model: &FormModel,
        view: &ViewState,
        frame: &mut PaintFrame<'_>,
    ) -> Result<Option<EngineFrame>, PaintError> {
        let current = self.cache.as_ref().is_some_and(|cache| {
            cache.model == *model
                && cache.components.same_bindings(&view.components)
                && Arc::ptr_eq(&cache.catalog, &self.catalog)
        });
        if !current {
            self.passes[0] += 1;
            let components = &view.components;
            self.cache = Some(FormCache {
                components: components.clone(),
                model: model.clone(),
                catalog: Arc::clone(&self.catalog),
                bound: bind_form_over(model, &self.catalog, self.registry.context(), components),
                laid: None,
                screen_cancel: json_ui::form_screen_cancel(&self.catalog),
            });
        }
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let now = frame.now;
        let text = frame.language;
        let font = frame.font.identity();
        let texture_metadata = self.textures.metadata_revision();
        let art = Art {
            assets: &self.assets,
            set: &self.textures,
            animator: &self.animator,
            dependencies: &self.dependencies,
        };
        let screen_cancel = self
            .cache
            .as_ref()
            .and_then(|cache| cache.screen_cancel.clone());
        let (cache, passes) = (&mut self.cache, &mut self.passes[1]);
        let screen_art = ScreenArt {
            view: Some(view),
            ..ScreenArt::default()
        };
        let drawn = render_with(art, frame, screen_art, move |env, root| {
            let cache = cache.as_mut()?;
            let layout = json_ui::LayoutCacheKey::new(root, px, env);
            let fresh = cache.laid.as_ref().is_some_and(|laid| {
                laid.view.same_layout(view)
                    && laid.layout == layout
                    && laid.text == text
                    && laid.font == font
                    && laid.texture_metadata == texture_metadata
            });
            if !fresh {
                let bound = cache.bound.clone()?;
                *passes += 1;
                let render = FormLayout::new(bound, root, env, view);
                cache.laid = Some(LaidForm {
                    view: view.clone(),
                    layout,
                    text,
                    font,
                    texture_metadata,
                    render,
                });
            }
            cache
                .laid
                .as_mut()
                .map(|laid| laid.render.frame(art.animator, now, env, view))
        })?;
        Ok(drawn.map(|mut drawn| {
            drawn.cancel_target = drawn.cancel_target.or(screen_cancel);
            drawn
        }))
    }
}
