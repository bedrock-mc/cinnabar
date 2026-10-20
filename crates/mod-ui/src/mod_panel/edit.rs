use super::widgets::*;
use super::*;
use serde_json::Value;

pub(super) const FIELD_TEXT: &str = "#native_editor_field_text";

pub(super) enum Editor {
    Choice {
        control: usize,
        rect: [f64; 4],
        selected: usize,
    },
    Field {
        control: usize,
        rect: [f64; 4],
        text: String,
        selected: bool,
        cursor: usize,
        invalid: bool,
    },
}

impl ModUi {
    pub fn mod_panel_editing(&self) -> bool {
        if self.mod_hud_editor_open() {
            return true;
        }
        self.mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open && panel.edit.is_some())
    }

    /// Returns selected field text, an empty slice for a caret, or None without native field focus.
    pub fn mod_panel_field_selection(&self) -> Option<&str> {
        if self.mod_hud_editor_open() {
            return None;
        }
        let panel = self.mod_panel.as_ref().filter(|panel| panel.open)?;
        match panel.edit.as_ref()? {
            Editor::Field { text, selected, .. } => Some(if *selected { text } else { "" }),
            Editor::Choice { .. } => None,
        }
    }

    /// Routes keyboard input exclusively to an open personal-panel editor.
    pub fn mod_panel_key(&mut self, key: &str, text: Option<&str>) -> Vec<Event> {
        if let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) {
            let close_autosave =
                editor.autosave && matches!(key, "Escape" | "Enter" | "NumpadEnter");
            if close_autosave {
                editor.close_requested = key != "Escape";
                self.set_mod_panel_open(false);
            } else {
                editor.key(key);
            }
            return Vec::new();
        }
        self.mod_panel
            .as_mut()
            .map_or_else(Vec::new, |panel| panel.edit_key(key, text))
    }

    pub fn cancel_mod_panel_edit(&mut self) {
        self.cancel_mod_hud_editor();
        if let Some(panel) = self.mod_panel.as_mut() {
            panel.cancel_edit();
        }
    }
}

impl ModPanel {
    /// Native text fields retain keyboard ownership while a guest's capture flag is unchanged.
    pub(super) fn editing_text(&self) -> bool {
        matches!(self.edit.as_ref(),Some(Editor::Field {control,..})
            if matches!(self.panel.controls.get(*control),Some(Control::Text {..})))
    }
    pub(super) fn cancel_edit(&mut self) {
        if self.edit.take().is_some() {
            self.refresh_editor();
        }
    }

    /// Publishes literal draft data without putting user text through template resolution.
    fn refresh_editor(&mut self) {
        self.catalog = None;
        self.frame = None;
        self.drag = None;
        self.pointer = None;
        let text = self.editor_draft();
        Arc::make_mut(&mut self.data).set_global(FIELD_TEXT, json_ui::Scalar::Text(text));
    }

    /// Restores the native draft after guest values or viewport data have been refreshed.
    pub(super) fn write_editor_data(&self, data: &mut json_ui::DataSource) {
        data.set_global(FIELD_TEXT, json_ui::Scalar::Text(self.editor_draft()));
    }

    /// Builds one bounded display value using the same width as the native field overlay.
    fn editor_draft(&self) -> String {
        match self.edit.as_ref() {
            Some(Editor::Field {
                control,
                rect,
                text,
                selected,
                cursor,
                ..
            }) => visible_draft(
                text,
                *selected,
                *cursor,
                field_width(&self.panel, *control, rect, self.viewport),
            ),
            _ => String::new(),
        }
    }

    /// Starts the native editor appropriate to the selected admitted control kind.
    pub(super) fn open_editor(&mut self, index: usize, rect: [f64; 4]) {
        self.edit = match self.panel.controls.get(index) {
            Some(Control::Choice {
                index: selected, ..
            }) => Some(Editor::Choice {
                control: index,
                rect,
                selected: *selected as usize,
            }),
            Some(Control::Slider { value, .. }) => {
                let text = format_number(*value);
                let cursor = text.len();
                Some(Editor::Field {
                    control: index,
                    rect,
                    text,
                    selected: true,
                    cursor,
                    invalid: false,
                })
            }
            Some(Control::Text { value, .. }) => Some(Editor::Field {
                control: index,
                rect,
                text: value.clone(),
                selected: true,
                cursor: value.len(),
                invalid: false,
            }),
            _ => None,
        };
        self.refresh_editor();
    }

    pub(super) fn choose(&mut self, selected: usize) -> Vec<Event> {
        let Some(Editor::Choice { control, .. }) = self.edit.as_ref() else {
            return Vec::new();
        };
        let control = *control;
        let Some(Control::Choice {
            id, index, options, ..
        }) = self.panel.controls.get_mut(control)
        else {
            self.cancel_edit();
            return Vec::new();
        };
        if selected >= options.len() {
            return Vec::new();
        }
        *index = selected as u32;
        let event = Event {
            id: id.clone(),
            value: selected as f32,
        };
        self.refresh_data();
        self.cancel_edit();
        vec![event]
    }

    /// Commits valid numeric and text fields through their separate event channels.
    fn commit_field(&mut self) -> Vec<Event> {
        let Some(Editor::Field {
            control,
            text,
            invalid,
            ..
        }) = self.edit.as_mut()
        else {
            return Vec::new();
        };
        if let Some(Control::Text {
            id,
            value,
            min_chars,
            max_chars,
            ..
        }) = self.panel.controls.get_mut(*control)
        {
            if value == text {
                self.cancel_edit();
                return Vec::new();
            }
            if ui::mod_panel::validate_text_value(text, *min_chars, *max_chars).is_err()
                || self.text_events.len() >= ui::mod_panel::MAX_PANEL_EVENTS
            {
                *invalid = true;
                self.refresh_editor();
                return Vec::new();
            }
            *value = text.clone();
            self.text_events.push(ui::mod_panel::TextEvent {
                id: id.clone(),
                value: text.clone(),
            });
            self.refresh_data();
            self.cancel_edit();
            return Vec::new();
        }
        let Some(Control::Slider {
            id,
            value,
            min,
            max,
            step,
            ..
        }) = self.panel.controls.get_mut(*control)
        else {
            self.cancel_edit();
            return Vec::new();
        };
        let parsed = text
            .parse::<f32>()
            .ok()
            .filter(|n| n.is_finite() && *n >= *min && *n <= *max);
        let Some(parsed) = parsed else {
            *invalid = true;
            self.refresh_editor();
            return Vec::new();
        };
        let next = (*min + ((parsed - *min) / *step).round() * *step).clamp(*min, *max);
        let event = Event {
            id: id.clone(),
            value: next,
        };
        *value = next;
        self.refresh_data();
        self.cancel_edit();
        vec![event]
    }

    /// Edits one bounded single-line draft, keeping its caret on Unicode character boundaries.
    fn edit_key(&mut self, key: &str, input: Option<&str>) -> Vec<Event> {
        if !self.open || self.edit.is_none() {
            return Vec::new();
        }
        match key {
            "Escape" | "F10" | "ShiftRight" => {
                self.cancel_edit();
                return Vec::new();
            }
            "Enter" | "NumpadEnter" => {
                return match self.edit.as_ref() {
                    Some(Editor::Choice { selected, .. }) => self.choose(*selected),
                    _ => self.commit_field(),
                };
            }
            _ => {}
        }
        let text_limit = match self.edit.as_ref() {
            Some(Editor::Field { control, .. }) => match self.panel.controls.get(*control) {
                Some(Control::Text { max_chars, .. }) => Some(*max_chars),
                _ => None,
            },
            _ => None,
        };
        match self.edit.as_mut() {
            Some(Editor::Choice {
                control, selected, ..
            }) => {
                let Some(Control::Choice { options, .. }) = self.panel.controls.get(*control)
                else {
                    return Vec::new();
                };
                match key {
                    "ArrowDown" => *selected = (*selected + 1) % options.len(),
                    "ArrowUp" => *selected = (*selected + options.len() - 1) % options.len(),
                    "Home" => *selected = 0,
                    "End" => *selected = options.len() - 1,
                    _ => return Vec::new(),
                }
            }
            Some(Editor::Field {
                text,
                selected,
                cursor,
                invalid,
                ..
            }) => match key {
                "SelectAll" => {
                    *selected = true;
                    *cursor = text.len();
                }
                "Home" => {
                    *cursor = 0;
                    *selected = false;
                }
                "End" => {
                    *cursor = text.len();
                    *selected = false;
                }
                "ArrowLeft" => {
                    *cursor = if *selected {
                        0
                    } else {
                        previous_character(text, *cursor)
                    };
                    *selected = false;
                }
                "ArrowRight" => {
                    *cursor = if *selected {
                        text.len()
                    } else {
                        next_character(text, *cursor)
                    };
                    *selected = false;
                }
                "Backspace" | "Delete" => {
                    if *selected {
                        text.clear();
                        *cursor = 0;
                    } else if key == "Backspace" && *cursor > 0 {
                        let previous = previous_character(text, *cursor);
                        text.drain(previous..*cursor);
                        *cursor = previous;
                    } else if key == "Delete" && *cursor < text.len() {
                        text.remove(*cursor);
                    }
                    *selected = false;
                    *invalid = false;
                }
                _ => {
                    let fallback = key
                        .strip_prefix("Digit")
                        .or_else(|| key.strip_prefix("Numpad"))
                        .filter(|s| s.len() == 1 && s.as_bytes()[0].is_ascii_digit())
                        .or(match key {
                            "Period" | "NumpadDecimal" => Some("."),
                            "Minus" | "NumpadSubtract" => Some("-"),
                            _ => None,
                        });
                    let Some(input) = input.or(fallback) else {
                        return Vec::new();
                    };
                    if input.is_empty()
                        || input.chars().any(char::is_control)
                        || text_limit.is_none()
                            && !input
                                .chars()
                                .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
                    {
                        return Vec::new();
                    }
                    let current = if *selected { 0 } else { text.chars().count() };
                    if current + input.chars().count() > text_limit.unwrap_or(24) {
                        return Vec::new();
                    }
                    if *selected {
                        text.clear();
                        *cursor = 0;
                    }
                    text.insert_str(*cursor, input);
                    *cursor += input.len();
                    *selected = false;
                    *invalid = false;
                }
            },
            None => return Vec::new(),
        }
        self.refresh_editor();
        Vec::new()
    }
}

/// Uses the finite native slider value's shortest ordinary representation.
fn format_number(value: f32) -> String {
    value.to_string()
}

/// Keeps a byte-indexed caret visible without splitting UTF-8 characters.
pub(super) fn visible_draft(text: &str, selected: bool, cursor: usize, width: f64) -> String {
    let capacity = ((width - 6.) / 5.).floor().max(3.) as usize;
    if selected {
        return text.chars().take(capacity).collect();
    }
    let characters: Vec<char> = text.chars().collect();
    let cursor = text
        .char_indices()
        .take_while(|(at, _)| *at < cursor)
        .count();
    let start = cursor.saturating_sub(capacity - 1);
    let end = (start + capacity - 1).min(characters.len());
    characters[start..cursor]
        .iter()
        .copied()
        .chain(std::iter::once('|'))
        .chain(characters[cursor..end].iter().copied())
        .collect()
}

/// Returns the previous complete character boundary for native field navigation and deletion.
fn previous_character(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .take_while(|(at, _)| *at < cursor)
        .last()
        .map_or(0, |(at, _)| at)
}

/// Returns the next complete character boundary, including the end of the field.
fn next_character(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .find(|(at, _)| *at > cursor)
        .map_or(text.len(), |(at, _)| at)
}

/// Keeps editing width identical for clipping and overlay geometry across control kinds.
fn field_width(spec: &Panel, control: usize, rect: &[f64; 4], viewport: [f64; 2]) -> f64 {
    let minimum = if matches!(spec.controls.get(control), Some(Control::Text { .. })) {
        180.
    } else {
        42.
    };
    rect[2].max(minimum).min(viewport[0] - 8.)
}
pub(super) fn append_overlay(
    document: &mut Value,
    spec: &Panel,
    viewport: [f64; 2],
    editor: Option<&Editor>,
) {
    let Some(editor) = editor else {
        return;
    };
    let palette = Palette::for_panel(spec);
    let mut controls = vec![named(
        "dismiss",
        button("mod.dismiss", viewport, [0.; 2], Vec::new()),
    )];
    match editor {
        Editor::Choice {
            control,
            rect,
            selected,
        } => {
            let Some(Control::Choice { options, .. }) = spec.controls.get(*control) else {
                return;
            };
            let width = rect[2].max(88.).min(viewport[0] - 8.);
            let row_height = ((viewport[1] - 12.) / options.len() as f64).min(21.);
            let height = options.len() as f64 * row_height + 4.;
            let x = rect[0].clamp(4., viewport[0] - width - 4.);
            let below = rect[1] + rect[3] + 2.;
            let y = if below + height <= viewport[1] - 4. {
                below
            } else {
                (rect[1] - height - 2.).max(4.)
            };
            let mut rows = chrome([width, height], palette, 5.);
            for (index, option) in options.iter().enumerate() {
                let mut items = Vec::new();
                if index == *selected {
                    items.push(named(
                        "selected",
                        rounded([width - 4., row_height - 1.], [0.; 2], 3., palette.raised),
                    ));
                }
                items.push(named(
                    "label",
                    label(
                        option,
                        [width - 12., row_height - 4.],
                        [4., 5.],
                        if index == *selected {
                            palette.accent
                        } else {
                            palette.text
                        },
                        false,
                    ),
                ));
                rows.push(named(
                    &format!("option_{index}"),
                    button(
                        &format!("mod.option:{index}"),
                        [width - 4., row_height],
                        [2., 2. + index as f64 * row_height],
                        items,
                    ),
                ));
            }
            controls.push(named("choices", panel([width, height], [x, y], rows)));
        }
        Editor::Field {
            control,
            rect,
            selected,
            invalid,
            ..
        } => {
            let is_text = matches!(spec.controls.get(*control), Some(Control::Text { .. }));
            let width = field_width(spec, *control, rect, viewport);
            let x = if is_text {
                rect[0]
            } else {
                rect[0] + rect[2] - width
            }
            .clamp(4., viewport[0] - width - 4.);
            let height = 17.;
            let y = (rect[1] + (rect[3] - height) * 0.5).clamp(4., viewport[1] - height - 4.);
            let mut raised = palette;
            raised.card = palette.raised;
            raised.border = if *invalid {
                palette.accent
            } else {
                palette.text
            };
            let mut field = chrome([width, height], raised, 3.);
            field.push(named(
                "value",
                label(
                    FIELD_TEXT,
                    [width - 6., 14.],
                    [3., 4.],
                    if *selected {
                        palette.accent
                    } else {
                        palette.text
                    },
                    true,
                ),
            ));
            controls.push(named(
                "number",
                button("mod.editor", [width, height], [x, y], field),
            ));
            let message = if *invalid {
                match spec.controls.get(*control) {
                    Some(Control::Slider { min, max, .. }) => Some(format!(
                        "Use {} to {}",
                        format_number(*min),
                        format_number(*max)
                    )),
                    Some(Control::Text {
                        min_chars,
                        max_chars,
                        ..
                    }) => Some(format!("Use {min_chars} to {max_chars} characters")),
                    _ => None,
                }
            } else {
                None
            };
            if let Some(message) = message {
                let w = 105_f64.min(viewport[0] - 8.);
                let x = x.min(viewport[0] - w - 4.);
                let y = (y + height + 2.).min(viewport[1] - 22.);
                let mut hint = chrome([w, 18.], raised, 3.);
                hint.push(named(
                    "label",
                    label(&message, [w - 6., 14.], [3., 4.], palette.accent, false),
                ));
                controls.push(named("hint", panel([w, 18.], [x, y], hint)));
            }
        }
    }
    document["panel"]["controls"]
        .as_array_mut()
        .expect("screen controls are an array")
        .extend(controls);
}
