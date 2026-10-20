//! Presents a server form through the JSON-UI engine: the vanilla
//! `ui/*.json` templates resolve against the compiled carrier's catalog, lay out
//! in virtual UI pixels, and their draw nodes become retained UI nodes over the
//! carrier's atlas pages. One virtual pixel is one GUI pixel of the HUD's scale
//! (needs native measurement against Bedrock's own scale-index rule).
mod scene_policy;
pub mod tooltip;

use std::{
    borrow::{Borrow, Cow},
    sync::Arc,
};

use assets::{RuntimeFontCatalog, RuntimeUiAssets};
use json_ui::{
    Catalog, Context, DataSource, Draw, DrawNode, FormModel, FormRender, LayoutEnv, RectOut,
    ResolvedControl, ViewState, bind_form_over, render_bound_gated,
};
use ui::{TextLayoutCache, TextShadow, UiNode, UiNodeId, UiPoint, UiRect, UiVisual};

use {crate::text_metrics::TextMetrics, ui::FONT_DESIGN_PIXEL_TEXELS};

mod banner_renderer;
mod dependencies;
mod fill_renderers;
mod filter;
mod formatting_colors;
mod fragment;
pub use fragment::{CachedFragment, CachedFragmentTextures, normalize_fragment};
pub mod glass;
pub mod host_edit;
pub mod item_renderer;
#[cfg(test)]
mod ownership_tests;
pub mod pack_catalog;
mod paint_bounds;
pub use paint_bounds::{PaintBounds, PaintObserver};
mod paint_transform;
pub mod pixel_snap;
mod render;
mod rounded;
pub mod screen;
pub mod screen_cache;
mod sprites;
pub mod text_paint;
mod vector_icons;
mod vector_paths;
use crate::atlas::ServerAtlas;
use crate::frame::EngineFrame;
use crate::sources::{ImageWork, RemoteSource};
use crate::textures::{TextureSet, Textures};
use render::render_with;
use screen::{CustomDraw, CustomRenderer, PaintError, PaintFrame, ScreenRegistry};
use text_paint::TextPaint;

pub struct FormEngine {
    assets: Arc<RuntimeUiAssets>,
    /// The carrier's vanilla catalog, before the built-in packs.
    vanilla: Arc<Catalog>,
    /// Vanilla under the built-in packs: the catalog with no server pack.
    base: Arc<Catalog>,
    catalog: Arc<Catalog>,
    /// Built-in screens' catalog carrying the installed pack's filters.
    builtin_screens: Arc<Catalog>,
    /// What the screen families registered: catalog policy, screens and binding hooks.
    registry: &'static ScreenRegistry,
    pub formatting_palette: ui::FormattingPalette,
    /// Where texture paths draw from, including the on-demand server atlas.
    pub textures: TextureSet,
    /// The atlas page images last handed to the dynamic pages.
    pub server_pages: Vec<render_model::UiTexturePage>,
    pub server_sheet_pages: Vec<render_model::UiTexturePage>,
    /// Whether a server pack's ui files are layered over the base catalog.
    pub server_ui: bool,
    /// The last form's bound tree and laid-out output, reused while unchanged.
    pub cache: Option<FormCache>,
    /// Resolve+bind and layout passes run, for cache tests and profiling.
    pub passes: [usize; 2],
    screens: screen_cache::ScreenCache,
    /// Logical dependencies of recent immutable draw-node snapshots.
    dependencies: std::sync::Mutex<dependencies::DependencyCache>,
    /// Animation state of every drawn control, keyed by layout key.
    animator: std::sync::Mutex<json_ui::Animator>,
}

pub struct FormCache {
    model: FormModel,
    components: json_ui::Components,
    catalog: Arc<Catalog>,
    /// A rejected bind retains its input key without publishing a partial tree.
    bound: Option<ResolvedControl>,
    laid: Option<LaidForm>,
    /// The screen's Escape target; flattening the screen per frame deep-clones pack controls.
    screen_cancel: Option<String>,
}

struct LaidForm {
    view: ViewState,
    layout: json_ui::LayoutCacheKey,
    text: [usize; 3],
    font: assets::FontCatalogIdentity,
    texture_metadata: u64,
    render: FormRender,
}

/// Borrowed texture sources a paint reads.
#[derive(Clone, Copy)]
struct Art<'a> {
    assets: &'a RuntimeUiAssets,
    set: &'a TextureSet,
    animator: &'a std::sync::Mutex<json_ui::Animator>,
    dependencies: &'a std::sync::Mutex<dependencies::DependencyCache>,
}

/// A carrier catalog extended and layered with a registry's built-in screens: the costly part
/// of building a [`FormEngine`], which needs no engine state and may run on any thread.
pub struct PreparedCatalog {
    vanilla: Arc<Catalog>,
    base: Arc<Catalog>,
    formatting_palette: ui::FormattingPalette,
    registry: &'static ScreenRegistry,
}

impl PreparedCatalog {
    /// Extends `catalog` with the families `registry` records and layers its built-in screens.
    pub fn new(mut catalog: Catalog, registry: &'static ScreenRegistry) -> Self {
        registry.catalog().extended(&mut catalog);
        let vanilla = Arc::new(catalog);
        let base = Arc::new(registry.catalog().builtin(&vanilla));
        Self {
            formatting_palette: formatting_colors::from_catalog(&base),
            vanilla,
            base,
            registry,
        }
    }
}

impl FormEngine {
    /// An engine over `assets` and its `catalog` under the families `registry` records; URL
    /// textures download through `remote`, and images read and decode as `work` allows.
    pub fn new(
        assets: Arc<RuntimeUiAssets>,
        catalog: Catalog,
        first_page: u16,
        registry: &'static ScreenRegistry,
        remote: Arc<dyn RemoteSource>,
        work: ImageWork,
    ) -> Self {
        let catalog = PreparedCatalog::new(catalog, registry);
        Self::with_prepared(assets, catalog, first_page, remote, work)
    }

    /// [`Self::new`] over a catalog already prepared for its registry.
    pub fn with_prepared(
        assets: Arc<RuntimeUiAssets>,
        catalog: PreparedCatalog,
        first_page: u16,
        remote: Arc<dyn RemoteSource>,
        work: ImageWork,
    ) -> Self {
        let PreparedCatalog {
            vanilla,
            base,
            formatting_palette,
            registry,
        } = catalog;
        Self {
            textures: TextureSet::new(first_page, remote, work)
                .with_builtin_images(registry.builtin_images())
                .with_carrier(Arc::clone(&assets)),
            assets,
            catalog: Arc::clone(&base),
            builtin_screens: Arc::clone(&base),
            formatting_palette,
            screens: screen_cache::ScreenCache::default(),
            vanilla,
            base,
            registry,
            server_pages: Vec::new(),
            server_sheet_pages: Vec::new(),
            server_ui: false,
            cache: None,
            passes: [0; 2],
            animator: std::sync::Mutex::default(),
            dependencies: std::sync::Mutex::default(),
        }
    }

    fn art(&self) -> Art<'_> {
        Art {
            assets: &self.assets,
            set: &self.textures,
            animator: &self.animator,
            dependencies: &self.dependencies,
        }
    }

    /// The animator every paint ticks: the dispatcher fires button events into
    /// it, drains its end and destroy events, and ends each presented frame.
    pub fn animator(&self) -> std::sync::MutexGuard<'_, json_ui::Animator> {
        self.animator
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Which catalog forms resolve against, for the render-path log.
    pub fn catalog_label(&self) -> String {
        if Arc::ptr_eq(&self.catalog, &self.base) {
            "vanilla catalog with the Java HUD pack".to_owned()
        } else {
            let notes = self
                .catalog
                .diagnostics()
                .len()
                .saturating_sub(self.vanilla.diagnostics().len());
            format!("server pack overlay, {notes} pack diagnostics")
        }
    }

    /// Install a server texture atlas whose pages start at texture page `first`.
    pub fn set_server_atlas(&mut self, atlas: ServerAtlas, first: u16) {
        self.textures.set_atlas(atlas, first);
        if let Some(cache) = &mut self.cache {
            cache.laid = None;
        }
        self.screens.invalidate_layouts();
    }

    /// Polls original-image readiness even when the current menu needs no repaint.
    pub fn poll_native_images(&mut self) -> bool {
        let atlas = self.textures.atlas_mut();
        atlas.poll_native_images() | atlas.take_metadata_changed()
    }

    /// The atlas page images when they changed since the last call.
    pub fn take_server_pages(&mut self) -> Option<&[render_model::UiTexturePage]> {
        let atlas = self.textures.atlas_mut();
        if !atlas.take_dirty() {
            return None;
        }
        self.server_pages = atlas.images().to_vec();
        self.server_sheet_pages = atlas.native_pages().to_vec();
        Some(&self.server_pages)
    }

    /// The last form's sprite textures, and those resolving to no source.
    #[cfg(any(test, feature = "test-support"))]
    pub fn drawn_sprites(&self) -> (Vec<String>, Vec<String>) {
        let atlas = self.textures.lock();
        let view = Textures {
            assets: &self.assets,
            set: &self.textures,
            atlas: &atlas,
            images: None,
        };
        let mut drawn: Vec<String> = self
            .cache
            .iter()
            .flat_map(|cache| cache.laid.iter())
            .flat_map(|laid| laid.render.nodes.iter())
            .filter_map(|node| match &node.draw {
                Draw::Sprite { texture, .. } => Some(view.canonical(texture).into_owned()),
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

    /// Apply a server pack's ui files over vanilla and the built-in packs; none restores the base.
    pub fn set_server_pack(&mut self, layers: &[Vec<(String, Vec<u8>)>]) {
        if layers.iter().all(Vec::is_empty) {
            self.install_pack_catalog(Arc::clone(&self.base), None);
            return;
        }
        let layered = self.registry.catalog().layer(&self.vanilla, layers);
        self.install_pack_catalog(
            Arc::new(layered.pack),
            layered.builtin_screens.map(Arc::new),
        );
    }

    /// Returns the original catalog used by background pack compilation.
    pub fn pack_catalog_base(&self) -> Arc<Catalog> {
        self.vanilla.clone()
    }

    /// Publishes worker-resolved catalogs and retires caches holding the previous ones;
    /// `builtin_screens` defaults to the base catalog.
    pub fn install_pack_catalog(
        &mut self,
        catalog: Arc<Catalog>,
        builtin_screens: Option<Arc<Catalog>>,
    ) {
        self.builtin_screens = builtin_screens.unwrap_or_else(|| Arc::clone(&self.base));
        self.formatting_palette = formatting_colors::from_catalog(&catalog);
        self.catalog = catalog;
        self.cache = None;
        self.screens = screen_cache::ScreenCache::default();
        *self
            .dependencies
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner()) =
            dependencies::DependencyCache::default();
        *self
            .animator
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner()) = json_ui::Animator::new();
    }

    /// Render `model`; `Ok(None)` without its template. Layout holds until model or scroll change.
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
                let measures = &mut Default::default();
                let render = render_bound_gated(bound, root, env, view, measures);
                cache.laid = Some(LaidForm {
                    view: view.clone(),
                    layout,
                    text,
                    font,
                    texture_metadata,
                    render,
                });
            }
            cache.laid.as_ref().map(|laid| &laid.render)
        })?;
        Ok(drawn.map(|mut drawn| {
            drawn.cancel_target = drawn.cancel_target.or(screen_cancel);
            drawn
        }))
    }

    pub fn assets(&self) -> &RuntimeUiAssets {
        &self.assets
    }

    pub fn catalog(&self) -> &Arc<Catalog> {
        &self.catalog
    }

    /// The catalog before server UI layers, including registered built-in packs.
    pub fn base_catalog(&self) -> &Arc<Catalog> {
        &self.base
    }

    /// Registered built-in screens resolve against the base catalog carrying pack filters;
    /// others use the pack stack.
    pub fn screen_catalog(&self, reference: &str) -> &Arc<Catalog> {
        if self.registry.builtin(reference) {
            &self.builtin_screens
        } else {
            &self.catalog
        }
    }

    /// Resolves built-in screens against the carrier alone, or again under the built-in packs.
    #[cfg(any(test, feature = "test-support"))]
    pub fn set_native_chat_fixture(&mut self, native: bool) {
        self.base = if native {
            Arc::clone(&self.vanilla)
        } else {
            Arc::new(self.registry.catalog().builtin(&self.vanilla))
        };
        self.install_pack_catalog(Arc::clone(&self.base), None);
    }

    pub fn context(&self) -> &Context {
        self.registry.context()
    }

    /// Paint what `draw` lays out over this engine's textures; `Ok(None)` when it lays out nothing.
    pub fn draw<R: Borrow<FormRender>>(
        &self,
        art: ScreenArt<'_>,
        frame: &mut PaintFrame<'_>,
        draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
    ) -> Result<Option<EngineFrame>, PaintError> {
        render_with(self.art(), frame, art, draw)
    }

    /// [`Self::draw`] over `textures` instead of the engine's own sources.
    pub fn draw_with<R: Borrow<FormRender>>(
        &self,
        textures: &TextureSet,
        art: ScreenArt<'_>,
        frame: &mut PaintFrame<'_>,
        draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
    ) -> Result<Option<EngineFrame>, PaintError> {
        let sources = Art {
            assets: &self.assets,
            set: textures,
            animator: &self.animator,
            dependencies: &self.dependencies,
        };
        render_with(sources, frame, art, draw)
    }

    /// Render a registered screen against `data` under `view`; `frame`'s renderers draw its
    /// custom controls.
    pub fn render_screen<'a>(
        &self,
        reference: &str,
        data: &DataSource,
        context: &Context,
        view: &'a ViewState,
        mut art: ScreenArt<'a>,
        frame: &mut PaintFrame<'_>,
    ) -> Result<Option<EngineFrame>, PaintError> {
        art.view = Some(view);
        let px = frame.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let dpi_scale = frame.metrics.dpi_scale.get();
        let text = frame.language;
        let texture_metadata = self.textures.metadata_revision();
        render_with(self.art(), frame, art, |env, root| {
            let key = screen_cache::ScreenKey {
                reference,
                catalog: &self.catalog,
                context,
                data,
                view,
                root,
                px,
                dpi_scale,
                text,
                texture_metadata,
            };
            self.screens.render(key, env, self.registry)
        })
    }
}

/// `[left, top, right, bottom]` in logical px as a rect.
pub fn rect(left: f32, top: f32, right: f32, bottom: f32) -> Result<UiRect, PaintError> {
    UiRect::new(
        UiPoint::new(left, top).map_err(PaintError::Geometry)?,
        UiPoint::new(right, bottom).map_err(PaintError::Geometry)?,
    )
    .map_err(PaintError::Geometry)
}

/// Selects a label's optional default formatting color after binding and localization.
pub type TextColor<'a> = &'a dyn Fn(&DrawNode) -> Option<char>;

/// What the engine paints a screen against besides its layout: the interaction state, the
/// hover tooltip and pointer (virtual px), transition clocks, artwork and edit feedback.
#[derive(Clone, Copy, Default)]
pub struct ScreenArt<'a> {
    /// Keeps this screen's sprites on the shared nearest sampler, including pack overrides.
    pub nearest_sampling: bool,
    /// Optional bounds observation of selected screen artwork, across renderer types.
    pub paint_bounds: Option<PaintObserver<'a>>,
    /// GUI offset into the source coordinate system used for physical-pixel snapping.
    /// Cached artwork adds this before snapping each control and removes it afterward.
    pub pixel_snap_offset: [f64; 2],
    /// Native replacements retain the pack's backdrop while omitting the replaced controls.
    pub omit_controls: &'a [&'a str],
    /// The interaction state gated nodes ([`json_ui::render_bound_gated`]) paint under.
    pub view: Option<&'a ViewState>,
    /// Text a shown hover tooltip draws instead of its bound `#hover_text`.
    pub tooltip: Option<&'a str>,
    pub pointer: Option<[f32; 2]>,
    /// Default formatting color chosen per label after binding and localization.
    pub text_color: Option<TextColor<'a>>,
    /// Creation times that fades naming a clock read instead of their own.
    pub clocks: Option<&'a std::collections::BTreeMap<String, f64>>,
    pub images: Option<&'a std::collections::HashMap<String, ui::IconRef>>,
    pub edit: Option<host_edit::Feedback>,
    /// Caller nodes painted above the screen (the held stack).
    pub overlay: &'a [DrawNode],
}

impl ScreenArt<'_> {
    fn omits(&self, node: &DrawNode) -> bool {
        self.omit_controls.iter().any(|name| {
            node.key
                .split('/')
                .any(|part| part.split(['[', '~']).next() == Some(*name))
        })
    }
}

/// Turns draw nodes into retained UI nodes, opening a clip group per clip change to keep
/// order; custom renderers draw through it.
pub struct Canvas<'a> {
    edit: Option<host_edit::Target<'a>>,
    textures: Textures<'a>,
    solid_page: u16,
    art: ScreenArt<'a>,
    now: f64,
    /// The whole content area, the clip for unclipped tooltips.
    screen: [f32; 4],
    layouts: &'a mut TextLayoutCache,
    font: &'a RuntimeFontCatalog,
    metrics: TextMetrics,
    px: f32,
    translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
    nodes: &'a mut Vec<UiNode>,
    next: &'a mut u32,
    clip: Option<([f32; 4], UiNodeId)>,
    animator: std::sync::MutexGuard<'a, json_ui::Animator>,
    /// Multiplies authored lengths of a coverage mesh baked at a transformed size.
    length_scale: f32,
    /// The paint transform left for the current draw's nodes, in logical px.
    residual: Option<json_ui::transform::Affine>,
    /// An optional scoped observer of clipped, transformed logical paint bounds.
    paint_bounds: Option<Option<[f32; 4]>>,
    /// The filter group receiving the current draws.
    filter: Option<filter::OpenFilter>,
}

impl<'a> Canvas<'a> {
    /// Logical px per GUI px.
    pub fn px(&self) -> f32 {
        self.px
    }

    pub fn solid_page(&self) -> u16 {
        self.solid_page
    }

    pub fn metrics(&self) -> TextMetrics {
        self.metrics
    }

    pub fn font(&self) -> &'a RuntimeFontCatalog {
        self.font
    }

    pub fn layouts(&mut self) -> &mut TextLayoutCache {
        self.layouts
    }

    /// The whole content area in logical px.
    pub fn screen(&self) -> [f32; 4] {
        self.screen
    }

    /// The current clip group's bounds.
    pub fn clip(&self) -> Option<[f32; 4]> {
        self.clip.map(|(bounds, _)| bounds)
    }

    /// The pointer in virtual px, when the caller tracks one.
    pub fn pointer(&self) -> Option<[f32; 2]> {
        self.art.pointer
    }

    pub fn textures(&self) -> &Textures<'a> {
        &self.textures
    }
}

impl Canvas<'_> {
    fn logical(&self, rect: &RectOut) -> [f32; 4] {
        let px = self.px;
        [
            rect.x as f32 * px,
            rect.y as f32 * px,
            (rect.x + rect.w) as f32 * px,
            (rect.y + rect.h) as f32 * px,
        ]
    }

    /// `rect` as logical bounds on whole physical pixels, where vanilla places
    /// an image ([`pixel_snap`]).
    fn snapped(&self, rect: &RectOut) -> [f32; 4] {
        pixel_snap::translated(
            [rect.x, rect.y, rect.w, rect.h],
            self.art.pixel_snap_offset,
            self.px,
            |rect| pixel_snap::snapped(rect, self.metrics.gui_scale, self.px),
        )
    }

    /// `rect` as logical bounds moved to a whole physical pixel, where vanilla
    /// places text ([`pixel_snap`]).
    fn positioned(&self, rect: &RectOut) -> [f32; 4] {
        pixel_snap::translated(
            [rect.x, rect.y, rect.w, rect.h],
            self.art.pixel_snap_offset,
            self.px,
            |rect| pixel_snap::positioned(rect, self.metrics.gui_scale, self.px),
        )
    }

    fn id(&mut self) -> UiNodeId {
        let id = UiNodeId::new(*self.next);
        *self.next = self.next.saturating_add(1);
        id
    }

    /// The clip group for `clip`, reusing the current one when it matches.
    pub fn group(&mut self, clip: [f32; 4]) -> Result<UiNodeId, PaintError> {
        if let Some((current, id)) = self.clip
            && current == clip
        {
            return Ok(id);
        }
        let id = self.id();
        self.nodes.push(
            UiNode::new(id, None, rect(clip[0], clip[1], clip[2], clip[3])?)
                .with_clip_children(true),
        );
        self.clip = Some((clip, id));
        Ok(id)
    }

    /// Hands `node` to the frame's renderers, then to the engine's own: vector icons, rounded
    /// chrome, fills, messaging art, named profile images, name tags and banners. Others draw
    /// nothing.
    fn custom(
        &mut self,
        node: &CustomDraw<'_>,
        renderers: &mut [&mut dyn CustomRenderer],
    ) -> Result<(), PaintError> {
        for renderer in renderers.iter_mut() {
            if renderer.paint(node, self)? {
                return Ok(());
            }
        }
        let (data, dest) = (node.data, node.dest);
        let alpha = |color: [u8; 4]| node.alpha(color);
        let drawn = match node.renderer {
            "cinnabar_vector_icon" => self
                .vector_icon(data, dest, &alpha)
                .map(|visual| (visual, dest)),
            json_ui::vector_path::RENDERER => self
                .vector_path(data, dest, &alpha)
                .map(|visual| (visual, dest)),
            json_ui::rounded::RENDERER => {
                self.rounded_shadow(data, dest, &alpha)?;
                self.rounded_rectangle(data, dest, &alpha)
                    .map(|visual| (visual, dest))
            }
            "progress_bar_renderer" => {
                self.progress_bar(data, dest, &alpha);
                None
            }
            "gradient_renderer" => self.gradient(data, &alpha).map(|visual| (visual, dest)),
            "animated_gif_renderer" => self.animated_gif(data, dest, &alpha),
            "profile_image_renderer" => self.named_profile_image(data, dest, &alpha),
            "name_tag_renderer" => self.name_tag(data, dest, &alpha),
            banner_renderer::RENDERER => {
                self.banner_pattern(data, dest, &alpha)?;
                None
            }
            _ => None,
        };
        match drawn {
            Some((visual, bounds)) => self.push(visual, bounds),
            None => Ok(()),
        }
    }

    /// Push `visual` at `bounds` into the current clip group.
    pub fn push(&mut self, mut visual: UiVisual, bounds: [f32; 4]) -> Result<(), PaintError> {
        let Some((clip, parent)) = self.clip else {
            return Ok(());
        };
        if self.art.nearest_sampling {
            match &mut visual {
                UiVisual::StyledSprite { style, .. } => *style &= !ui::UI_STYLE_BILINEAR,
                UiVisual::RepeatedSprite { style, .. } => {
                    *style &= !u16::from(ui::UI_STYLE_BILINEAR)
                }
                _ => {}
            }
        }
        self.observe_paint_bounds(bounds, clip);
        let id = self.id();
        self.nodes.push(
            UiNode::new(
                id,
                Some(parent),
                rect(
                    bounds[0] - clip[0],
                    bounds[1] - clip[1],
                    bounds[2] - clip[0],
                    bounds[3] - clip[1],
                )?,
            )
            .with_visual(visual),
        );
        Ok(())
    }

    /// A solid rect in the current clip group.
    pub fn solid(&mut self, bounds: [f32; 4], color: [u8; 4]) -> Result<(), PaintError> {
        let visual = UiVisual::Solid {
            texture_page: self.solid_page,
            color,
        };
        self.push(visual, bounds)
    }

    /// Paints one draw node, then moves what it pushed by its paint transform.
    fn paint(
        &mut self,
        node: &DrawNode,
        renderers: &mut [&mut dyn CustomRenderer],
    ) -> Result<(), PaintError> {
        let start = self.nodes.len();
        let painted = self.paint_draw(node, renderers);
        self.transform_painted(start);
        self.tag_filtered(start);
        self.length_scale = 1.0;
        painted
    }

    fn paint_draw(
        &mut self,
        node: &DrawNode,
        renderers: &mut [&mut dyn CustomRenderer],
    ) -> Result<(), PaintError> {
        if let Draw::Sprite { texture, .. } = &node.draw
            && node
                .anim
                .as_ref()
                .and_then(|anim| anim.own.as_ref())
                .is_some_and(|own| {
                    own.graph.nodes.iter().any(|anim| {
                        matches!(
                            anim.kind,
                            json_ui::AnimKind::FlipBook | json_ui::AnimKind::Aseprite
                        )
                    })
                })
            && !self.textures.animation_ready(texture)
        {
            return Ok(());
        }
        let drawn = node.animate(
            &mut self.animator,
            self.now,
            self.art.clocks,
            Some(&self.textures),
        );
        let floats =
            matches!(&node.draw, Draw::Custom { renderer, .. } if renderer == tooltip::RENDERER);
        let clip = if floats {
            self.screen
        } else {
            self.logical(&drawn.clip)
        };
        let dest = if let Draw::Text { options, .. } = &node.draw {
            pixel_snap::translated(
                [drawn.dest.x, drawn.dest.y, drawn.dest.w, drawn.dest.h],
                self.art.pixel_snap_offset,
                self.px,
                |rect| {
                    text_paint::label_positioned(
                        rect,
                        self.font,
                        options,
                        self.metrics.gui_scale,
                        self.px,
                    )
                },
            )
        } else {
            self.snapped(&drawn.dest)
        };
        let dest = match &drawn.transform {
            Some(transform) => {
                let Some(placement) = paint_transform::place(&node.draw, transform, dest, self.px)
                else {
                    return Ok(());
                };
                self.residual = placement.residual;
                self.length_scale = placement.length_scale;
                placement.dest
            }
            None => dest,
        };
        let opacity = drawn.opacity;
        if self
            .edit
            .is_some_and(|edit| edit.placeholder == Some(node.key.as_str()))
            || drawn.hidden
            || clip[2] <= clip[0]
            || clip[3] <= clip[1]
            || (!floats && (dest[2] <= dest[0] || dest[3] <= dest[1]))
            || opacity <= 0.0
        {
            return Ok(());
        }
        let alpha = |color: [u8; 4]| {
            let a = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
            [color[0], color[1], color[2], a]
        };
        if let Draw::Text {
            text,
            color,
            shadow,
            align,
            scale,
            localize,
            options,
        } = &node.draw
        {
            let feedback = self
                .edit
                .filter(|edit| edit.text == node.key)
                .map(|edit| edit.feedback);
            let colored = self
                .art
                .text_color
                .and_then(|color| color(node))
                .map(|code| {
                    let text = if *localize {
                        text_paint::localized(text, self.translate)
                    } else {
                        Cow::Borrowed(text.as_str())
                    };
                    format!("§{code}{text}")
                });
            let (text, localize) = colored
                .as_deref()
                .map_or((text.as_str(), *localize), |text| (text, false));
            let style = TextPaint {
                edit: feedback,
                color: alpha(*color),
                shadow: if *shadow {
                    self.metrics.shadow()
                } else {
                    TextShadow::None
                },
                align: *align,
                scale: *scale,
                localize,
                options,
            };
            return self.text(text, dest, clip, style);
        }
        self.group(clip)?;
        let (visual, bounds) = match &node.draw {
            Draw::Solid { color } => (
                UiVisual::Solid {
                    texture_page: self.solid_page,
                    color: alpha(*color),
                },
                dest,
            ),
            Draw::Sprite {
                texture,
                uv,
                color,
                filter,
                ..
            } => {
                let uv = drawn.uv.unwrap_or(*uv);
                let color = drawn.color.unwrap_or(*color);
                if self.paint_native_sprite(
                    texture,
                    uv,
                    alpha(color),
                    *filter,
                    dest,
                    drawn.repeat,
                )? {
                    return Ok(());
                }
                let Some(visual) =
                    self.sprite_with_repeat(texture, uv, alpha(color), *filter, drawn.repeat)
                else {
                    return Ok(());
                };
                (visual, dest)
            }
            // Drawn above.
            Draw::Text { .. } => return Ok(()),
            Draw::Custom { renderer, data } if renderer == tooltip::RENDERER => {
                let text = self
                    .art
                    .tooltip
                    .or_else(|| data.get("#hover_text")?.as_str())
                    .filter(|text| !text.is_empty());
                return match text {
                    Some(text) => self.tooltip(text, data, dest, &alpha),
                    None => Ok(()),
                };
            }
            Draw::Custom { renderer, data } => {
                let custom = CustomDraw {
                    key: &node.key,
                    renderer,
                    data,
                    dest,
                    opacity,
                };
                return self.custom(&custom, renderers);
            }
        };
        self.push(visual, bounds)
    }
}
