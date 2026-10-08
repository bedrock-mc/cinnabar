//! Server-form presentation: the vanilla JSON-UI templates through the engine
//! when the UI carrier is loaded, else the programmatic fallback dialog.
pub mod book_screen;
pub mod chat_coordinates;
mod chat_link_dialog;
mod chat_links;
mod chat_position;
pub mod chat_screen;
pub mod container_data;
pub mod container_kinds;
mod debug_overlay;
pub mod discord_presence_setting;
pub(super) use container_kinds::supported_storage_slots;
pub mod containers;
pub(super) mod credits_content;
pub mod credits_screen;
pub mod crosshair_settings;
pub mod emote_screen;
pub mod engine;
pub mod experience;
pub mod experience_modal;
pub mod fallback;
#[cfg(test)]
mod formatting_tests;
pub mod global_resources;
pub mod hud;
#[cfg(test)]
pub mod inbox_tests;
mod invite_screen;
pub mod java_animations_setting;
pub mod join_progress;
#[cfg(test)]
mod join_request_tests;
pub mod loading_screen;
#[cfg(test)]
pub mod loading_texture_tests;
pub(super) mod menu_caret;
#[cfg(test)]
pub mod menu_latency;
pub mod menu_screens;
pub mod menus;
pub mod mod_hud;
pub mod mod_hud_editor;
pub mod mod_panel;
pub mod mod_widgets;
pub mod model;
pub mod motion_blur_setting;
pub mod npc;
pub mod oreui;
mod retained_menu;
pub(super) use retained_menu::RetainedMenu;
#[cfg(any(test, feature = "test-support"))]
pub mod pack_harness;
pub mod pages;
pub mod panorama;
#[cfg(test)]
mod publication_tests;
#[cfg(test)]
pub mod regression_snapshots;
pub use panorama::{built_in_faces, launcher_view};
mod accounts;
pub mod always_sprint_setting;
pub mod enhanced_setting;
pub mod graphics_expander;
#[cfg(test)]
pub mod play_flow_snapshots;
pub mod play_screen;
mod player_list;
pub mod recipe_book;
pub mod remote_images;
pub mod scene_policy;
pub mod server_pack;
pub mod settings_account;
pub mod settings_chat;
pub mod settings_controls;
pub mod settings_defaults;
pub mod settings_keys;
pub mod settings_language;
pub mod settings_reset;
pub mod settings_resources;
#[cfg(test)]
pub mod settings_snapshots;
pub mod settings_storage;
#[cfg(test)]
pub mod settings_storage_support_tests;
pub mod settings_support;
pub mod sign_editor;
#[cfg(any(test, feature = "test-support"))]
pub mod snapshot;
pub mod start_feed;
#[cfg(test)]
mod store_tests;
#[cfg(test)]
pub mod tests;
pub mod textures;
pub mod toast_screen;
pub mod vsync_setting;

pub use chat_screen::{CHAT_SCREEN, ChatHit};
pub use container_data::observe_station_block;
pub use emote_screen::{EMOTE_EQUIP_POPUP, EMOTE_SCREEN, EmoteHit};
pub use experience_modal::ExperienceModal;
pub use loading_screen::{LOADING_SCREEN, LoadingStage};
pub use menu_screens::menu_reference;
pub use npc::NPC_SCREEN;
pub use oreui::BedHit;
pub use sign_editor::SIGN_SCREEN;

use super::{TextMetrics, UiPresentationError, UiPresentationRuntime, dynamic_textures};
use crate::ui_runtime::scene_stack::ScreenSettingsTable;
use crate::ui_runtime::{LocalFormAction, ServerFormIdentity, UiRuntime, forms::EngineFrame};
use assets::RuntimeUiAssets;
pub use containers::{container_screen_reference, engine_panel_contains, engine_screen_for};
pub use engine::hud_renderers;
pub use recipe_book::{recipe_book_hover, recipe_book_icons, recipe_book_shown};
pub use server_pack::{MAX_PACK_TEXTURE_BYTES, ServerUiPack};
use std::sync::Arc;
use ui::{UiNode, UiPoint, UiRect};

#[derive(Default)]
pub(super) struct FormPresentation {
    identity: Option<ServerFormIdentity>,
    pub(super) hits: Vec<(LocalFormAction, UiRect)>,
    pub(super) offsets: Vec<usize>,
    scroll: usize,
    height: usize,
    pub(super) maximum: usize,
    row_height: usize,
    /// The engine frame drawn this build, when the engine drew the form.
    frame: Option<EngineFrame>,
    /// The JSON-UI engine; carried across the per-frame reset.
    engine: Option<Box<engine::FormEngine>>,
    /// The container screen the engine drew this build, with its cell layout.
    container: Option<(EngineFrame, containers::ScreenLayout)>,
    /// The engine menu's regions by action, for next frame's hover state.
    menu_keys: Vec<(crate::menu::MenuAction, String)>,
    /// Keyboard controls include rows outside the pointer's clipped viewports.
    pub(super) menu_focus: Vec<crate::menu::MenuAction>,
    pub(super) menu_focus_geometry: Vec<crate::menu::view::SettingsFocusTarget>,
    pub(super) menu_focus_landmarks: Vec<crate::menu::view::SettingsFocusLandmark>,
    /// The engine menu's press sounds by action; carried across the per-frame reset.
    menu_sounds: Vec<(crate::menu::MenuAction, json_ui::ControlSound)>,
    /// The form whose render path was last logged, so each form logs once.
    logged: Option<ServerFormIdentity>,
    /// The engine HUD's cached screens; carried across the per-frame reset.
    hud: hud::HudScreens,
    mod_hud: Option<mod_hud::ModHud>,
    mod_hud_editor: Option<mod_hud_editor::HudEditor>,
    mod_widgets: Option<mod_widgets::ModWidgets>,
    mod_crosshair: Option<ui::mod_hud::Crosshair>,
    player_list: Option<player_list::PlayerList>,
    mod_panel: Option<mod_panel::ModPanel>,
    experience: Option<experience::ExperienceChrome>,
    /// A client part's modal screen; carried across the per-frame reset.
    experience_modal: Option<experience_modal::ModalScreen>,
    /// The last container screen's layout; carried across the per-frame reset.
    container_cache: Option<containers::ScreenCache>,
    /// Immutable creative rows reused across hover and scroll frames.
    book_cache: Option<recipe_book::BookCache>,
    /// The menu text caret's blink and its boxes' text; carried across the per-frame reset.
    menu_caret: menu_caret::MenuCaretState,
    /// The open chat's cached screen; carried across the per-frame reset.
    chat: chat_screen::ChatScreen,
    emote: emote_screen::EmoteScreen,
    /// The bed screen's hits and pointer; carried across the per-frame reset.
    bed: oreui::BedScreen,
    /// The sign editor's cached screen; carried across the per-frame reset.
    sign: sign_editor::SignScreen,
    credits: credits_screen::CreditsScreen,
    /// Dev-mode OreUI originals and the look OreUI screens draw with.
    oreui_originals: Option<Arc<oreui::Originals>>,
    oreui_look: oreui::Look,
    oreui_dark_mode: bool,
    oreui_transitions: oreui::Transitions,
    pub(super) oreui_slider_tracks: Vec<(u16, UiRect, Option<UiRect>)>,
    pub(super) oreui_settings_input: bool,
    /// The engine catalog's screen settings; carried across the per-frame reset.
    screen_settings: Arc<ScreenSettingsTable>,
    /// Last build's container frame, for this build's pointer hover.
    previous_container: Option<(EngineFrame, containers::ScreenLayout)>,
}

impl UiPresentationRuntime {
    /// Shares immutable carrier definitions with the optional-pack reload worker.
    pub fn pack_catalog_base(&self) -> Option<Arc<json_ui::Catalog>> {
        self.form_presentation
            .engine
            .as_ref()
            .map(|engine| engine.pack_catalog_base())
    }

    /// Bind the compiled UI carrier: its atlas pages join the texture array and
    /// its catalog drives server forms. On failure the fallback dialog stays.
    pub fn enable_json_ui(&mut self, assets: Arc<RuntimeUiAssets>) -> Result<(), String> {
        let catalog = json_ui::Catalog::from_files(
            assets
                .ui_files()
                .iter()
                .map(|file| (&*file.path, &*file.bytes)),
        )
        .map_err(|error| format!("ui catalog: {error}"))?;
        let (textures, first_page) = pages::with_ui_pages(&self.textures, &assets)
            .map_err(|error| format!("ui atlas pages: {error}"))?;
        self.textures = Arc::new(textures);
        // Dynamic pages moved up; their references rebuild from the new start.
        self.preview_dirty = true;
        self.menu_artwork_dirty = true;
        self.rebuild_dynamic_textures();
        let mut engine = engine::FormEngine::new(assets, catalog, first_page);
        engine.textures.server_page =
            (self.textures.dynamic_start() + dynamic_textures::SERVER_UI_PAGE) as u16;
        self.form_presentation.engine = Some(Box::new(engine));
        self.refresh_screen_settings();
        self.hud_frame.engine_containers = true;
        Ok(())
    }

    /// Overlay a joined server's resource-pack UI: its `ui/*.json` merge over the
    /// vanilla catalog layer by layer and its `textures/**` images shadow the
    /// carrier's from reserved dynamic pages, so the static texture identity the
    /// renderer pins never changes. An empty pack restores vanilla.
    pub fn set_server_ui_pack(&mut self, pack: &ServerUiPack) {
        let first = self.textures.dynamic_start() + dynamic_textures::SERVER_UI_PAGE;
        let Some(engine) = self.form_presentation.engine.as_mut() else {
            return;
        };
        if let Some(catalog) = &pack.catalog {
            engine.install_pack_catalog(catalog.clone());
        } else {
            engine.set_server_pack(&pack.ui_layers);
        }
        // Palette-only reloads can leave every cached text node unchanged.
        self.last_frame = None;
        let atlas = server_pack::ServerAtlas::new(
            &pack.textures,
            pack.view.clone(),
            dynamic_textures::SERVER_UI_PAGES,
        );
        bevy::log::info!(
            layers = pack.ui_layers.len(),
            ui_files = pack.ui_layers.iter().map(Vec::len).sum::<usize>(),
            textures = pack.textures.len(),
            "server resource-pack UI applied to the form engine"
        );
        engine.set_server_atlas(atlas, first as u16);
        // A texture-only pack may retain the catalog while changing sprite
        // dimensions, UV metadata, or nine-slice borders used during layout.
        self.form_presentation.hud.invalidate_textures();
        if let Some(settings) = pack
            .screen_settings
            .as_ref()
            .and_then(|settings| settings.for_inputs(engine.catalog(), engine.context()))
        {
            self.form_presentation.screen_settings = settings;
        } else {
            self.refresh_screen_settings();
        }
        self.sync_server_ui_pages();
    }

    /// Draws oversized server textures from their server-page downscale again.
    #[cfg(test)]
    pub fn drop_full_res_art(&mut self) {
        if let Some(engine) = self.form_presentation.engine.as_mut() {
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
        if let Some(engine) = self.form_presentation.engine.as_mut() {
            engine.textures.set_full_res(full_res);
        }
    }

    /// Hands changed server atlas pages to the dynamic texture pages; runs
    /// after the frame's screens drew, before the frame publishes.
    pub(super) fn sync_server_ui_pages(&mut self) {
        let server = self
            .form_presentation
            .engine
            .as_mut()
            .is_some_and(|engine| engine.take_server_pages().is_some());
        let changed = self.refresh_experience_modal_pages() | server;
        // Server textures too big for a server page draw from full-resolution art.
        let set = super::menu_artwork::ArtworkSet {
            paths: self.menu_artwork_set.paths.clone(),
            oversized: self.oversized_ui_textures(),
            skins: self.menu_artwork_set.skins.clone(),
            capes: self.menu_artwork_set.capes.clone(),
        };
        if !set.same(&self.menu_artwork_set) {
            self.menu_artwork_set = set.clone();
            self.menu_artwork_loader.request(set);
        }
        if changed {
            self.rebuild_dynamic_textures();
        }
    }

    /// Let forms draw vanilla images the UI carrier lacks: item textures from
    /// the item icon atlas already on the UI texture array, anything else read
    /// on demand from the local vanilla pack at `vanilla`.
    pub fn set_form_texture_fallbacks(
        &mut self,
        entities: &assets::RuntimeEntityAssets,
        vanilla: std::path::PathBuf,
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
        if let Some(engine) = self.form_presentation.engine.as_mut() {
            engine.textures.set_fallbacks(icons, vanilla);
        }
    }

    /// Reads vanilla images the carrier lacks from the local pack at `vanilla`.
    #[cfg(test)]
    pub fn set_vanilla_texture_root(&mut self, vanilla: std::path::PathBuf) {
        if let Some(engine) = self.form_presentation.engine.as_mut() {
            engine.textures.set_fallbacks(Default::default(), vanilla);
        }
    }

    /// Retire animation state no paint touched this frame, so a control that
    /// comes back starts its animations afresh.
    pub(super) fn end_animation_frame(&mut self) {
        if let Some(engine) = self.form_presentation.engine.as_ref() {
            engine.animator().end_frame();
        }
        self.form_presentation.oreui_transitions.end_frame();
    }

    pub(super) fn configure_oreui_motion(&mut self) {
        if let Some(view) = &self.menu_view {
            self.form_presentation.oreui_dark_mode = view.settings_options.oreui_dark_mode();
            self.form_presentation
                .oreui_transitions
                .configure_motion(view.settings_options.value("screen_animations") != 0);
        }
    }

    /// Drawn engine textures too big for a server page, for the art pages.
    pub(super) fn oversized_ui_textures(&self) -> Vec<(String, Arc<[u8]>)> {
        self.form_presentation
            .engine
            .as_ref()
            .map_or_else(Vec::new, |engine| engine.textures.oversized())
    }

    pub(super) fn server_ui_pages(&self) -> &[render_model::UiTexturePage] {
        self.form_presentation
            .engine
            .as_ref()
            .map_or(&[], |engine| &engine.server_pages)
    }

    /// Applies the runtime's server UI pack when it changes identity.
    pub(super) fn observe_server_ui(&mut self, pack: Option<&Arc<ServerUiPack>>) {
        let Some(engine) = self.form_presentation.engine.as_mut() else {
            return;
        };
        if !engine.take_server_source(pack) {
            return;
        }
        match pack {
            Some(pack) => self.set_server_ui_pack(&Arc::clone(pack)),
            None => self.set_server_ui_pack(&ServerUiPack::default()),
        }
    }

    /// The text color table resolved from the currently installed UI pack stack.
    pub(super) fn formatting_palette(&self) -> Option<&ui::FormattingPalette> {
        self.form_presentation
            .engine
            .as_ref()
            .map(|engine| &engine.formatting_palette)
    }

    /// The sound the engine menu's control for `action` plays when pressed.
    pub fn menu_sound(&self, action: crate::menu::MenuAction) -> Option<&json_ui::ControlSound> {
        self.form_presentation
            .menu_sounds
            .iter()
            .find(|(candidate, _)| *candidate == action)
            .map(|(_, sound)| sound)
    }

    /// The live catalog's screen settings; empty without the JSON-UI engine.
    pub fn screen_settings(&self) -> Arc<ScreenSettingsTable> {
        Arc::clone(&self.form_presentation.screen_settings)
    }

    /// Hands the runtime this frame's loading cover and the catalog's screen settings.
    pub fn publish_scene_inputs(&self, runtime: &mut UiRuntime) {
        runtime.observe_presentation(self.loading_stage.is_some(), self.screen_settings());
    }

    fn refresh_screen_settings(&mut self) {
        if let Some(engine) = self.form_presentation.engine.as_deref() {
            self.form_presentation.screen_settings = Arc::new(ScreenSettingsTable::for_catalog(
                engine.catalog(),
                engine.context(),
            ));
        }
    }

    /// The engine frame for `identity`, when the engine drew that form.
    /// The engine forms' animator, which input fires button events into.
    pub fn form_animator(&self) -> Option<std::sync::MutexGuard<'_, json_ui::Animator>> {
        Some(self.form_presentation.engine.as_ref()?.animator())
    }

    pub fn form_engine_frame(&self, identity: ServerFormIdentity) -> Option<&EngineFrame> {
        self.form_presentation
            .frame
            .as_ref()
            .filter(|frame| frame.identity == Some(identity))
    }

    pub fn form_button_count(&self, identity: ServerFormIdentity) -> Option<usize> {
        (self.form_presentation.identity == Some(identity))
            .then_some(self.form_presentation.offsets.len().saturating_sub(1))
    }
    pub fn form_button_visible(&self, identity: ServerFormIdentity, index: usize) -> bool {
        self.form_presentation.identity == Some(identity)
            && self
                .form_presentation
                .hits
                .iter()
                .any(|(action, _)| *action == LocalFormAction::SubmitButton(index as u32))
    }
    pub fn hit_test_form(&self, point: UiPoint) -> Option<(ServerFormIdentity, LocalFormAction)> {
        let identity = self.form_presentation.identity?;
        self.form_presentation
            .hits
            .iter()
            .rev()
            .find_map(|(action, bounds)| bounds.contains(point).then_some((identity, *action)))
    }
    pub fn form_focus_scroll(&self, identity: ServerFormIdentity, index: usize) -> Option<usize> {
        let state = &self.form_presentation;
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

    /// Starts a build's form state, keeping what is carried across builds.
    pub(super) fn begin_form_frame(&mut self) {
        let previous_container = self.form_presentation.container.take();
        self.form_presentation.oreui_slider_tracks.clear();
        self.form_presentation.menu_focus_geometry.clear();
        self.form_presentation.menu_focus_landmarks.clear();
        let state = std::mem::take(&mut self.form_presentation);
        self.form_presentation = FormPresentation {
            engine: state.engine,
            menu_keys: state.menu_keys,
            menu_sounds: state.menu_sounds,
            logged: state.logged,
            hud: state.hud,
            mod_hud: state.mod_hud,
            mod_hud_editor: state.mod_hud_editor,
            mod_widgets: state.mod_widgets,
            mod_crosshair: state.mod_crosshair,
            player_list: state.player_list,
            mod_panel: state.mod_panel,
            experience: state.experience,
            experience_modal: state.experience_modal,
            container_cache: state.container_cache,
            book_cache: state.book_cache,
            menu_caret: state.menu_caret,
            chat: state.chat,
            emote: state.emote,
            bed: state.bed,
            sign: state.sign,
            credits: state.credits,
            oreui_originals: state.oreui_originals,
            oreui_look: state.oreui_look,
            oreui_dark_mode: state.oreui_dark_mode,
            oreui_transitions: state.oreui_transitions,
            oreui_slider_tracks: state.oreui_slider_tracks,
            menu_focus_geometry: state.menu_focus_geometry,
            menu_focus_landmarks: state.menu_focus_landmarks,
            screen_settings: state.screen_settings,
            previous_container,
            ..FormPresentation::default()
        };
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
        let previous = self.form_presentation.previous_container.take();
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
        let reason = match self.form_presentation.engine.as_deref_mut() {
            None => "JSON-UI carrier not loaded".to_owned(),
            Some(renderer) => {
                let translate = |key: &str| runtime.translation(key);
                let state = runtime.server_forms().engine();
                let remote = renderer.textures.remote.clone();
                let images = |url: &str| remote.state(url);
                match model::engine_model(&entry.model, state, &translate, &images) {
                    None => "form kind has no engine template".to_owned(),
                    Some(form) => {
                        let rollback = (nodes.len(), *next);
                        let inputs = engine::EngineInputs {
                            layouts: &mut self.layouts,
                            font: &self.font,
                            metrics,
                            solid_page: self.solid_texture_page,
                            safe_area: self.safe_area,
                            content: [width, height],
                            translate: &translate,
                            language: runtime.text_generation(),
                        };
                        let out = engine::EngineOutput {
                            nodes: &mut *nodes,
                            next: &mut *next,
                            overlay: &[],
                        };
                        let catalog = renderer.catalog_label();
                        let now = self.menu_seconds;
                        match renderer.render(
                            &form,
                            &state.view,
                            (entry.identity, now),
                            inputs,
                            out,
                        ) {
                            Ok(Some(frame)) => {
                                self.form_presentation.frame = Some(frame);
                                log_path(
                                    &mut self.form_presentation.logged,
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
            &mut self.form_presentation.logged,
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
        SIGN_SCREEN,
        EMOTE_SCREEN,
        EMOTE_EQUIP_POPUP,
        NPC_SCREEN,
        toast_screen::TOAST_SCREEN,
        crate::store::SDL_SCREEN,
        crate::ui_runtime::credits::CREDITS_SCREEN,
    ]
    .into_iter()
    .chain(loading_screen::LOADING_SCREENS)
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
        bevy::log::info!(?identity, path, reason, "server form render path");
    }
}

impl UiPresentationRuntime {
    /// Borrows the loaded UI carrier for the app panorama adapter.
    pub fn ui_assets(&self) -> Option<&assets::RuntimeUiAssets> {
        self.form_presentation
            .engine
            .as_deref()
            .map(|engine| engine.assets())
    }
}

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
