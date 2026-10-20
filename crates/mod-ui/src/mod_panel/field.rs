//! Bounded single-line editing and transient IME state for native panel fields.

use std::{ops::Range, sync::Arc};
use ui::mod_panel::FieldModifiers;

pub(super) struct Field {
    pub text: String,
    pub cursor: usize,
    anchor: usize,
    pub composition: String,
    pub composition_cursor: Option<usize>,
    pub display: String,
    pub layout: Option<Arc<ui::TextLayout>>,
    pub blink_start: Option<f64>,
    pub scroll: f32,
    pub ime_position: [f32; 2],
    pub left: f32,
    pub dragging: bool,
}

impl Field {
    /// Opens a selected value, retaining a display buffer and one shaped layout.
    pub fn new(text: String) -> Self {
        let cursor = text.len();
        Self {
            display: text.clone(),
            text,
            cursor,
            anchor: 0,
            composition: String::new(),
            composition_cursor: None,
            layout: None,
            blink_start: None,
            scroll: 0.,
            ime_position: [0.; 2],
            left: 0.,
            dragging: false,
        }
    }

    /// Returns the selected byte range, or the empty range at the caret.
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.cursor)..self.anchor.max(self.cursor)
    }

    /// Replaces inline preedit without changing the committed value or selection.
    pub fn preedit(&mut self, text: &str, cursor: Option<usize>, limit: usize) {
        if text.chars().any(char::is_control) || text.chars().count() > limit {
            return;
        }
        self.composition.clear();
        self.composition.push_str(text);
        self.composition_cursor = cursor.map(|at| boundary(text, at));
        self.refresh_display();
    }

    /// Publishes a display only on text/preedit changes; caret motion keeps its shaped layout.
    fn refresh_display(&mut self) {
        self.blink_start = None;
        let selection = self.selection();
        let same = if self.composition.is_empty() {
            self.display == self.text
        } else {
            self.display
                .strip_prefix(&self.text[..selection.start])
                .and_then(|tail| tail.strip_prefix(&self.composition))
                == Some(&self.text[selection.end..])
        };
        if same {
            return;
        }
        self.display.clear();
        if self.composition.is_empty() {
            self.display.push_str(&self.text);
        } else {
            let selection = self.selection();
            self.display.push_str(&self.text[..selection.start]);
            self.display.push_str(&self.composition);
            self.display.push_str(&self.text[selection.end..]);
        }
        self.layout = None;
        self.blink_start = None;
    }

    /// Replaces selection after checking the complete resulting character count.
    pub fn insert(&mut self, input: &str, limit: usize, numeric: bool) -> bool {
        let range = self.selection();
        if input.is_empty()
            || input.chars().any(char::is_control)
            || numeric
                && !input
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
            || self.text[..range.start].chars().count()
                + input.chars().count()
                + self.text[range.end..].chars().count()
                > limit
        {
            return false;
        }
        let changed = self.text[range.clone()] != *input;
        self.text.replace_range(range.clone(), input);
        self.cursor = range.start + input.len();
        self.anchor = self.cursor;
        self.composition.clear();
        self.composition_cursor = None;
        self.refresh_display();
        changed
    }

    /// Clears transient preedit when the platform cancels or commits its composition.
    pub fn clear_composition(&mut self) {
        if !self.composition.is_empty() {
            self.composition.clear();
            self.composition_cursor = None;
            self.refresh_display();
        }
    }

    /// Applies selection, Unicode-safe motion and deletion, returning whether the value changed.
    pub fn key(&mut self, key: &str, modifiers: FieldModifiers) -> bool {
        let word = modifiers.alt || modifiers.control;
        let selecting = modifiers.shift;
        let range = self.selection();
        let target = match key {
            "SelectAll" => {
                self.anchor = 0;
                self.cursor = self.text.len();
                self.blink_start = None;
                return false;
            }
            "Home" => Some(0),
            "End" => Some(self.text.len()),
            "ArrowLeft" => Some(if !selecting && !range.is_empty() {
                range.start
            } else if word {
                previous_word(&self.text, self.cursor)
            } else {
                previous_character(&self.text, self.cursor)
            }),
            "ArrowRight" => Some(if !selecting && !range.is_empty() {
                range.end
            } else if word {
                next_word(&self.text, self.cursor)
            } else {
                next_character(&self.text, self.cursor)
            }),
            _ => None,
        };
        if let Some(target) = target {
            self.cursor = target;
            if !selecting {
                self.anchor = target;
            }
            self.blink_start = None;
            return false;
        }
        if !matches!(key, "Backspace" | "Delete") {
            return false;
        }
        let range = if !range.is_empty() {
            range
        } else if key == "Backspace" {
            (if word {
                previous_word(&self.text, self.cursor)
            } else {
                previous_character(&self.text, self.cursor)
            })..self.cursor
        } else {
            self.cursor..(if word {
                next_word(&self.text, self.cursor)
            } else {
                next_character(&self.text, self.cursor)
            })
        };
        if range.is_empty() {
            return false;
        }
        self.text.replace_range(range.clone(), "");
        self.cursor = range.start;
        self.anchor = self.cursor;
        self.refresh_display();
        true
    }

    /// Places or extends a mouse selection using the retained glyph boundaries.
    pub fn place_cursor(&mut self, window_x: f32, selecting: bool) {
        if !self.composition.is_empty() {
            return;
        }
        let Some(layout) = &self.layout else {
            return;
        };
        let x = window_x - self.left;
        self.cursor = self
            .text
            .char_indices()
            .map(|(at, _)| at)
            .chain([self.text.len()])
            .min_by(|left, right| {
                let distance = |at| (layout.first_line_caret_x_64(at) as f32 / 64. - x).abs();
                distance(*left).total_cmp(&distance(*right))
            })
            .unwrap_or(0);
        if !selecting {
            self.anchor = self.cursor;
        }
        self.blink_start = None;
    }

    /// Maps a displayed IME caret to the same byte stops used for the painted text.
    pub fn display_cursor(&self) -> usize {
        if self.composition.is_empty() {
            self.cursor
        } else {
            self.selection().start + self.composition_cursor.unwrap_or(self.composition.len())
        }
    }
}

/// Clamps a platform-provided byte cursor to a complete UTF-8 boundary.
fn boundary(text: &str, at: usize) -> usize {
    let mut at = at.min(text.len());
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

/// Finds the previous complete character for caret motion and deletion.
fn previous_character(text: &str, at: usize) -> usize {
    text[..at]
        .char_indices()
        .next_back()
        .map_or(0, |(at, _)| at)
}

/// Finds the next complete character, including the end boundary.
fn next_character(text: &str, at: usize) -> usize {
    text[at..]
        .char_indices()
        .nth(1)
        .map_or(text.len(), |(next, _)| at + next)
}

/// Skips whitespace and the preceding word for modified left motion.
fn previous_word(text: &str, at: usize) -> usize {
    text[..at]
        .trim_end_matches(char::is_whitespace)
        .char_indices()
        .rfind(|(_, c)| c.is_whitespace())
        .map_or(0, |(at, c)| at + c.len_utf8())
}

/// Skips the current word and following whitespace for modified right motion.
fn next_word(text: &str, at: usize) -> usize {
    let after = &text[at..];
    let end = after.find(char::is_whitespace).unwrap_or(after.len());
    at + end
        + after[end..]
            .find(|c: char| !c.is_whitespace())
            .unwrap_or(after.len() - end)
}
