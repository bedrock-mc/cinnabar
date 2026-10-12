//! The launcher text boxes' caret glyph, after vanilla's text edit component as the engine
//! models it: the `CARET_GLYPH` at the caret while the blink shows it.

use std::borrow::Cow;

use launcher::menu::{MenuField, MenuView};
use ui_engine::caret::caret_byte;

/// `text` with the caret glyph at the caret while `field` is focused and the blink shows it.
pub fn with_caret<'a>(view: &MenuView, field: MenuField, text: &'a str) -> Cow<'a, str> {
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
