//! A client part's modal screen: its signed JSON-UI templates drawn through the engine over
//! gameplay, under the package screen rules of [`crate::template_screen`].

use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

use json_ui::{CollectionItem, HitKind};
use server_experience::screen::{self, GuiSize};
use ui::IconRef;
use ui_engine::engine::{
    FormEngine, ScreenArt,
    item_renderer::ItemRenderer,
    screen::{CustomRenderer, PaintFrame},
};

use crate::{
    ModUi,
    template_screen::{ModalEdits, TemplateArt, TemplateScreen},
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
    /// Every `#item_id_aux` key the bound data names.
    icon_keys: BTreeSet<i64>,
}

impl ModUi {
    /// Follows the client part's modal, closed or open; `None` (no running client part) drops
    /// its catalog and textures. `page` is the first dynamic page reserved to its atlas.
    pub fn set_experience_modal(&mut self, modal: Option<ExperienceModal<'_>>, page: u16) {
        let slot = &mut self.experience_modal;
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
                    ui_engine::pages::MODAL_UI_PAGES,
                ),
                screen: TemplateScreen::default(),
                scrolled: BTreeMap::new(),
                rows: Default::default(),
                icons: Vec::new(),
                icon_keys: BTreeSet::new(),
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
        screen.icon_keys = crate::mod_screens::icon_keys(modal.modal);
    }

    /// Every `#item_id_aux` key the client part's bound data names; `None` without a modal.
    pub fn experience_modal_icon_keys(&self) -> Option<&BTreeSet<i64>> {
        Some(&self.experience_modal.as_ref()?.icon_keys)
    }

    /// The drawn modal's root size in GUI units and the GUI scale; none while it is not drawn.
    pub fn experience_modal_size(&self) -> Option<GuiSize> {
        let screen = &self.experience_modal.as_ref()?.screen;
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
        match self.experience_modal.as_mut() {
            Some(modal) => modal.screen.edit(point, pressed, typed, escape, now),
            None => ModalEdits::default(),
        }
    }

    /// Returns named scroll ranges whose last drawn values have not been accepted for delivery.
    /// Undelivered ranges are retried, with only the latest drawn range for each view.
    pub fn experience_modal_scrolls(&self) -> Vec<(String, screen::ScrollRange)> {
        let Some(screen) = self.experience_modal.as_ref() else {
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
        if let Some(screen) = self.experience_modal.as_mut() {
            screen.scrolled.insert(view.to_owned(), range);
        }
    }

    /// Why the modal's templates were refused, which ends the client part.
    pub fn experience_modal_failure(&self) -> Option<&str> {
        self.experience_modal.as_ref()?.art.failure()
    }

    /// Whether the client part has a screen open, drawn yet or not; Escape and
    /// `ui.close-screen` both close it.
    pub fn experience_modal_open(&self) -> bool {
        self.experience_modal
            .as_ref()
            .is_some_and(|modal| modal.screen.template.is_some())
    }

    /// Whether the last build drew the modal, which then owns pointer and keyboard.
    pub fn experience_modal_shown(&self) -> bool {
        self.experience_modal
            .as_ref()
            .is_some_and(|modal| modal.screen.frame.is_some())
    }

    /// Lights the control under the pointer and remembers it for scrolling.
    pub fn hover_experience_modal(&mut self, point: Option<[f32; 2]>) {
        if let Some(modal) = self.experience_modal.as_mut() {
            modal.screen.hover(point);
        }
    }

    /// Scrolls the scroll view under the pointer by wheel `notches` (positive scrolls down).
    pub fn scroll_experience_modal(&mut self, notches: f64) {
        if let Some(modal) = self.experience_modal.as_mut() {
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
        let modal = self.experience_modal.as_mut()?;
        modal.screen.press(point, pressed, released)
    }

    /// The collection and row of the drawn modal's cell under `point`, such as a slot of its
    /// read-only `inventory_items` or `hotbar_items` rows.
    pub fn experience_modal_cell_at(&self, point: [f32; 2]) -> Option<(&str, usize)> {
        self.experience_modal.as_ref()?.screen.cell_at(point)
    }

    /// Whether an edit box of the drawn modal is selected and takes typing.
    pub fn experience_modal_text_focused(&self) -> bool {
        self.experience_modal
            .as_ref()
            .is_some_and(|modal| modal.screen.text_focused())
    }

    /// Tracks a secondary (right) press; see [`TemplateScreen::secondary_press`].
    pub fn secondary_press_experience_modal(
        &mut self,
        point: Option<[f32; 2]>,
        pressed: bool,
        released: bool,
    ) -> Option<(String, Option<usize>)> {
        let modal = self.experience_modal.as_mut()?;
        modal.screen.secondary_press(point, pressed, released)
    }

    /// Draws the modal over the gameplay scenes when nothing else holds the screen; trusted
    /// chrome draws after it. `player_rows` fills the read-only inventory and hotbar rows and
    /// their icons, and runs only when the modal draws; `id_aux` holds the icons of
    /// [`Self::experience_modal_icon_keys`].
    pub fn paint_experience_modal(
        &mut self,
        engine: Option<&FormEngine>,
        paint_frame: &mut PaintFrame<'_>,
        over_gameplay: bool,
        id_aux: &[(i64, IconRef)],
        player_rows: impl FnOnce(&mut Vec<IconRef>) -> [Vec<CollectionItem>; 2],
    ) {
        let Some(modal) = self.experience_modal.as_mut() else {
            return;
        };
        let Some(renderer) = engine else {
            modal.screen.frame = None;
            return;
        };
        if !over_gameplay {
            modal.screen.frame = None;
            return;
        }
        let mut icons = Vec::new();
        let rows = player_rows(&mut icons);
        if modal.rows != rows || modal.icons != icons {
            modal.screen.set_player_rows(rows.clone());
            modal.rows = rows;
            modal.icons = icons;
        }
        let mut items = ItemRenderer {
            icons: &modal.icons,
            id_aux,
        };
        let mut renderers: [&mut dyn CustomRenderer; 1] = [&mut items];
        if modal.screen.draw(
            &mut modal.art,
            renderer,
            &mut paint_frame.with_renderers(&mut renderers),
            ScreenArt::default(),
        ) {
            let hits = Arc::clone(&modal.screen.frame.as_ref().expect("drawn modal").hits);
            modal.screen.finish_input(&hits);
        }
    }

    /// Copies the modal atlas's page images when they changed; `true` asks for a page rebuild.
    pub fn refresh_experience_modal_pages(&mut self) -> bool {
        self.experience_modal
            .as_mut()
            .is_some_and(|modal| modal.art.refresh_pages())
    }

    /// The modal atlas's pages, for the dynamic pages reserved to it.
    pub fn experience_modal_pages(&self) -> &[render_model::UiTexturePage] {
        self.experience_modal
            .as_ref()
            .map_or(&[], |modal| modal.art.pages())
    }
}
