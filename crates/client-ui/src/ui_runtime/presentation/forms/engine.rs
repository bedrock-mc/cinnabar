//! Presents a server form through the clean-room JSON-UI engine: the vanilla
//! `ui/*.json` templates resolve against the compiled carrier's catalog, lay out
//! in virtual UI pixels, and their draw nodes become retained UI nodes over the
//! carrier's atlas pages. One virtual pixel is one GUI pixel of the HUD's scale
//! (needs native measurement against Bedrock's own scale-index rule).
mod scene_policy;
mod tooltip;

use std::{borrow::Borrow, cell::RefCell, sync::Arc};

use assets::{RuntimeFontCatalog, RuntimeUiAssets};
use json_ui::{
    Catalog, Context, DataSource, Draw, DrawNode, FormModel, FormRender, HitRegion, LayoutEnv,
    RectOut, ResolvedControl, ViewState, bind_form_over, render_bound_gated,
};
use ui::{SafeArea, TextLayoutCache, TextShadow, UiNode, UiNodeId, UiVisual};

use super::super::player_preview::PreviewView;
use super::super::{FONT_DESIGN_PIXEL_TEXELS, IconRef, TextMetrics, UiPresentationError, rect};

pub(super) mod credits_renderer;
mod fill_renderers;
mod formatting_colors;
mod hud_pack;
mod hud_target;
use ui::native_hud;
mod item_renderer;
mod menu_renderers;
mod menu_title;
#[cfg(test)]
mod ownership_tests;
mod pack_catalog;
mod pixel_snap;
mod rounded;
mod vector_icons;
pub(super) use pack_catalog::layer_pack_catalog;
pub(super) mod host_edit;
mod screen_cache;
mod text_paint;
use super::server_pack::{ServerAtlas, ServerUiPack};
use super::textures::{TextureSet, Textures};
use crate::ui_runtime::{
    ServerFormIdentity,
    forms::{EditText, EngineFrame},
};
use text_paint::{Measure, TextPaint};
pub(super) use text_paint::{UNWRAPPED_LOGICAL, active_codes, painted_label_request, width_64};

pub struct FormEngine {
    assets: Arc<RuntimeUiAssets>,
    /// The carrier's vanilla catalog, before the built-in Java HUD pack.
    vanilla: Arc<Catalog>,
    /// Vanilla under the built-in Java HUD pack: the catalog with no server pack.
    base: Arc<Catalog>,
    catalog: Arc<Catalog>,
    pub(super) formatting_palette: ui::FormattingPalette,
    context: Context,
    /// Where texture paths draw from, including the on-demand server atlas.
    pub(super) textures: TextureSet,
    /// The atlas page images last handed to the dynamic pages.
    pub(super) server_pages: Vec<render_model::UiTexturePage>,
    /// The runtime pack last applied, compared by identity.
    server_source: Option<Arc<ServerUiPack>>,
    menu_title_source: menu_title::TitleSource,
    /// The last form's bound tree and laid-out output, reused while unchanged.
    pub(super) cache: Option<FormCache>,
    /// Resolve+bind and layout passes run, for cache tests and profiling.
    pub(super) passes: [usize; 2],
    /// The title splash, picked once per launch.
    splash: std::sync::OnceLock<Option<String>>,
    credits: std::sync::OnceLock<Arc<super::credits_content::Content>>,
    screens: screen_cache::ScreenCache,
    /// Animation state of every drawn control, keyed by layout key.
    animator: std::sync::Mutex<json_ui::Animator>,
}

pub(super) struct FormCache {
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
    root: [f64; 2],
    px: f32,
    text: [usize; 3],
    font: assets::FontCatalogIdentity,
    render: FormRender,
}

/// Borrowed texture sources a paint reads.
#[derive(Clone, Copy)]
struct Art<'a> {
    assets: &'a RuntimeUiAssets,
    set: &'a TextureSet,
    animator: &'a std::sync::Mutex<json_ui::Animator>,
}

/// Everything a render borrows from the presentation runtime for one frame.
pub(super) struct EngineInputs<'a> {
    pub(super) layouts: &'a mut TextLayoutCache,
    pub(super) font: &'a RuntimeFontCatalog,
    pub(super) metrics: TextMetrics,
    pub(super) solid_page: u16,
    pub(super) safe_area: SafeArea,
    pub(super) content: [f32; 2],
    pub(super) translate: &'a dyn Fn(&str) -> Option<Arc<str>>,
    /// The language tables `translate` reads; a change measures text again.
    pub(super) language: [usize; 3],
}

impl FormEngine {
    pub(super) fn new(assets: Arc<RuntimeUiAssets>, mut catalog: Catalog, first_page: u16) -> Self {
        super::global_resources::extend_catalog(&mut catalog);
        super::credits_screen::extend_catalog(&mut catalog);
        let vanilla = Arc::new(catalog);
        let base = Arc::new(hud_pack::with_java_hud(&vanilla));
        let context = super::menu_screens::retail_context();
        let menu_title_source = menu_title::TitleSource::new(&base, &context);
        Self {
            textures: TextureSet::new(first_page).with_carrier(Arc::clone(&assets)),
            assets,
            catalog: Arc::clone(&base),
            formatting_palette: formatting_colors::from_catalog(&base),
            screens: screen_cache::ScreenCache::default(),
            vanilla,
            base,
            context,
            menu_title_source,
            server_pages: Vec::new(),
            server_source: None,
            cache: None,
            passes: [0; 2],
            splash: std::sync::OnceLock::new(),
            credits: std::sync::OnceLock::new(),
            animator: std::sync::Mutex::default(),
        }
    }

    fn art(&self) -> Art<'_> {
        Art {
            assets: &self.assets,
            set: &self.textures,
            animator: &self.animator,
        }
    }

    /// The animator every paint ticks: the dispatcher fires button events into
    /// it, drains its end and destroy events, and ends each presented frame.
    pub fn animator(&self) -> std::sync::MutexGuard<'_, json_ui::Animator> {
        self.animator
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Records `pack` as the applied source; `true` when it differs from the last.
    pub(super) fn take_server_source(&mut self, pack: Option<&Arc<ServerUiPack>>) -> bool {
        let same = match (&self.server_source, pack) {
            (Some(current), Some(next)) => Arc::ptr_eq(current, next),
            (None, None) => true,
            _ => false,
        };
        self.server_source = pack.cloned();
        if !same {
            self.credits = std::sync::OnceLock::new();
        }
        !same
    }

    /// Which catalog forms resolve against, for the render-path log.
    pub(super) fn catalog_label(&self) -> String {
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
    pub(super) fn set_server_atlas(&mut self, atlas: ServerAtlas, first: u16) {
        self.textures.set_atlas(atlas, first);
    }

    /// The atlas page images when they changed since the last call.
    pub(super) fn take_server_pages(&mut self) -> Option<&[render_model::UiTexturePage]> {
        let atlas = self.textures.atlas_mut();
        if !atlas.take_dirty() {
            return None;
        }
        self.server_pages = atlas.images().to_vec();
        Some(&self.server_pages)
    }

    /// The last form's sprite textures, and those resolving to no source.
    #[cfg(test)]
    pub(super) fn drawn_sprites(&self) -> (Vec<String>, Vec<String>) {
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

    /// Apply a server pack's ui files over vanilla and the Java HUD pack; none restores the base.
    pub(super) fn set_server_pack(&mut self, layers: &[Vec<(String, Vec<u8>)>]) {
        if layers.iter().all(Vec::is_empty) {
            self.install_pack_catalog(Arc::clone(&self.base));
            return;
        }
        self.install_pack_catalog(Arc::new(layer_pack_catalog(&self.vanilla, layers)));
    }

    /// Returns the original catalog used by background pack compilation.
    pub(super) fn pack_catalog_base(&self) -> Arc<Catalog> {
        self.vanilla.clone()
    }

    /// Publishes a worker-resolved catalog and retires caches holding the previous one.
    pub(super) fn install_pack_catalog(&mut self, catalog: Arc<Catalog>) {
        self.menu_title_source = menu_title::TitleSource::new(&catalog, &self.context);
        self.formatting_palette = formatting_colors::from_catalog(&catalog);
        self.catalog = catalog;
        self.cache = None;
        self.screens = screen_cache::ScreenCache::default();
    }

    /// Render `model`; `Ok(None)` without its template. Layout holds until model or scroll change.
    pub(super) fn render(
        &mut self,
        model: &FormModel,
        view: &ViewState,
        (identity, now): (ServerFormIdentity, f64),
        inputs: EngineInputs<'_>,
        out: EngineOutput<'_>,
    ) -> Result<Option<EngineFrame>, UiPresentationError> {
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
                bound: bind_form_over(model, &self.catalog, &self.context, components),
                laid: None,
                screen_cancel: json_ui::form_screen_cancel(&self.catalog),
            });
        }
        let px = inputs.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let text = inputs.language;
        let font = inputs.font.identity();
        let art = Art {
            assets: &self.assets,
            set: &self.textures,
            animator: &self.animator,
        };
        let screen_cancel = self
            .cache
            .as_ref()
            .and_then(|cache| cache.screen_cancel.clone());
        let (cache, passes) = (&mut self.cache, &mut self.passes[1]);
        let screen_art = ScreenArt {
            view: Some(view),
            now,
            ..ScreenArt::default()
        };
        let frame = render_with(
            art,
            inputs,
            out,
            screen_art,
            Some(identity),
            move |env, root| {
                let cache = cache.as_mut()?;
                let fresh = cache.laid.as_ref().is_some_and(|laid| {
                    laid.view.same_layout(view)
                        && (laid.root, laid.px, laid.text) == (root, px, text)
                        && laid.font == font
                });
                if !fresh {
                    let bound = cache.bound.clone()?;
                    *passes += 1;
                    let measures = &mut Default::default();
                    let render = render_bound_gated(bound, root, env, view, measures);
                    cache.laid = Some(LaidForm {
                        view: view.clone(),
                        root,
                        px,
                        text,
                        font,
                        render,
                    });
                }
                cache.laid.as_ref().map(|laid| &laid.render)
            },
        )?;
        Ok(frame.map(|mut frame| {
            frame.cancel_target = frame.cancel_target.or(screen_cancel);
            frame
        }))
    }

    pub(super) fn assets(&self) -> &RuntimeUiAssets {
        &self.assets
    }

    pub(super) fn splash(&self, translate: &dyn Fn(&str) -> Option<Arc<str>>) -> Option<&str> {
        self.splash
            .get_or_init(|| menu_renderers::pick_splash(&self.assets, translate))
            .as_deref()
    }

    pub(super) fn catalog(&self) -> &Arc<Catalog> {
        &self.catalog
    }

    /// Chat retains the built-in presentation while other screens use the pack stack.
    pub(super) fn screen_catalog(&self, reference: &str) -> &Arc<Catalog> {
        if reference == super::chat_screen::CHAT_SCREEN {
            &self.base
        } else {
            &self.catalog
        }
    }

    #[cfg(test)]
    pub(in crate::ui_runtime::presentation) fn set_native_chat_fixture(&mut self, native: bool) {
        self.base = if native {
            Arc::clone(&self.vanilla)
        } else {
            Arc::new(hud_pack::with_java_hud(&self.vanilla))
        };
    }

    pub(super) fn context(&self) -> &Context {
        &self.context
    }

    /// Paint what `draw` lays out over this engine's textures; `Ok(None)` when it lays out nothing.
    pub(super) fn draw<R: Borrow<FormRender>>(
        &self,
        art: ScreenArt<'_>,
        inputs: EngineInputs<'_>,
        out: EngineOutput<'_>,
        draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
    ) -> Result<Option<EngineFrame>, UiPresentationError> {
        render_with(self.art(), inputs, out, art, None, draw)
    }

    /// [`Self::draw`] over `textures` instead of the engine's own sources.
    pub(super) fn draw_with<R: Borrow<FormRender>>(
        &self,
        textures: &TextureSet,
        art: ScreenArt<'_>,
        inputs: EngineInputs<'_>,
        out: EngineOutput<'_>,
        draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
    ) -> Result<Option<EngineFrame>, UiPresentationError> {
        let sources = Art {
            assets: &self.assets,
            set: textures,
            animator: &self.animator,
        };
        render_with(sources, inputs, out, art, None, draw)
    }

    /// Render an allow-listed screen against `data` under `view`; `art` backs its custom renderers.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_screen<'a>(
        &self,
        reference: &str,
        data: &DataSource,
        context: &Context,
        view: &'a ViewState,
        mut art: ScreenArt<'a>,
        inputs: EngineInputs<'_>,
        out: EngineOutput<'_>,
    ) -> Result<Option<EngineFrame>, UiPresentationError> {
        art.view = Some(view);
        let px = inputs.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
        let text = inputs.language;
        render_with(self.art(), inputs, out, art, None, |env, root| {
            let key = screen_cache::ScreenKey {
                reference,
                catalog: &self.catalog,
                context,
                data,
                view,
                root,
                px,
                text,
            };
            self.screens.render(key, env)
        })
    }
}

fn render_with<R: Borrow<FormRender>>(
    textures: Art<'_>,
    inputs: EngineInputs<'_>,
    out: EngineOutput<'_>,
    art: ScreenArt<'_>,
    identity: Option<ServerFormIdentity>,
    draw: impl FnOnce(&LayoutEnv, [f64; 2]) -> Option<R>,
) -> Result<Option<EngineFrame>, UiPresentationError> {
    let px = inputs.metrics.scale.get() * FONT_DESIGN_PIXEL_TEXELS as f32;
    let root = [
        f64::from(inputs.content[0] / px),
        f64::from(inputs.content[1] / px),
    ];
    let cache = RefCell::new(inputs.layouts);
    let render = {
        let atlas = textures.set.lock();
        let view = Textures {
            assets: textures.assets,
            set: textures.set,
            atlas: &atlas,
            images: art.images,
        };
        let measure = Measure {
            layouts: &cache,
            font: inputs.font,
            metrics: inputs.metrics,
            px,
            translate: inputs.translate,
        };
        let env = LayoutEnv {
            text: &measure,
            textures: &view,
        };
        draw(&env, root)
    };
    let Some(render) = render else {
        return Ok(None);
    };
    let render = render.borrow();
    let layouts = cache.into_inner();
    let mut atlas = textures.set.lock();
    // Only what this screen draws needs to be resident.
    // Many nodes share a texture; each path resolves once.
    let paths: std::collections::HashSet<&str> = render
        .nodes
        .iter()
        .chain(out.overlay)
        .filter(|node| !art.omits(node))
        .filter_map(|node| match &node.draw {
            Draw::Sprite { texture, .. } => Some(texture.as_str()),
            Draw::Custom { renderer, .. } if renderer == tooltip::RENDERER => {
                Some(tooltip::BACKGROUND_TEXTURE)
            }
            Draw::Custom { renderer, .. } if renderer == credits_renderer::RENDERER => {
                Some(credits_renderer::TITLE_TEXTURE)
            }
            _ => None,
        })
        .chain(art.hud.into_iter().flat_map(native_hud::HudPaint::textures))
        .collect();
    let drawn = Textures {
        assets: textures.assets,
        set: textures.set,
        atlas: &atlas,
        images: art.images,
    }
    .atlas_keys(paths.into_iter());
    atlas.require(drawn.iter().map(String::as_str));
    let mut painter = Painter {
        textures: Textures {
            assets: textures.assets,
            set: textures.set,
            atlas: &atlas,
            images: art.images,
        },
        solid_page: inputs.solid_page,
        edit: host_edit::Target::from_frame(render, art),
        art,
        screen: [0.0, 0.0, inputs.content[0], inputs.content[1]],
        layouts,
        font: inputs.font,
        metrics: inputs.metrics,
        px,
        translate: inputs.translate,
        nodes: out.nodes,
        next: out.next,
        clip: None,
        animator: textures
            .animator
            .lock()
            .unwrap_or_else(|poison| poison.into_inner()),
    };
    let view = art.view;
    for node in render.nodes.iter().chain(out.overlay) {
        if !art.omits(node) && view.is_none_or(|view| node.shown(view)) {
            painter.paint(node)?;
        }
    }
    let origin = [inputs.safe_area.left(), inputs.safe_area.top()];
    Ok(Some(EngineFrame {
        identity,
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
            [inputs.metrics.gui_scale, px],
        ),
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

/// Caller art the custom renderers draw: `#item_renderer_data` icons, the player preview,
/// the tooltip pointer (virtual px), the fade clock (s), HUD state, artwork and gamerpic.
#[derive(Clone, Copy, Default)]
pub(super) struct ScreenArt<'a> {
    /// Native replacements retain the pack's backdrop while omitting the replaced controls.
    pub(super) omit_controls: &'a [&'a str],
    pub(super) icons: &'a [IconRef],
    /// Icons an `#item_id_aux` renderer names, by that value.
    pub(super) id_aux: &'a [(i64, IconRef)],
    /// The interaction state gated nodes ([`json_ui::render_bound_gated`]) paint under.
    pub(super) view: Option<&'a ViewState>,
    /// Text a shown hover tooltip draws instead of its bound `#hover_text`.
    pub(super) tooltip: Option<&'a str>,
    /// Where a drawn player renderer records how it wants the model posed.
    pub(super) preview_view: Option<&'a std::cell::Cell<Option<PreviewView>>>,
    pub(super) preview_control: Option<
        &'a std::cell::Cell<Option<super::super::player_preview::controller::PreviewControl>>,
    >,
    pub(super) preview_rotation: f32,
    pub(super) preview: Option<IconRef>,
    pub(super) pointer: Option<[f32; 2]>,
    pub(super) now: f64,
    /// Creation times that fades naming a clock read instead of their own.
    pub(super) clocks: Option<&'a std::collections::BTreeMap<String, f64>>,
    pub(super) hud: Option<&'a native_hud::HudPaint>,
    pub(super) images: Option<&'a std::collections::HashMap<String, IconRef>>,
    pub(super) portrait: Option<IconRef>,
    pub(super) splash: Option<&'a str>,
    pub(super) credits: Option<&'a super::credits_screen::CreditsPaint>,
    pub(super) edit: Option<host_edit::Feedback>,
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

/// Where a render writes its retained nodes, plus caller nodes painted on top (the held stack).
pub(super) struct EngineOutput<'a> {
    pub(super) nodes: &'a mut Vec<UiNode>,
    pub(super) next: &'a mut u32,
    pub(super) overlay: &'a [DrawNode],
}

/// Turns draw nodes into retained UI nodes, opening a clip group per clip change to keep order.
struct Painter<'a> {
    edit: Option<host_edit::Target<'a>>,
    textures: Textures<'a>,
    solid_page: u16,
    art: ScreenArt<'a>,
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
}

impl Painter<'_> {
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
        pixel_snap::snapped(
            [rect.x, rect.y, rect.w, rect.h],
            self.metrics.gui_scale,
            self.px,
        )
    }

    /// `rect` as logical bounds moved to a whole physical pixel, where vanilla
    /// places text ([`pixel_snap`]).
    fn positioned(&self, rect: &RectOut) -> [f32; 4] {
        pixel_snap::positioned(
            [rect.x, rect.y, rect.w, rect.h],
            self.metrics.gui_scale,
            self.px,
        )
    }

    fn id(&mut self) -> UiNodeId {
        let id = UiNodeId::new(*self.next);
        *self.next = self.next.saturating_add(1);
        id
    }

    /// The clip group for `clip`, reusing the current one when it matches.
    fn group(&mut self, clip: [f32; 4]) -> Result<UiNodeId, UiPresentationError> {
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

    /// Bridge the custom renderers screens use: item icons, the durability bar, the
    /// player preview, tooltips, and the HUD's native renderers. Others draw nothing yet.
    fn custom(
        &mut self,
        renderer: &str,
        data: &std::collections::BTreeMap<String, serde_json::Value>,
        dest: [f32; 4],
        alpha: impl Fn([u8; 4]) -> [u8; 4],
    ) -> Option<(UiVisual, [f32; 4])> {
        if let Some(hud) = self.art.hud
            && native_hud::paint(
                self,
                hud,
                renderer,
                data.get("#collection_index")
                    .and_then(serde_json::Value::as_f64)
                    .map_or(0, |index| index.clamp(0.0, 8.0) as usize),
                data.get("#bar_notches")
                    .and_then(serde_json::Value::as_f64)
                    .map_or(0, |notches| notches.clamp(0.0, 64.0) as u32),
                dest,
                &alpha,
            )
        {
            return None;
        }
        match renderer {
            "cinnabar_vector_icon" => Some((self.vector_icon(data, dest, &alpha)?, dest)),
            "cinnabar_rounded_rectangle" => {
                Some((self.rounded_rectangle(data, dest, &alpha)?, dest))
            }
            credits_renderer::RENDERER => {
                self.credits(dest, &alpha);
                None
            }
            "inventory_item_renderer" => {
                let icon = item_renderer::icon(data, self.art.icons, self.art.id_aux)?;
                Some((icon.visual(alpha([255; 4])), dest))
            }
            "progress_bar_renderer" => {
                self.progress_bar(data, dest, &alpha);
                None
            }
            "gradient_renderer" => Some((self.gradient(data, &alpha)?, dest)),
            "animated_gif_renderer" => self.animated_gif(data, dest, &alpha),
            "profile_image_renderer" => {
                // A friend row names its own gamerpic; elsewhere it is the player's.
                let portrait = match data.get("#profile_image_options") {
                    Some(serde_json::Value::String(path)) => *self.art.images?.get(path)?,
                    _ => self.art.portrait?,
                };
                Some((
                    UiVisual::Sprite {
                        texture_page: portrait.page,
                        uv: portrait.uv,
                        color: alpha([255; 4]),
                    },
                    dest,
                ))
            }
            "live_player_renderer" | "paper_doll_renderer" | "hud_player_renderer" => {
                self.player_preview(renderer, data, dest, &alpha)
            }
            "splash_text_renderer" => {
                self.splash(dest, &alpha);
                None
            }
            "name_tag_renderer" => self.name_tag(data, dest, &alpha),
            _ => None,
        }
    }

    /// A sprite of the texture at `path` (server pack first, then the carrier),
    /// sampling the normalised `uv`; `None` when neither holds it.
    fn sprite(
        &self,
        path: &str,
        uv: json_ui::UvRect,
        color: [u8; 4],
        filter: json_ui::SpriteFilter,
    ) -> Option<UiVisual> {
        let Some((page, [x, y, w, h])) = self.textures.sprite(path) else {
            // An unresolved texture draws vanilla's default white texture.
            return self.textures.missing(path).then_some(UiVisual::Solid {
                texture_page: self.solid_page,
                color,
            });
        };
        let pixel = |base: f32, span: f32, t: f32| (base + span * t).round() as u16;
        let uv = [
            pixel(x, w, uv.u0),
            pixel(y, h, uv.v0),
            pixel(x, w, uv.u1),
            pixel(y, h, uv.v1),
        ];
        let style = (u8::from(filter.grayscale) * ui::UI_STYLE_GRAYSCALE)
            | (u8::from(filter.bilinear) * ui::UI_STYLE_BILINEAR);
        Some(if style == 0 {
            UiVisual::Sprite {
                texture_page: page,
                uv,
                color,
            }
        } else {
            UiVisual::StyledSprite {
                texture_page: page,
                uv,
                color,
                style,
            }
        })
    }

    /// Push `visual` at `bounds` into the current clip group.
    fn push(&mut self, visual: UiVisual, bounds: [f32; 4]) -> Result<(), UiPresentationError> {
        let Some((clip, parent)) = self.clip else {
            return Ok(());
        };
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
    fn solid(&mut self, bounds: [f32; 4], color: [u8; 4]) -> Result<(), UiPresentationError> {
        let visual = UiVisual::Solid {
            texture_page: self.solid_page,
            color,
        };
        self.push(visual, bounds)
    }

    fn paint(&mut self, node: &DrawNode) -> Result<(), UiPresentationError> {
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
            && self.textures.animation_sprite(texture).is_none()
        {
            return Ok(());
        }
        let drawn = node.animate(
            &mut self.animator,
            self.art.now,
            self.art.clocks,
            Some(&self.textures),
        );
        let clip = self.logical(&drawn.clip);
        let dest = if matches!(node.draw, Draw::Text { .. }) {
            self.positioned(&drawn.dest)
        } else {
            self.snapped(&drawn.dest)
        };
        let opacity = drawn.opacity;
        if self
            .edit
            .is_some_and(|edit| edit.placeholder == Some(node.key.as_str()))
            || drawn.hidden
            || clip[2] <= clip[0]
            || clip[3] <= clip[1]
            || dest[2] <= dest[0]
            || dest[3] <= dest[1]
            || opacity <= 0.0
        {
            return Ok(());
        }
        let alpha = |color: [u8; 4]| {
            let a = (f32::from(color[3]) * opacity.clamp(0.0, 1.0)).round() as u8;
            [color[0], color[1], color[2], a]
        };
        // Tooltips ignore the hovered control's clip.
        let clip = match &node.draw {
            Draw::Custom { renderer, .. } if renderer == tooltip::RENDERER => self.screen,
            _ => clip,
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
                localize: *localize,
                options: options.clone(),
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
            } => {
                let uv = drawn.uv.unwrap_or(*uv);
                let color = drawn.color.unwrap_or(*color);
                let Some(visual) = self.sprite(texture, uv, alpha(color), *filter) else {
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
            Draw::Custom { renderer, data } => match self.custom(renderer, data, dest, alpha) {
                Some(visual) => visual,
                None => return Ok(()),
            },
        };
        self.push(visual, bounds)
    }
}
