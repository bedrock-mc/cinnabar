//! A client part's modal screen: its signed JSON-UI templates drawn through the engine over
//! gameplay, under the package screen rules of [`super::template_screen`].

use std::{collections::BTreeMap, sync::Arc};

use json_ui::{CollectionItem, HitKind};

use server_experience::screen::{self, GuiSize};
use ui::UiNode;

use super::{
    engine::{EngineInputs, ScreenArt},
    template_screen::{TemplateArt, TemplateScreen},
};
use crate::ui_runtime::UiRuntime;
use {
    super::super::{TextMetrics, UiPresentationRuntime},
    ui::IconRef,
};

pub use super::template_screen::ModalEdits;
#[cfg(test)]
use {
    super::template_screen::{data_source, package_catalog as modal_catalog},
    json_ui::{Catalog, DataSource, Scalar},
};

/// The modal a client part asks for: its bundle, verified screen files and bound data.
pub struct ExperienceModal<'a> {
    pub bundle: &'a str,
    pub files: &'a Arc<screen::Files>,
    pub modal: &'a screen::Modal,
}

pub(super) struct ModalScreen {
    art: TemplateArt,
    screen: TemplateScreen,
    scrolled: BTreeMap<String, screen::ScrollRange>,
    rows: [Vec<CollectionItem>; 2],
    icons: Vec<IconRef>,
}

impl UiPresentationRuntime {
    /// Follows the client part's modal, closed or open; `None` (no running client part) drops
    /// its catalog and textures.
    pub fn set_experience_modal(&mut self, modal: Option<ExperienceModal<'_>>) {
        let page =
            (self.textures.dynamic_start() + super::super::dynamic_textures::MODAL_UI_PAGE) as u16;
        let slot = &mut self.form_presentation.experience_modal;
        let Some(modal) = modal else {
            *slot = None;
            return;
        };
        if slot
            .as_ref()
            .is_none_or(|current| !current.art.is(modal.bundle, modal.files))
        {
            *slot = Some(ModalScreen {
                art: TemplateArt::new(
                    modal.bundle,
                    modal.files,
                    page,
                    super::super::dynamic_textures::MODAL_UI_PAGES,
                ),
                screen: TemplateScreen::default(),
                scrolled: BTreeMap::new(),
                rows: Default::default(),
                icons: Vec::new(),
            });
        }
        let screen = slot.as_mut().expect("modal installed");
        if screen.screen.template != modal.modal.template {
            screen.scrolled.clear();
        }
        if screen
            .screen
            .follow(modal.modal.template.as_ref(), modal.modal)
        {
            screen.rows = Default::default();
        }
    }

    /// The drawn modal's root size in GUI units and the GUI scale; none while it is not drawn.
    pub fn experience_modal_size(&self) -> Option<GuiSize> {
        let screen = &self.form_presentation.experience_modal.as_ref()?.screen;
        screen.frame.as_ref().and(screen.size)
    }

    /// Drives the drawn modal's edit boxes as vanilla's `text_edit_box`; see
    /// [`TemplateScreen::edit`].
    pub fn edit_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        typed: &[String],
        escape: bool,
        now: f64,
    ) -> ModalEdits {
        match self.form_presentation.experience_modal.as_mut() {
            Some(modal) => modal.screen.edit(point, pressed, typed, escape, now),
            None => ModalEdits::default(),
        }
    }

    /// Returns named scroll ranges whose last drawn values have not been accepted for delivery.
    /// Undelivered ranges are retried, with only the latest drawn range for each view.
    pub fn experience_modal_scrolls(&self) -> Vec<(String, screen::ScrollRange)> {
        let Some(screen) = self.form_presentation.experience_modal.as_ref() else {
            return Vec::new();
        };
        let Some(frame) = &screen.screen.frame else {
            return Vec::new();
        };
        let mut out: Vec<(String, screen::ScrollRange)> = Vec::new();
        for region in frame.hits.iter() {
            let (HitKind::ScrollView, Some(name)) = (region.kind, &region.control_name) else {
                continue;
            };
            let Some(metrics) = frame.report.scrolls.get(&region.key) else {
                continue;
            };
            let range = screen::ScrollRange {
                offset: metrics.offset,
                viewport: metrics.viewport,
                content: metrics.content,
            };
            if !range.valid() {
                continue;
            }
            out.retain(|(view, _)| view != name);
            out.push((name.clone(), range));
        }
        out.retain(|(view, range)| screen.scrolled.get(view) != Some(range));
        out
    }

    /// Records a range accepted by the modal event queue; later changes remain pending.
    pub fn accept_experience_modal_scroll(&mut self, view: &str, range: screen::ScrollRange) {
        if let Some(screen) = self.form_presentation.experience_modal.as_mut() {
            screen.scrolled.insert(view.to_owned(), range);
        }
    }

    /// Why the modal's templates were refused, which ends the client part.
    pub fn experience_modal_failure(&self) -> Option<&str> {
        self.form_presentation
            .experience_modal
            .as_ref()?
            .art
            .failure()
    }

    /// Whether the client part has a screen open, drawn yet or not; Escape and
    /// `ui.close-screen` both close it.
    pub(super) fn experience_modal_open(&self) -> bool {
        self.form_presentation
            .experience_modal
            .as_ref()
            .is_some_and(|modal| modal.screen.template.is_some())
    }

    /// Whether the last build drew the modal, which then owns pointer and keyboard.
    pub fn experience_modal_shown(&self) -> bool {
        self.form_presentation
            .experience_modal
            .as_ref()
            .is_some_and(|modal| modal.screen.frame.is_some())
    }

    /// Lights the control under the pointer and remembers it for scrolling.
    pub fn hover_experience_modal(&mut self, point: Option<[f32; 2]>) {
        if let Some(modal) = self.form_presentation.experience_modal.as_mut() {
            modal.screen.hover(point);
        }
    }

    /// Scrolls the scroll view under the pointer by wheel `notches` (positive scrolls down).
    pub fn scroll_experience_modal(&mut self, notches: f64) {
        if let Some(modal) = self.form_presentation.experience_modal.as_mut() {
            modal.screen.scroll(notches);
        }
    }

    /// Tracks a left press and returns the control id and collection row of a press released
    /// over the control it began on, as vanilla buttons fire.
    pub fn press_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let modal = self.form_presentation.experience_modal.as_mut()?;
        modal.screen.press(point, pressed, released)
    }

    /// Tracks a secondary (right) press; see [`TemplateScreen::secondary_press`].
    pub fn secondary_press_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let modal = self.form_presentation.experience_modal.as_mut()?;
        modal.screen.secondary_press(point, pressed, released)
    }

    /// Draws the modal over the gameplay scenes when nothing else holds the screen; trusted
    /// chrome draws after it.
    #[allow(clippy::too_many_arguments)]
    pub(in super::super) fn append_experience_modal(
        &mut self,
        player_runtime: &player_state::PlayerState,
        runtime: &UiRuntime,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        content: [f32; 2],
        over_gameplay: bool,
    ) {
        let Some(modal) = self.form_presentation.experience_modal.as_mut() else {
            return;
        };
        let Some(renderer) = self.form_presentation.engine.as_deref() else {
            modal.screen.frame = None;
            return;
        };
        if !over_gameplay {
            modal.screen.frame = None;
            return;
        }
        let mut icons = Vec::new();
        let rows =
            super::containers::player_rows(player_runtime, runtime, &self.hud_frame, &mut icons);
        if modal.rows != rows || modal.icons != icons {
            modal.screen.set_player_rows(rows.clone());
            modal.rows = rows;
            modal.icons = icons;
        }
        let translate = |key: &str| runtime.translation(key);
        let inputs = EngineInputs {
            layouts: &mut self.layouts,
            font: &self.font,
            metrics,
            solid_page: self.solid_texture_page,
            safe_area: self.safe_area,
            content,
            translate: &translate,
            language: runtime.text_generation(),
        };
        if modal.screen.draw(
            &mut modal.art,
            renderer,
            inputs,
            (nodes, next),
            ScreenArt {
                icons: &modal.icons,
                ..ScreenArt::default()
            },
        ) {
            let hits = Arc::clone(&modal.screen.frame.as_ref().expect("drawn modal").hits);
            modal.screen.finish_input(&hits);
        }
    }

    /// Copies the modal atlas's page images when they changed; `true` asks for a page rebuild.
    pub(super) fn refresh_experience_modal_pages(&mut self) -> bool {
        self.form_presentation
            .experience_modal
            .as_mut()
            .is_some_and(|modal| modal.art.refresh_pages())
    }

    /// The modal atlas's pages, for the dynamic pages reserved to it.
    pub(in super::super) fn experience_modal_pages(&self) -> &[render_model::UiTexturePage] {
        self.form_presentation
            .experience_modal
            .as_ref()
            .map_or(&[], |modal| modal.art.pages())
    }
}

#[cfg(test)]
mod tests;
