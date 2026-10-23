//! Server-form presentation: the vanilla JSON-UI templates through the engine
//! when the UI carrier is loaded, else the programmatic fallback dialog.
#[cfg(test)]
mod chat_links_tests;
#[cfg(test)]
mod chat_position_tests;
pub mod chat_screen;
pub mod containers;
pub mod credits_screen;
mod debug_overlay;
#[cfg(test)]
mod disconnect_retry_tests;
pub mod emote_screen;
#[cfg(test)]
mod engine_tests;
pub mod fallback;
#[cfg(test)]
mod formatting_tests;
pub mod furnace_book;
#[cfg(test)]
mod global_resources_tests;
pub mod hud;
#[cfg(test)]
pub mod inbox_tests;
#[cfg(test)]
mod inventory_work_probe;
#[cfg(test)]
mod item_renderer_tests;
#[cfg(test)]
mod join_request_tests;
pub mod loading_screen;
#[cfg(test)]
pub mod loading_texture_tests;
pub(super) mod menu_caret;
#[cfg(test)]
pub mod menu_latency;
mod menu_title;
pub mod menus;
mod mod_surfaces;
pub mod model;
pub mod npc;
mod oreui_runtime;
#[cfg(test)]
mod oreui_tests;
pub(in crate::ui_runtime::presentation) mod preview_renderer;
#[cfg(test)]
mod retained_image_tests;
mod retained_menu;
#[cfg(test)]
mod session_renderer_tests;
pub(super) use retained_menu::RetainedMenu;
#[cfg(test)]
pub(crate) mod compatibility_tests;
#[cfg(test)]
mod font_snapshots;
#[cfg(any(test, feature = "test-support"))]
pub mod pack_harness;
#[cfg(test)]
mod publication_tests;
#[cfg(test)]
pub mod regression_snapshots;

use launcher::menu::settings_options::SettingId;
#[cfg(test)]
mod accounts;
#[cfg(test)]
pub mod play_flow_snapshots;
mod player_list;
pub mod recipe_book;
pub mod scene_policy;
mod screen_registry;
pub mod server_pack;
#[cfg(test)]
mod settings_reset_tests;
#[cfg(test)]
pub mod settings_snapshots;
#[cfg(test)]
pub mod settings_storage_support_tests;
pub mod sign_editor;
#[cfg(any(test, feature = "test-support"))]
pub mod snapshot;
#[cfg(test)]
#[cfg(test)]
pub mod tests;
#[cfg(not(target_arch = "wasm32"))]
mod texture_sources;
pub mod toast_screen;
mod touch_controls;
#[cfg(all(test, not(target_arch = "wasm32")))]
mod url_images_tests;

pub use containers::{beacon_pyramid_level, observe_station_block};

use super::{UiPresentationError, UiPresentationRuntime};
use crate::ui_runtime::scene_stack::ScreenSettingsTable;
use crate::ui_runtime::{LocalFormAction, ServerFormIdentity, UiRuntime};
use assets::RuntimeUiAssets;
pub use containers::{container_screen_reference, engine_screen_for};
pub(crate) use recipe_book::BookIconCache;
pub use recipe_book::{BookIconSources, recipe_book_hover, recipe_book_icons, recipe_book_shown};
use ui_engine::{
    engine::{FormEngine, PreparedCatalog, screen::PaintFrame},
    frame::EngineFrame,
    pages,
    text_metrics::TextMetrics,
};

pub use server_pack::ServerUiPack;
use std::sync::Arc;
use ui::{UiNode, UiPoint, UiRect};

/// Each screen family's own state, carried across builds.
#[derive(Default)]
pub(super) struct ScreenStates {
    /// The engine menu's regions by action, for next frame's hover state.
    menu_keys: Vec<(launcher::menu::MenuAction, String)>,
    /// The engine menu's press sounds by action.
    menu_sounds: Vec<(launcher::menu::MenuAction, json_ui::ControlSound)>,
    /// Where the installed catalog's title image draws from.
    menu_title: Option<menu_title::TitleSource>,
    /// The title splash, picked once per resource reload.
    splash: std::sync::OnceLock<Option<String>>,
    splash_sources: Arc<[ui_screens::menu_renderers::splashes::SplashSource]>,
    /// The menu clock when the splash first drew, which its pulse runs from.
    splash_epoch: std::sync::OnceLock<f64>,
    /// The form whose render path was last logged, so each form logs once.
    logged: Option<ServerFormIdentity>,
    /// The engine HUD's cached screens.
    hud: ui_screens::hud::HudScreens,
    player_list: Option<ui_screens::player_list::PlayerList>,
    /// Every mod- and experience-owned surface.
    pub(super) mod_ui: mod_ui::ModUi,
    /// The container screens' layout and recipe rows.
    containers: ui_screens::containers::ContainerScreen,
    /// The menu text caret's blink and its boxes' text.
    menu_caret: menu_caret::MenuCaretState,
    /// The open chat's cached screen.
    chat: ui_screens::chat_screen::ChatScreen,
    emote: ui_screens::emote_screen::EmoteScreen,
    /// The bed screen's hits and pointer.
    bed: oreui::BedScreen,
    /// The sign editor's cached screen.
    sign: ui_screens::sign_editor::SignScreen,
    credits: ui_screens::credits_screen::CreditsScreen,
    /// The credits' skip regions from their last paint.
    credits_hits: Vec<(ui_screens::credits_screen::CreditsAction, UiRect)>,
    /// OreUI's artwork, look and motion.
    oreui: oreui::OreUiScreen,
    touch: touch_controls::TouchControls,
}

/// What the last build exposes to input and the next build; reset as each build starts.
#[derive(Default)]
pub(super) struct FrameProjections {
    /// The server form drawn this build.
    pub(super) form: FormProjection,
    /// The container screen the engine drew this build, with its cell layout.
    container: Option<(EngineFrame, ui_screens::containers::ScreenLayout)>,
    /// Last build's container frame, for this build's pointer hover.
    previous_container: Option<(EngineFrame, ui_screens::containers::ScreenLayout)>,
    /// Keyboard controls include rows outside the pointer's clipped viewports.
    pub(super) menu_focus: Vec<launcher::menu::MenuAction>,
    /// Screen and popup context owning the painted menu controls.
    pub(super) menu_focus_context: Option<(launcher::menu::MenuScreen, bool)>,
    pub(super) menu_focus_geometry: Vec<launcher::menu::view::SettingsFocusTarget>,
    pub(super) menu_focus_landmarks: Vec<launcher::menu::view::SettingsFocusLandmark>,
    pub(super) oreui_slider_tracks: Vec<(launcher::menu::MenuAction, UiRect, Option<UiRect>)>,
    pub(super) oreui_settings_input: bool,
}

/// The server form a build drew: the fallback dialog's rows, or the engine frame.
#[derive(Default)]
pub(super) struct FormProjection {
    /// The form the fallback dialog drew.
    identity: Option<ServerFormIdentity>,
    pub(super) hits: Vec<(LocalFormAction, UiRect)>,
    pub(super) offsets: Vec<usize>,
    scroll: usize,
    height: usize,
    pub(super) maximum: usize,
    row_height: usize,
    /// The engine frame and the form it drew; input validates that identity.
    frame: Option<(ServerFormIdentity, EngineFrame)>,
}

/// A UI carrier whose catalog is parsed and prepared with the client's built-in screens.
pub struct PreparedJsonUi {
    assets: Arc<RuntimeUiAssets>,
    catalog: PreparedCatalog,
    item_glint: Option<Arc<render_model::UiTexturePage>>,
}

impl PreparedJsonUi {
    /// Parses every definition the carrier lists; needs no runtime, so startup runs it off the
    /// main thread.
    pub fn new(assets: Arc<RuntimeUiAssets>) -> Result<Self, String> {
        let item_glint = super::item_glint::carrier_texture(&assets);
        Self::with_item_glint(assets, item_glint)
    }

    /// Prepares a validated startup carrier without decoding its glint artwork again.
    pub fn from_loaded(
        loaded: crate::ui_runtime::json_ui_assets::LoadedUiAssets,
    ) -> Result<Self, String> {
        Self::with_item_glint(loaded.assets, Some(loaded.item_glint))
    }

    /// Parses the catalog while retaining artwork already decoded by the caller.
    fn with_item_glint(
        assets: Arc<RuntimeUiAssets>,
        item_glint: Option<Arc<render_model::UiTexturePage>>,
    ) -> Result<Self, String> {
        let catalog = json_ui::Catalog::from_files(
            assets
                .ui_files()
                .iter()
                .map(|file| (&*file.path, &*file.bytes)),
        )
        .map_err(|error| format!("ui catalog: {error}"))?;
        Ok(Self {
            item_glint,
            catalog: PreparedCatalog::new(catalog, screen_registry::registry()),
            assets,
        })
    }
}

impl UiPresentationRuntime {
    /// Shares immutable carrier definitions with the optional-pack reload worker.
    pub fn pack_catalog_base(&self) -> Option<Arc<json_ui::Catalog>> {
        self.engine
            .as_ref()
            .map(|engine| engine.pack_catalog_base())
    }

    /// Bind the compiled UI carrier: its atlas pages join the texture array and
    /// its catalog drives server forms. On failure the fallback dialog stays.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_json_ui(&mut self, assets: Arc<RuntimeUiAssets>) -> Result<(), String> {
        self.enable_prepared_json_ui(PreparedJsonUi::new(assets)?)
    }

    /// [`Self::enable_json_ui`] with the carrier's catalog already parsed.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn enable_prepared_json_ui(&mut self, prepared: PreparedJsonUi) -> Result<(), String> {
        self.enable_prepared_json_ui_with_sources(
            prepared,
            texture_sources::downloads(),
            texture_sources::image_work(),
        )
    }

    /// Binds verified UI data to the host's image source and bounded decode executor.
    pub fn enable_prepared_json_ui_with_sources(
        &mut self,
        prepared: PreparedJsonUi,
        downloads: Arc<dyn ui_engine::sources::RemoteSource>,
        work: ui_engine::sources::ImageWork,
    ) -> Result<(), String> {
        let PreparedJsonUi {
            assets,
            catalog,
            item_glint,
        } = prepared;
        let (textures, first_page) = pages::with_ui_pages(&self.textures, &assets)
            .map_err(|error| format!("ui atlas pages: {error}"))?;
        self.textures = Arc::new(textures);
        self.item_glint.base = item_glint;
        // Dynamic pages moved up; their references rebuild from the new start.
        self.preview_dirty = true;
        self.menu_artwork_dirty = true;
        self.rebuild_dynamic_textures();
        let mut engine = FormEngine::with_prepared(assets, catalog, first_page, downloads, work);
        engine.textures.server_page =
            (self.textures.dynamic_start() + pages::SERVER_UI_PAGE) as u16;
        self.engine = Some(Box::new(engine));
        self.engine_pack = None;
        // A new carrier reads its own credits files and splashes.
        self.screens.credits.forget_content();
        self.screens.splash = std::sync::OnceLock::new();
        self.refresh_menu_title();
        self.refresh_screen_settings();
        self.hud_frame.engine_containers = true;
        Ok(())
    }

    /// Overlay a joined server's resource-pack UI: its `ui/*.json` merge over the
    /// vanilla catalog layer by layer and its `textures/**` images shadow the
    /// carrier's from reserved dynamic pages, so the static texture identity the
    /// renderer pins never changes. An empty pack restores vanilla.
    pub fn set_server_ui_pack(&mut self, pack: &ServerUiPack) {
        self.item_glint.overlay = pack.item_glint.clone();
        let first = self.textures.dynamic_start() + pages::SERVER_UI_PAGE;
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        if self.screens.splash_sources != pack.splashes {
            self.screens.splash_sources = pack.splashes.clone();
            self.screens.splash = std::sync::OnceLock::new();
            self.screens.splash_epoch = std::sync::OnceLock::new();
        }
        engine.server_ui = pack.ui_layers.iter().any(|layer| !layer.is_empty());
        if let Some(catalog) = &pack.catalog {
            engine.install_pack_catalog(catalog.clone(), pack.builtin_screen_catalog.clone());
        } else {
            engine.set_server_pack(&pack.ui_layers);
        }
        // Palette-only reloads can leave every cached text node unchanged.
        self.last_frame = None;
        let atlas = ui_engine::atlas::ServerAtlas::new(
            &pack.textures,
            pack.view.clone(),
            pages::SERVER_UI_PAGES,
        )
        .with_preread(
            pack.catalog
                .as_ref()
                .and_then(ui_engine::prepared::take_textures)
                .as_deref(),
        );
        tracing::info!(
            layers = pack.ui_layers.len(),
            ui_files = pack.ui_layers.iter().map(Vec::len).sum::<usize>(),
            textures = pack.textures.len(),
            "server resource-pack UI applied to the form engine"
        );
        engine.set_server_atlas(atlas, first as u16);
        // A texture-only pack may retain the catalog while changing sprite
        // dimensions, UV metadata, or nine-slice borders used during layout.
        self.screens.hud.invalidate_textures();
        self.screens.containers.invalidate_textures();
        if let Some(settings) = pack
            .screen_settings
            .as_ref()
            .and_then(|settings| settings.for_inputs(engine.catalog(), engine.context()))
        {
            self.screen_settings = settings;
        } else {
            self.refresh_screen_settings();
        }
        self.refresh_menu_title();
        self.sync_server_ui_pages();
    }

    /// Draws oversized server textures from their server-page downscale again.
    #[cfg(test)]
    pub fn drop_full_res_art(&mut self) {
        if let Some(engine) = self.engine.as_mut() {
            engine.textures.set_full_res(Default::default());
        }
    }

    /// Points the engine's oversized server textures at their art-page copies.
    pub(super) fn refresh_full_res_art(&mut self) {
        let full_res = self
            .menu_artwork_set
            .oversized
            .iter()
            .filter_map(|(key, _)| {
                let art = format!("{}{key}", super::menu_artwork::SERVER_ART_PREFIX);
                Some((key.clone(), *self.menu_artwork.refs.get(&art)?))
            })
            .collect();
        if let Some(engine) = self.engine.as_mut() {
            engine.textures.set_full_res(full_res);
        }
    }

    /// Publishes completed original images before a saved menu frame can be reused.
    pub(super) fn poll_server_ui_images(&mut self) {
        if self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.poll_native_images())
        {
            self.retained_menu = None;
            self.screens.hud.invalidate_textures();
            self.screens.containers.invalidate_textures();
            self.sync_server_ui_pages();
        }
    }

    /// Hands changed server atlas pages to the dynamic texture pages; runs
    /// after the frame's screens drew, before the frame publishes.
    pub(super) fn sync_server_ui_pages(&mut self) {
        let server = self
            .engine
            .as_mut()
            .is_some_and(|engine| engine.take_server_pages().is_some());
        let changed = self.screens.mod_ui.refresh_experience_modal_pages()
            | self.screens.mod_ui.refresh_mod_screen_pages()
            | server;
        // Server textures too big for a server page draw from full-resolution art.
        let oversized = self.oversized_ui_textures();
        if !oversized.shares_storage(&self.menu_artwork_set.oversized) {
            let artwork_changed = !oversized.same(&self.menu_artwork_set.oversized);
            self.menu_artwork_set.oversized = oversized;
            if artwork_changed {
                self.menu_artwork_loader
                    .request(self.menu_artwork_set.clone());
            }
        }
        if changed {
            self.rebuild_dynamic_textures();
        }
    }

    /// Let forms draw vanilla images the UI carrier lacks: item textures from
    /// the item icon atlas already on the UI texture array, anything else read
    /// on demand from the local vanilla pack at `vanilla`.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_form_texture_fallbacks(
        &mut self,
        entities: &assets::RuntimeEntityAssets,
        vanilla: std::path::PathBuf,
    ) {
        self.set_form_texture_source(
            entities,
            Arc::new(texture_sources::VanillaPack::new(vanilla)),
        );
    }

    /// Supplies item atlas fallbacks and a host-owned source for additional pack textures.
    pub fn set_form_texture_source(
        &mut self,
        entities: &assets::RuntimeEntityAssets,
        source: Arc<dyn ui_engine::sources::LocalPack>,
    ) {
        let mut icons = std::collections::HashMap::new();
        for visual in entities.item_visuals() {
            let assets::ItemVisualDefinitionRoute::Sprite { texture } = visual.route else {
                continue;
            };
            let Some(source) = entities.sources().get(texture.source as usize) else {
                continue;
            };
            let path = source
                .path
                .rsplit_once('.')
                .map_or(&*source.path, |(stem, _)| stem);
            if let Some(icon) = self.item_icon(&visual.key.identifier, visual.key.metadata) {
                icons.entry(path.to_owned()).or_insert(icon);
            }
        }
        if let Some(engine) = self.engine.as_mut() {
            engine.textures.set_fallbacks(icons, source);
        }
    }

    /// Reads vanilla images the carrier lacks from the local pack at `vanilla`.
    #[cfg(test)]
    pub fn set_vanilla_texture_root(&mut self, vanilla: std::path::PathBuf) {
        if let Some(engine) = self.engine.as_mut() {
            engine.textures.set_fallbacks(
                Default::default(),
                Arc::new(texture_sources::VanillaPack::new(vanilla)),
            );
        }
    }

    /// Retire animation state no paint touched this frame, so a control that
    /// comes back starts its animations afresh.
    pub(super) fn end_animation_frame(&mut self) {
        if let Some(engine) = self.engine.as_ref() {
            engine.animator().end_frame();
        }
        self.screens.oreui.transitions.end_frame();
    }

    /// Refreshes screen preferences before painting, including while the menu is hidden.
    pub(super) fn configure_oreui_settings(&mut self) {
        if let Some(view) = &self.menu_view {
            self.screens
                .oreui
                .configure_accessibility(&view.settings_options);
            self.screens.oreui.dark_mode = view.settings_options.oreui_dark_mode();
            self.screens
                .oreui
                .transitions
                .configure_motion(view.settings_options.value(SettingId::ScreenAnimations) != 0);
        }
    }

    /// Drawn engine textures too big for a server page, for the art pages.
    pub(super) fn oversized_ui_textures(&self) -> ui_engine::atlas::OversizedTextures {
        self.engine
            .as_ref()
            .map_or_else(Default::default, |engine| engine.textures.oversized())
    }

    pub(super) fn server_ui_pages(&self) -> &[render_model::UiTexturePage] {
        self.engine
            .as_ref()
            .map_or(&[], |engine| &engine.server_pages)
    }

    /// Immutable original-image tiles owned by the active resource-pack namespace.
    pub(super) fn server_ui_sheet_pages(&self) -> &[render_model::UiTexturePage] {
        self.engine
            .as_ref()
            .map_or(&[], |engine| &engine.server_sheet_pages)
    }

    /// Applies the runtime's server UI pack when it changes identity.
    pub(super) fn observe_server_ui(&mut self, pack: Option<&Arc<ServerUiPack>>) {
        if self.engine.is_none() {
            return;
        }
        let same = match (&self.engine_pack, pack) {
            (Some(current), Some(next)) => Arc::ptr_eq(current, next),
            (None, None) => true,
            _ => false,
        };
        self.engine_pack = pack.cloned();
        if same {
            return;
        }
        self.screens.credits.forget_content();
        match pack {
            Some(pack) => self.set_server_ui_pack(&Arc::clone(pack)),
            None => self.set_server_ui_pack(&ServerUiPack::default()),
        }
    }

    /// The text color table resolved from the currently installed UI pack stack.
    pub(super) fn formatting_palette(&self) -> Option<&ui::FormattingPalette> {
        self.engine
            .as_ref()
            .map(|engine| &engine.formatting_palette)
    }

    /// The sound the engine menu's control for `action` plays when pressed.
    pub fn menu_sound(&self, action: launcher::menu::MenuAction) -> Option<&json_ui::ControlSound> {
        self.screens
            .menu_sounds
            .iter()
            .find(|(candidate, _)| *candidate == action)
            .map(|(_, sound)| sound)
    }

    /// The live catalog's screen settings; empty without the JSON-UI engine.
    pub fn screen_settings(&self) -> Arc<ScreenSettingsTable> {
        Arc::clone(&self.screen_settings)
    }

    /// Hands the runtime this frame's loading cover and the catalog's screen settings.
    pub fn publish_scene_inputs(&self, runtime: &mut UiRuntime) {
        runtime.observe_presentation(self.loading_stage, self.screen_settings());
    }

    fn refresh_screen_settings(&mut self) {
        if let Some(engine) = self.engine.as_deref() {
            self.screen_settings = Arc::new(ScreenSettingsTable::for_catalog(
                engine.catalog(),
                engine.context(),
            ));
        }
    }

    /// The engine forms' animator, which input fires button events into.
    pub fn form_animator(&self) -> Option<std::sync::MutexGuard<'_, json_ui::Animator>> {
        Some(self.engine.as_ref()?.animator())
    }

    /// The engine frame for `identity`, when the engine drew that form.
    pub fn form_engine_frame(&self, identity: ServerFormIdentity) -> Option<&EngineFrame> {
        self.projections
            .form
            .frame
            .as_ref()
            .filter(|(drawn, _)| *drawn == identity)
            .map(|(_, frame)| frame)
    }

    pub fn form_button_count(&self, identity: ServerFormIdentity) -> Option<usize> {
        (self.projections.form.identity == Some(identity))
            .then_some(self.projections.form.offsets.len().saturating_sub(1))
    }
    pub fn form_button_visible(&self, identity: ServerFormIdentity, index: usize) -> bool {
        self.projections.form.identity == Some(identity)
            && self
                .projections
                .form
                .hits
                .iter()
                .any(|(action, _)| *action == LocalFormAction::SubmitButton(index as u32))
    }
    pub fn hit_test_form(&self, point: UiPoint) -> Option<(ServerFormIdentity, LocalFormAction)> {
        let identity = self.projections.form.identity?;
        self.projections
            .form
            .hits
            .iter()
            .rev()
            .find_map(|(action, bounds)| bounds.contains(point).then_some((identity, *action)))
    }
    pub fn form_focus_scroll(&self, identity: ServerFormIdentity, index: usize) -> Option<usize> {
        let state = &self.projections.form;
        if state.identity != Some(identity) {
            return None;
        }
        let top = *state.offsets.get(index)?;
        let bottom = top.saturating_add(state.row_height);
        Some(
            if top < state.scroll {
                top
            } else if bottom > state.scroll + state.height {
                bottom.saturating_sub(state.height)
            } else {
                state.scroll
            }
            .min(state.maximum),
        )
    }

    /// Starts a build's projections; screen families keep their own state.
    pub(super) fn begin_form_frame(&mut self) {
        let projections = &mut self.projections;
        let previous_container = projections.container.take();
        let mut menu_focus_geometry = std::mem::take(&mut projections.menu_focus_geometry);
        let mut menu_focus_landmarks = std::mem::take(&mut projections.menu_focus_landmarks);
        let mut oreui_slider_tracks = std::mem::take(&mut projections.oreui_slider_tracks);
        menu_focus_geometry.clear();
        menu_focus_landmarks.clear();
        oreui_slider_tracks.clear();
        *projections = FrameProjections {
            previous_container,
            menu_focus_geometry,
            menu_focus_landmarks,
            oreui_slider_tracks,
            ..FrameProjections::default()
        };
        self.screens.mod_ui.begin_mod_screens_frame();
        self.screens.touch.begin_frame();
    }

    /// Draws the open container's engine screen.
    #[allow(
        clippy::too_many_arguments,
        reason = "Player authority is borrowed separately from UI state."
    )]
    pub(super) fn append_container_scene(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<(), UiPresentationError> {
        let previous = self.projections.previous_container.take();
        self.append_engine_container(
            player_runtime,
            runtime,
            previous.as_ref().map(|(frame, _)| frame),
            nodes,
            next,
            metrics,
            width,
            height,
        )?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_server_form(
        &mut self,
        runtime: &UiRuntime,
        registry: Option<&std::collections::BTreeMap<i32, protocol::ItemRegistryEntry>>,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        width: f32,
        height: f32,
    ) -> Result<(), UiPresentationError> {
        let Some(entry) = runtime.server_forms().active() else {
            return Ok(());
        };
        if let protocol::ServerFormModel::NpcDialogue(npc) = &entry.model
            && self.append_npc_dialogue(
                runtime,
                npc,
                entry.identity,
                nodes,
                next,
                metrics,
                width,
                height,
            )?
        {
            return Ok(());
        }
        let session_icons = &self.session_icons;
        let icon_catalog = self.icon_catalog.as_deref();
        let icon_refs = self.icon_refs.as_deref();
        let item_icon = |id, aux| {
            let entry = registry?.get(&id)?;
            super::session_icons::lookup_icon(
                session_icons,
                icon_catalog,
                icon_refs,
                &entry.identifier,
                aux,
            )
        };
        let mut items = ui_engine::engine::item_renderer::ItemIdAuxRenderer { icon: &item_icon };
        let reason = match self.engine.as_deref_mut() {
            None => "JSON-UI carrier not loaded".to_owned(),
            Some(renderer) => {
                let translate = |key: &str| runtime.translation(key);
                let state = runtime.server_forms().engine();
                let remote = renderer.textures.remote.clone();
                let images = |url: &str| remote.state(url);
                match model::engine_model(
                    &entry.model,
                    state,
                    runtime.session_id() != 0,
                    &translate,
                    &images,
                    &|text, budget| runtime.resolve_form_text(text, budget),
                ) {
                    None => "form kind has no engine template".to_owned(),
                    Some(form) => {
                        let rollback = (nodes.len(), *next);
                        let mut frame = PaintFrame {
                            layouts: &mut self.layouts,
                            font: &self.font,
                            metrics,
                            solid_page: self.solid_texture_page,
                            safe_area: self.safe_area,
                            content: [width, height],
                            translate: &translate,
                            language: runtime.text_generation(),
                            now: self.menu_seconds,
                            nodes: &mut *nodes,
                            next: &mut *next,
                            renderers: &mut [&mut items],
                        };
                        let catalog = renderer.catalog_label();
                        match renderer.render(&form, &state.view, &mut frame) {
                            Ok(Some(frame)) => {
                                self.projections.form.frame = Some((entry.identity, frame));
                                log_path(
                                    &mut self.screens.logged,
                                    entry.identity,
                                    "engine",
                                    &catalog,
                                );
                                return Ok(());
                            }
                            // A missing template or a node the tree rejects falls
                            // back to the programmatic dialog, not a blank screen.
                            Ok(None) => {
                                nodes.truncate(rollback.0);
                                *next = rollback.1;
                                format!("template did not resolve ({catalog})")
                            }
                            Err(error) => {
                                nodes.truncate(rollback.0);
                                *next = rollback.1;
                                format!("engine output rejected: {error}")
                            }
                        }
                    }
                }
            }
        };
        log_path(
            &mut self.screens.logged,
            entry.identity,
            "fallback",
            &reason,
        );
        self.append_fallback_form(runtime, nodes, next, metrics, width, height)
    }
}

/// Screens the host draws beyond [`json_ui::ENGINE_SCREENS`], for the scene settings.
pub fn host_screen_references() -> impl Iterator<Item = &'static str> {
    [
        ui_screens::sign_editor::SIGN_SCREEN,
        ui_screens::emote_screen::EMOTE_SCREEN,
        ui_screens::emote_screen::EMOTE_EQUIP_POPUP,
        ui_screens::npc::NPC_SCREEN,
        ui_screens::toast_screen::TOAST_SCREEN,
        ui_screens::credits_screen::CREDITS_SCREEN,
    ]
    .into_iter()
    .chain(ui_screens::loading_screen::LOADING_SCREENS)
}

/// Logs which path draws `identity`, once per form.
fn log_path(
    logged: &mut Option<ServerFormIdentity>,
    identity: ServerFormIdentity,
    path: &str,
    reason: &str,
) {
    if *logged != Some(identity) {
        *logged = Some(identity);
        tracing::info!(?identity, path, reason, "server form render path");
    }
}

impl UiPresentationRuntime {
    /// Borrows the loaded UI carrier for the app panorama adapter.
    pub fn ui_assets(&self) -> Option<&assets::RuntimeUiAssets> {
        self.engine.as_deref().map(|engine| engine.assets())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
