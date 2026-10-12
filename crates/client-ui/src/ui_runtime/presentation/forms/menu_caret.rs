//! The launcher text boxes' caret, after vanilla's text edit component as the
//! engine models it (`json_ui` `component/edit.rs` and `emit.rs`): the
//! `CARET_GLYPH` at the caret, toggled every `CARET_BLINK_SECONDS` and shown
//! again after each edit. A press inside a box places the caret at the
//! character nearest the pointer, measured as the box drew its text.

use ui::UiPoint;

use super::super::UiPresentationRuntime;
use launcher::menu::{MenuField, MenuView};
use ui_engine::caret::{TextSpot, nearest_boundary};

/// The caret's blink clock and where the last menu frame drew each box's text.
#[derive(Default)]
pub(super) struct MenuCaretState {
    /// The caret revision last seen, and the menu clock when it changed.
    blink: Option<(u64, f64)>,
    spots: Vec<TextSpot<MenuField>>,
}

impl UiPresentationRuntime {
    /// Starts a menu frame: the view's blink phase from this frame's clock, and no
    /// text drawn yet.
    pub(super) fn begin_menu_caret(&mut self, view: &mut MenuView) {
        let now = self.menu_seconds;
        let state = &mut self.screens.menu_caret;
        state.spots.clear();
        let since = match state.blink {
            Some((revision, since)) if revision == view.caret.revision => since,
            _ => {
                state.blink = Some((view.caret.revision, now));
                now
            }
        };
        let phase = ((now - since).max(0.0) / json_ui::CARET_BLINK_SECONDS) as u64;
        if view.field.is_some() {
            view.caret.shown = phase.is_multiple_of(2);
        }
    }

    pub(super) fn add_menu_text_spots(
        &mut self,
        spots: impl IntoIterator<Item = TextSpot<MenuField>>,
    ) {
        self.screens.menu_caret.spots.extend(spots);
    }

    /// The byte of `text` a press at `point` inside `field` puts its caret at,
    /// measured as the last menu frame drew the box's text.
    pub fn menu_caret_at(&mut self, point: UiPoint, field: MenuField, text: &str) -> Option<usize> {
        let spot = self
            .screens
            .menu_caret
            .spots
            .iter()
            .find(|spot| spot.field == field && spot.bounds.contains(point))?;
        let (layouts, font) = (&mut self.layouts, &self.font);
        Some(nearest_boundary(text, point.x() - spot.left, |prefix| {
            spot.width(layouts, font, prefix)
        }))
    }
}
