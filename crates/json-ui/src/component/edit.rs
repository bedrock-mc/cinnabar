//! The text edit component of an `edit_box`: what the factory reads, the
//! retained text/caret state, and vanilla's character-entry and fit rules.

use serde_json::Value;

use super::{descendant, text_of};
use crate::layout::{LaidOut, TextMeasure};
use crate::widgets::bound_bool;

/// Seconds between caret blinks.
pub const CARET_BLINK_SECONDS: f64 = 0.3;
/// The glyph a selected box's text target draws at its caret.
pub const CARET_GLYPH: char = '_';

/// `text_type`: the platform keyboard mode the box asks for; physical-keyboard
/// characters are not filtered by it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextType {
    #[default]
    ExtendedAscii,
    IdentifierChars,
    NumberChars,
}

/// What an edit box's component reads at creation, with its targets' keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditMeta {
    pub name: Option<String>,
    pub grid_collection: Option<String>,
    /// `max_length`: an integer, else zero; no text longer than it fits.
    pub max_length: i64,
    pub enabled_newline: bool,
    pub constrain_to_rect: bool,
    pub text_type: TextType,
    pub keyboard_buffer: Option<String>,
    /// Key and `[w, h]` of the `text_control` label.
    pub text_target: Option<(String, [f64; 2])>,
    pub placeholder: Option<String>,
    pub can_be_deselected: bool,
    pub always_listening: bool,
    pub placeholder_hover_color: Option<Value>,
    /// The text the box was laid out with.
    pub text: String,
}

impl EditMeta {
    pub(crate) fn read(node: &LaidOut) -> Self {
        let control = node.control;
        let target = |key: &str| text_of(control, key).and_then(|name| descendant(node, &name));
        let text_node = target("text_control");
        EditMeta {
            name: text_of(control, "text_box_name"),
            grid_collection: text_of(control, "text_edit_box_grid_collection_name"),
            // Any integral number is admitted, else it reads zero.
            max_length: crate::widgets::bound_number(control, "max_length")
                .filter(|length| length.fract() == 0.0)
                .map_or(0, |length| length as i64),
            enabled_newline: bound_bool(control, "enabled_newline").unwrap_or(false),
            constrain_to_rect: bound_bool(control, "constrain_to_rect").unwrap_or(false),
            text_type: match text_of(control, "text_type").as_deref() {
                Some("IdentifierChars") => TextType::IdentifierChars,
                Some("NumberChars") => TextType::NumberChars,
                _ => TextType::ExtendedAscii,
            },
            keyboard_buffer: target("virtual_keyboard_buffer_control").map(|n| n.key.clone()),
            text: text_node
                .and_then(|n| n.control.properties.get("text"))
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_owned(),
            text_target: text_node.map(|n| (n.key.clone(), [n.rect.w, n.rect.h])),
            placeholder: target("place_holder_control").map(|n| n.key.clone()),
            can_be_deselected: bound_bool(control, "#can_be_deselected")
                .or_else(|| bound_bool(control, "can_be_deselected"))
                .unwrap_or(true),
            always_listening: bound_bool(control, "always_listening").unwrap_or(false),
            placeholder_hover_color: control
                .properties
                .get("place_holder_text_hover_color")
                .cloned(),
        }
    }

    /// Whether `text` fits: no more characters than `max_length`, and with
    /// `constrain_to_rect` no taller, wrapped, than the text target.
    pub fn fits(&self, text: &str, measure: Option<&dyn TextMeasure>) -> bool {
        if text.chars().count() as i64 > self.max_length {
            return false;
        }
        if !self.constrain_to_rect {
            return true;
        }
        match (&self.text_target, measure) {
            (Some((_, [width, height])), Some(measure)) => {
                measure.wrapped(text, *width)[1] <= *height
            }
            (None, _) => false,
            (Some(_), None) => true,
        }
    }
}

/// An edit box's retained editing state.
#[derive(Clone, Debug, PartialEq)]
pub struct TextEdit {
    pub text: String,
    /// Caret position in characters; typing appends, so it follows the end.
    pub caret: usize,
    pub caret_shown: bool,
    blink: f64,
}

impl TextEdit {
    pub(crate) fn new(text: &str) -> Self {
        Self {
            text: text.to_owned(),
            caret: text.chars().count(),
            caret_shown: true,
            blink: 0.0,
        }
    }

    /// Advance the caret blink by `delta` seconds; `true` when it flipped.
    pub fn tick(&mut self, delta: f64) -> bool {
        self.blink += delta;
        if self.blink > CARET_BLINK_SECONDS {
            self.blink = 0.0;
            self.caret_shown = !self.caret_shown;
            return true;
        }
        false
    }
}

/// What typed text did to a box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CharOutcome {
    /// Enter on a box without `enabled_newline`: the caller ends editing.
    Enter,
    Changed,
    Rejected,
    Unchanged,
}

/// Apply typed `input`: Enter without newlines ends
/// editing, backspace drops the last character, anything else appends whole
/// when the result fits.
pub(crate) fn type_text(
    meta: &EditMeta,
    edit: &mut TextEdit,
    input: &str,
    measure: Option<&dyn TextMeasure>,
) -> CharOutcome {
    let Some(first) = input.chars().next() else {
        return CharOutcome::Unchanged;
    };
    if matches!(first, '\n' | '\r') && !meta.enabled_newline {
        return CharOutcome::Enter;
    }
    if !meta.enabled_newline && input.contains(['\n', '\r']) {
        return CharOutcome::Rejected;
    }
    if first == '\u{8}' {
        if edit.text.pop().is_none() {
            return CharOutcome::Unchanged;
        }
        edit.caret = edit.text.chars().count();
        return CharOutcome::Changed;
    }
    let mut next = edit.text.clone();
    next.push_str(input);
    if !meta.fits(&next, measure) {
        return CharOutcome::Rejected;
    }
    edit.text = next;
    edit.caret = edit.text.chars().count();
    edit.caret_shown = true;
    edit.blink = 0.0;
    CharOutcome::Changed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(max_length: i64) -> EditMeta {
        EditMeta {
            max_length,
            can_be_deselected: true,
            ..EditMeta::default()
        }
    }

    // A box without max_length accepts nothing, as the factory reads zero.
    #[test]
    fn missing_max_length_admits_no_text() {
        let mut edit = TextEdit::new("");
        assert_eq!(
            type_text(&meta(0), &mut edit, "a", None),
            CharOutcome::Rejected
        );
        assert_eq!(
            type_text(&meta(-1), &mut edit, "a", None),
            CharOutcome::Rejected
        );
        assert_eq!(
            type_text(&meta(2), &mut edit, "ab", None),
            CharOutcome::Changed
        );
        assert_eq!(
            type_text(&meta(2), &mut edit, "c", None),
            CharOutcome::Rejected
        );
    }

    // Enter ends editing unless newlines are enabled; backspace removes one.
    #[test]
    fn enter_and_backspace_follow_the_component() {
        let mut edit = TextEdit::new("ab");
        assert_eq!(
            type_text(&meta(10), &mut edit, "\r", None),
            CharOutcome::Enter
        );
        let newline = EditMeta {
            enabled_newline: true,
            ..meta(10)
        };
        assert_eq!(
            type_text(&newline, &mut edit, "\n", None),
            CharOutcome::Changed
        );
        assert_eq!(edit.text, "ab\n");
        assert_eq!(
            type_text(&meta(10), &mut edit, "\u{8}", None),
            CharOutcome::Changed
        );
        assert_eq!(edit.text, "ab");
    }

    struct Wide;
    impl TextMeasure for Wide {
        fn extent(&self, text: &str) -> [f64; 2] {
            [text.chars().count() as f64 * 6.0, 10.0]
        }
        fn wrapped(&self, text: &str, max_width: f64) -> [f64; 2] {
            let width = text.chars().count() as f64 * 6.0;
            [
                width.min(max_width),
                10.0 * (width / max_width).ceil().max(1.0),
            ]
        }
    }

    // constrain_to_rect rejects text that would wrap past the target's height.
    #[test]
    fn constrained_boxes_reject_overflowing_text() {
        let boxed = EditMeta {
            constrain_to_rect: true,
            text_target: Some(("/t".into(), [30.0, 10.0])),
            ..meta(100)
        };
        assert!(boxed.fits("abcde", Some(&Wide)));
        assert!(!boxed.fits("abcdef", Some(&Wide)));
    }

    // The caret blinks every 0.3 seconds.
    #[test]
    fn caret_blinks_on_the_component_interval() {
        let mut edit = TextEdit::new("");
        assert!(!edit.tick(0.2));
        assert!(edit.tick(0.2));
        assert!(!edit.caret_shown);
    }
    #[test]
    fn review_pasted_newlines_obey_the_single_line_rule() {
        let mut edit = TextEdit::new("old");
        for paste in ["a\nb", "a\rb"] {
            assert_eq!(
                type_text(&meta(40), &mut edit, paste, None),
                CharOutcome::Rejected
            );
            assert_eq!(edit.text, "old");
        }
    }
}
