//! The launcher text boxes' caret, after vanilla's text edit component as the
//! engine models it (`json_ui` `component/edit.rs` and `emit.rs`): the
//! `CARET_GLYPH` at the caret, toggled every `CARET_BLINK_SECONDS` and shown
//! again after each edit. A press inside a box places the caret at the
//! character nearest the pointer, measured as the box drew its text.

use std::borrow::Cow;

use ui::UiPoint;

use super::super::UiPresentationRuntime;
use launcher::menu::{MenuField, MenuView};
use ui_engine::caret::{TextSpot, caret_byte, nearest_boundary};

/// The caret's blink clock and where the last menu frame drew each box's text.
#[derive(Default)]
pub(super) struct MenuCaretState {
    /// The caret revision last seen, and the menu clock when it changed.
    blink: Option<(u64, f64)>,
    spots: Vec<TextSpot<MenuField>>,
}

/// `text` with the caret glyph at the caret while `field` is focused and the blink shows it.
pub(in crate::ui_runtime::presentation) fn with_caret<'a>(
    view: &MenuView,
    field: MenuField,
    text: &'a str,
) -> Cow<'a, str> {
    if view.field != Some(field) || !view.caret.shown || view.caret.selection.is_some() {
        return Cow::Borrowed(text);
    }
    let at = caret_byte(text, view.caret.byte);
    let mut shown = String::with_capacity(text.len() + json_ui::CARET_GLYPH.len_utf8());
    shown.push_str(&text[..at]);
    shown.push(json_ui::CARET_GLYPH);
    shown.push_str(&text[at..]);
    Cow::Owned(shown)
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
