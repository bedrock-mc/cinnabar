//! The OreUI design system, drawn in our own code, and the screens 26.30 shows
//! with OreUI by default (`docs/oreui.md`). The dev-only local-originals mode
//! swaps in the install's icon and border sprites for side-by-side comparison.

mod bedtime;
mod death;
mod friends;
mod grid;
mod icons;
mod inbox;
mod loading;
mod modal;
mod paint;
mod play;
mod play_realms;
mod play_servers;
mod profile;
#[cfg(test)]
mod review_tests;
mod theme;
mod widgets;
mod world_settings;

use std::sync::Arc;

use render::{UiRenderTextureArray, UiTexturePage};
use ui::{UiNode, UiPoint, UiRect};

pub(crate) use bedtime::BedHit;
use paint::Canvas;
pub(crate) use paint::Originals;

use super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};
use crate::ui_runtime::oreui_assets::{OREUI_PAGE_SIDE, OreUiImages};

/// Which look OreUI screens draw with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(crate) enum Look {
    #[default]
    Drawn,
    /// The install's sprites where the drawn look would approximate them.
    Originals,
}

impl UiPresentationRuntime {
    /// Packs the dev-mode originals into a texture page; `CINNABAR_OREUI_LOOK=drawn`
    /// keeps the drawn look selected for comparison.
    pub(crate) fn enable_oreui_originals(&mut self, images: OreUiImages) -> Result<(), String> {
        let page = UiTexturePage::owned([OREUI_PAGE_SIDE, OREUI_PAGE_SIDE], images.rgba.into())
            .map_err(|error| format!("{error:?}"))?;
        let dynamic_start = self.textures.dynamic_start();
        let first = u16::try_from(dynamic_start).map_err(|_| "texture page overflow".to_owned())?;
        let mut pages = self.textures.pages()[..dynamic_start].to_vec();
        pages.push(page);
        pages.extend_from_slice(&self.textures.pages()[dynamic_start..]);
        let textures = UiRenderTextureArray::with_source_identity(
            pages,
            dynamic_start + 1,
            self.textures.static_identity(),
        )
        .map_err(|error| format!("{error:?}"))?;
        self.textures = Arc::new(textures);
        if let Some(engine) = self.form_presentation.engine.as_mut() {
            engine.textures.server_page = (self.textures.dynamic_start()
                + super::super::dynamic_textures::SERVER_UI_PAGE)
                as u16;
        }
        self.preview_dirty = true;
        self.menu_artwork_dirty = true;
        self.rebuild_dynamic_textures();
        let drawn = std::env::var("CINNABAR_OREUI_LOOK").is_ok_and(|look| look == "drawn");
        self.form_presentation.oreui_look = if drawn { Look::Drawn } else { Look::Originals };
        self.form_presentation.oreui_originals = Some(Arc::new(Originals {
            page: first,
            sprites: images.sprites,
            loading_frames: images.loading_frames,
        }));
        Ok(())
    }

    /// Draws `view` as an OreUI screen when 26.30 shows it with OreUI by
    /// default; `Ok(None)` leaves it to JSON-UI.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn append_oreui_screen(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        portrait: Option<super::super::IconRef>,
    ) -> Result<Option<Vec<(MenuAction, UiRect)>>, UiPresentationError> {
        // A launcher dialog draws over the OreUI screen instead.
        let covered = view.connecting
            || view.local.progress.is_some()
            || view.disconnect_message.is_some()
            || matches!(view.auth_state, AuthState::AwaitingCode { .. });
        let screen = view.screen;
        if covered
            || !matches!(
                screen,
                MenuScreen::Death
                    | MenuScreen::Profile
                    | MenuScreen::Inbox
                    | MenuScreen::Friends
                    | MenuScreen::Play
                    | MenuScreen::Social
                    | MenuScreen::Servers
            )
        {
            return Ok(None);
        }
        let originals = self
            .form_presentation
            .oreui_originals
            .clone()
            .filter(|_| self.form_presentation.oreui_look == Look::Originals);
        let offsets = self.menu_scrolls.offsets().clone();
        let mut canvas = Canvas::new(
            nodes,
            next,
            &mut self.layouts,
            &self.font,
            metrics,
            self.solid_texture_page,
            originals.as_deref(),
        );
        canvas.offsets = offsets;
        canvas.seconds = self.menu_seconds;
        match screen {
            MenuScreen::Death => death::draw(&mut canvas, view, size)?,
            MenuScreen::Profile => {
                profile::draw(&mut canvas, view, size, portrait, &self.menu_artwork.refs)?
            }
            MenuScreen::Inbox => inbox::draw(&mut canvas, view, size)?,
            MenuScreen::Play | MenuScreen::Social | MenuScreen::Servers => {
                match world_settings::route(view.local.screen, &view.local) {
                    Some(route) => world_settings::draw(&mut canvas, view, size, route)?,
                    None => play::draw(&mut canvas, view, size, &self.menu_artwork.refs)?,
                }
                if let Some(dialog) = modal::local_world_modal(&view.local) {
                    modal::draw(&mut canvas, view, size, &dialog)?;
                }
            }
            _ => friends::draw(&mut canvas, view, size)?,
        }
        let (hits, scrolls, spots) = (canvas.hits, canvas.scrolls, canvas.spots);
        self.menu_scrolls.set_areas(scrolls);
        self.add_menu_text_spots(spots);
        Ok(Some(hits))
    }
}

/// The bed screen's last hit rects (window-logical) and the tracked pointer.
#[derive(Default)]
pub(super) struct BedScreen {
    hits: Vec<(BedHit, UiRect)>,
    pointer: Option<UiPoint>,
}

impl UiPresentationRuntime {
    /// Draws the OreUI bed screen while the player lies in bed.
    pub(in super::super) fn append_bed_screen(
        &mut self,
        runtime: &crate::ui_runtime::UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
        now_millis: u64,
    ) -> Result<(), UiPresentationError> {
        let bed = &mut self.form_presentation.bed;
        let Some(elapsed) = self.hud_frame.sleep.asleep_for(now_millis) else {
            bed.hits.clear();
            return Ok(());
        };
        let hovered = bed.pointer.and_then(|point| {
            bed.hits
                .iter()
                .find_map(|(hit, bounds)| bounds.contains(point).then_some(*hit))
        });
        let state = bedtime::Bedtime {
            elapsed,
            // The local player is on the list too.
            remote_players: runtime.known_player_names().len() > 1,
            thunderstorm: self.hud_frame.thunderstorm,
            status: runtime.sleep_status(),
            hovered,
            pressed: None,
        };
        let mut canvas = Canvas::new(
            nodes,
            next,
            &mut self.layouts,
            &self.font,
            metrics,
            self.solid_texture_page,
            None,
        );
        let hits = bedtime::draw(&mut canvas, &state, size)?;
        let [left, top] = [self.safe_area.left(), self.safe_area.top()];
        self.form_presentation.bed.hits = hits
            .into_iter()
            .filter_map(|(hit, bounds)| {
                let min = bounds.min();
                let max = bounds.max();
                super::super::rect(min.x() + left, min.y() + top, max.x() + left, max.y() + top)
                    .ok()
                    .map(|bounds| (hit, bounds))
            })
            .collect();
        Ok(())
    }

    /// What a press at the window-logical `position` hits on the bed screen.
    pub(crate) fn hit_test_bed(&self, position: UiPoint) -> Option<BedHit> {
        self.form_presentation
            .bed
            .hits
            .iter()
            .find_map(|(hit, bounds)| bounds.contains(position).then_some(*hit))
    }

    /// Track the pointer for next frame's hover state.
    pub(crate) fn set_bed_pointer(&mut self, position: Option<UiPoint>) {
        self.form_presentation.bed.pointer = position;
    }
}

#[cfg(test)]
impl UiPresentationRuntime {
    /// The bed screen's hit rects from the last frame.
    pub(crate) fn bed_hits(&self) -> &[(BedHit, UiRect)] {
        &self.form_presentation.bed.hits
    }
}
