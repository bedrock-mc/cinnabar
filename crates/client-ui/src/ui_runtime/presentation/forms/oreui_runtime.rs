//! Adapters that project runtime authority into OreUI's typed inputs and apply its outputs.

use std::sync::Arc;

use launcher::menu::{MenuAction, MenuScreen, MenuView};
use render_model::{UiRenderTextureArray, UiTexturePage};
use ui::{IconRef, UiNode, UiPoint, UiRect};

use super::super::menu_artwork::TITLE_KEY;
use super::super::player_preview::MenuPreviewConfig;
use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime, dynamic_textures};
use super::engine::screen::{PaintFrame, Screen};
use super::loading_screen::LoadingStage;
use super::menu_screens::Translate;
use super::oreui::{
    BedHit, Look, Originals,
    bed_screen::BedInput,
    loading_screen::LoadingInput,
    menu_screen::{self, MenuGeometry, MenuInput},
};
use crate::ui_runtime::{UiRuntime, oreui_assets::OreUiImages};

impl UiPresentationRuntime {
    /// Draws an owned OreUI route, including the owner's menu design extensions.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_oreui_screen(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        portrait: Option<IconRef>,
        translate: Translate<'_>,
    ) -> Result<Option<Vec<(MenuAction, UiRect)>>, UiPresentationError> {
        #[cfg(feature = "tracy")]
        let _screen_span = tracing::info_span!("ui.oreui_paint").entered();
        if !menu_screen::draws_menu(view) {
            return Ok(None);
        }
        #[cfg(test)]
        {
            self.oreui_paints += 1;
        }
        let progress = view.connecting || view.local.progress.is_some();
        let screen = view.screen;
        self.menu_scrolls.configure_motion(
            view.settings_options.value("screen_animations") != 0,
            self.menu_seconds,
        );
        let title_artwork = if screen == MenuScreen::Home && !progress {
            self.menu_artwork.refs.get(TITLE_KEY).copied()
        } else if progress || screen == MenuScreen::Pause {
            if self.engine.is_some() {
                self.title_icon(&self.menu_artwork.refs)
            } else {
                self.menu_artwork.refs.get(TITLE_KEY).copied()
            }
        } else {
            None
        };
        let input = MenuInput {
            view,
            size,
            portrait,
            translate,
            artwork: &self.menu_artwork.refs,
            title_artwork,
            scrolls: &mut self.menu_scrolls,
            geometry: MenuGeometry {
                slider_tracks: std::mem::take(&mut self.projections.oreui_slider_tracks),
                focus_targets: std::mem::take(&mut self.projections.menu_focus_geometry),
                focus_landmarks: std::mem::take(&mut self.projections.menu_focus_landmarks),
            },
        };
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: size,
            translate,
            language: [0; 3],
            now: self.menu_seconds,
            nodes: &mut *nodes,
            next: &mut *next,
            renderers: &mut [],
        };
        let output = self.screens.oreui.paint(input, &mut frame)?;
        let mut paint = self
            .screens
            .oreui
            .take_paint()
            .expect("a menu paint leaves its layout");
        if let Some(focus) = output.focus {
            self.projections.menu_focus = focus;
        }
        if paint.settings_input {
            self.settings_slider_drag_targets.clear();
            self.projections.oreui_settings_input = true;
        }
        let mut hits = output.hits;
        let geometry = std::mem::take(&mut paint.geometry);
        self.projections.oreui_slider_tracks = geometry.slider_tracks;
        self.projections.menu_focus_geometry = geometry.focus_targets;
        self.projections.menu_focus_landmarks = geometry.focus_landmarks;
        self.menu_scrolls
            .set_areas(std::mem::take(&mut paint.scrolls));
        self.add_menu_text_spots(std::mem::take(&mut paint.spots));
        if let Some(preview) = paint.character_preview.take() {
            self.append_menu_player_preview(
                nodes,
                next,
                metrics,
                preview.control,
                preview.clip,
                MenuPreviewConfig::DRESSING_ROOM,
            )?;
        }
        if let Some(dressing) = paint.dressing_preview.take() {
            self.menu_skin_thumbnail_indices = dressing.visible_skins;
            self.menu_cape_thumbnail_indices = dressing.visible_capes;
            self.append_menu_player_preview(
                nodes,
                next,
                metrics,
                dressing.preview.control,
                dressing.preview.clip,
                MenuPreviewConfig {
                    starting_rotation: if view.dressing_room.section
                        == launcher::dressing_room::DressingRoomSection::Capes
                    {
                        210.0
                    } else {
                        30.0
                    },
                    ..MenuPreviewConfig::DRESSING_ROOM
                },
            )?;
            paint.apply_entrance(nodes, size)?;
            if view.dressing_room.editor.is_some() {
                self.menu_preview.control = None;
                self.cancel_menu_player_preview_input();
                let mut frame = PaintFrame {
                    layouts: &mut self.layouts,
                    font: &self.font,
                    metrics,
                    solid_page: self.solid_texture_page,
                    safe_area: self.safe_area,
                    content: size,
                    translate,
                    language: [0; 3],
                    now: self.menu_seconds,
                    nodes: &mut *nodes,
                    next: &mut *next,
                    renderers: &mut [],
                };
                let editor = self.screens.oreui.paint_editor(
                    &paint,
                    view,
                    size,
                    &self.menu_artwork.refs,
                    &mut frame,
                )?;
                self.projections.menu_focus = editor.focus;
                self.projections.menu_focus_geometry = editor.focus_targets;
                self.projections.menu_focus_landmarks = editor.focus_landmarks;
                hits = editor.hits;
                self.add_menu_text_spots(editor.spots);
                self.menu_scrolls.set_areas(Vec::new());
            }
        } else {
            paint.apply_entrance(nodes, size)?;
        }
        self.screens.menu_sounds.clear();
        Ok(Some(hits))
    }

    /// Draws the quit confirmation; its targets alone then count.
    pub(super) fn append_oreui_exit(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        translate: Translate<'_>,
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: size,
            translate,
            language: [0; 3],
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        let (hits, scrolls) = self
            .screens
            .oreui
            .paint_exit(view, size, translate, &mut frame)?;
        self.menu_scrolls.set_areas(scrolls);
        Ok(hits)
    }

    /// Draws the saved-accounts picker over the current screen; its targets alone then count.
    pub(super) fn append_oreui_accounts(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: size,
            translate: &|_| None,
            language: [0; 3],
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        Ok(self.screens.oreui.paint_accounts(
            view,
            size,
            &self.menu_artwork.refs,
            &mut self.menu_scrolls,
            &mut frame,
        )?)
    }

    /// Plays out OreUI's exit and capture effects over the frame.
    pub(in super::super) fn append_oreui_motion(
        &mut self,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        Ok(self
            .screens
            .oreui
            .finish_motion(nodes, next, self.menu_seconds, size)?)
    }

    pub(in super::super) fn append_oreui_loading(
        &mut self,
        stage: LoadingStage,
        words: [&str; 2],
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
    ) -> Result<(), UiPresentationError> {
        let title_artwork = self.title_icon(&self.menu_artwork.refs);
        let destination_icon = (stage == LoadingStage::ChangingDimension)
            .then(|| {
                crate::ui_runtime::oreui_assets::dimensions::destination(self.hud_frame.dimension)
                    .and_then(|art| self.item_icon(art.block, 0))
            })
            .flatten();
        let input = LoadingInput {
            stage,
            words,
            size,
            title_artwork,
            destination_icon,
            artwork: &self.menu_artwork.refs,
            dimension: self.hud_frame.dimension,
        };
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: size,
            translate: &|_| None,
            language: [0; 3],
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        Ok(self.screens.oreui.paint_loading(input, &mut frame)?)
    }

    /// Draws the OreUI bed screen while the player lies in bed.
    pub(in super::super) fn append_bed_screen(
        &mut self,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let input = BedInput {
            asleep_for: self.hud_frame.sleep.asleep_for(now_millis),
            // The local player is on the list too.
            remote_players: runtime.known_player_names().len() > 1,
            thunderstorm: self.hud_frame.thunderstorm,
            status: runtime.sleep_status(),
        };
        let mut frame = PaintFrame {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content: size,
            translate: &|_| None,
            language: [0; 3],
            now: self.menu_seconds,
            nodes,
            next,
            renderers: &mut [],
        };
        Ok(self.screens.bed.paint(input, &mut frame)?)
    }

    /// What a press at the window-logical `position` hits on the bed screen.
    pub fn hit_test_bed(&self, position: UiPoint) -> Option<BedHit> {
        self.screens.bed.hit(position)
    }

    /// Track the pointer for next frame's hover state.
    pub fn set_bed_pointer(&mut self, position: Option<UiPoint>) {
        self.screens.bed.set_pointer(position);
    }

    /// The bed screen's hit rects from the last frame.
    #[cfg(test)]
    pub fn bed_hits(&self) -> &[(BedHit, UiRect)] {
        self.screens.bed.hits()
    }

    pub fn cancel_menu_server_list_input(&mut self) {
        self.screens.oreui.cancel_server_list_input();
    }

    /// Section capture suppresses click activation until release, including outside the list.
    pub fn menu_server_list_pointer(
        &mut self,
        screen: Option<MenuScreen>,
        pointer: Option<UiPoint>,
        held: bool,
        pressed: bool,
    ) -> (bool, Option<MenuAction>) {
        let (captured, action, scroll) = self.screens.oreui.server_list_pointer(
            screen,
            pointer,
            (held, pressed),
            self.menu_seconds,
        );
        if scroll != 0.0 {
            self.menu_scrolls.scroll_by("servers.side_menu", scroll);
        }
        (captured, action)
    }

    /// Registers the installed artwork as immutable static texture pages.
    pub fn enable_oreui_originals(&mut self, images: OreUiImages) -> Result<(), String> {
        if self
            .screens
            .oreui
            .originals
            .as_ref()
            .is_some_and(|old| old.same_images(&images))
        {
            return Ok(());
        }
        let originals = images
            .pages
            .iter()
            .map(|page| {
                UiTexturePage::owned(page.dimensions, page.pixels.clone())
                    .map_err(|error| format!("{error:?}"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let dynamic_start = self.textures.dynamic_start();
        let start = self
            .screens
            .oreui
            .originals
            .as_ref()
            .map_or(dynamic_start, |old| usize::from(old.page()));
        let first = u16::try_from(start).map_err(|_| "texture page overflow".to_owned())?;
        let count = originals.len();
        let mut pages = self.textures.pages()[..start].to_vec();
        pages.extend(originals);
        pages.extend_from_slice(&self.textures.pages()[dynamic_start..]);
        let textures = UiRenderTextureArray::with_source_identity(
            pages,
            start + count,
            self.textures.static_identity(),
        )
        .map_err(|error| format!("{error:?}"))?;
        self.textures = Arc::new(textures);
        if let Some(engine) = self.engine.as_mut() {
            engine.textures.server_page =
                (self.textures.dynamic_start() + dynamic_textures::SERVER_UI_PAGE) as u16;
        }
        self.preview_dirty = true;
        self.menu_artwork_dirty = true;
        self.rebuild_dynamic_textures();
        self.screens.oreui.look = Look::Originals;
        self.screens.oreui.originals = Some(Arc::new(Originals::new(first, images)));
        Ok(())
    }

    /// Prepares additional native art before drawing; unchanged requests retain texture pages.
    pub fn prepare_oreui_artwork(&mut self, keys: &[&str]) -> Result<(), String> {
        if self
            .screens
            .oreui
            .originals
            .as_ref()
            .is_some_and(|old| old.has_sprites(keys))
        {
            return Ok(());
        }
        let images = crate::ui_runtime::oreui_assets::load_optional_oreui_images()
            .ok_or("OreUI installed artwork is unavailable")?;
        let old_start = self
            .screens
            .oreui
            .originals
            .as_ref()
            .map_or(self.textures.dynamic_start(), |old| usize::from(old.page()));
        let previous_artwork = self.textures.pages()[old_start..self.textures.dynamic_start()]
            .iter()
            .map(|page| page.pixels().len())
            .sum::<usize>();
        let resident = self.textures.fixed_budget_bytes() - previous_artwork;
        let available = render_model::MAX_UI_FIXED_TEXTURE_BYTES
            .checked_sub(resident)
            .ok_or("Resident UI textures exceed the texture budget")?;
        self.enable_oreui_originals(images.with_artwork_budget(keys, available)?)
    }
}
