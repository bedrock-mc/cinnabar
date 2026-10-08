use super::widgets::*;
use super::*;
use serde_json::Value;

pub(super) enum Editor {
    Choice {
        control: usize,
        rect: [f64; 4],
        selected: usize,
    },
    Number {
        control: usize,
        rect: [f64; 4],
        text: String,
        selected: bool,
        cursor: usize,
        invalid: bool,
    },
}

impl UiPresentationRuntime {
    pub fn mod_panel_editing(&self) -> bool {
        if self.mod_hud_editor_open() {
            return true;
        }
        self.form_presentation
            .mod_panel
            .as_ref()
            .is_some_and(|panel| panel.open && panel.edit.is_some())
    }

    /// Routes keyboard input exclusively to an open personal-panel editor.
    pub fn mod_panel_key(&mut self, key: &str, text: Option<&str>) -> Vec<Event> {
        if let Some(editor) = self
            .form_presentation
            .mod_hud_editor
            .as_mut()
            .filter(|e| e.open)
        {
            editor.key(key);
            return Vec::new();
        }
        self.form_presentation
            .mod_panel
            .as_mut()
            .map_or_else(Vec::new, |panel| panel.edit_key(key, text))
    }

    pub fn cancel_mod_panel_edit(&mut self) {
        self.cancel_mod_hud_editor();
        if let Some(panel) = self.form_presentation.mod_panel.as_mut() {
            panel.cancel_edit();
        }
    }
}

impl ModPanel {
    pub(super) fn cancel_edit(&mut self) {
        if self.edit.take().is_some() {
            self.refresh_editor();
        }
    }

    fn refresh_editor(&mut self) {
        self.catalog = None;
        self.frame = None;
        self.drag = None;
        self.pointer = None;
    }

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
                Some(Editor::Number {
                    control: index,
                    rect,
                    text,
                    selected: true,
                    cursor,
                    invalid: false,
                })
            }
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
        self.data = Arc::new(control_data(&self.panel));
        self.cancel_edit();
        vec![event]
    }

    fn commit_number(&mut self) -> Vec<Event> {
        let Some(Editor::Number {
            control,
            text,
            invalid,
            ..
        }) = self.edit.as_mut()
        else {
            return Vec::new();
        };
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
        self.data = Arc::new(control_data(&self.panel));
        self.cancel_edit();
        vec![event]
    }

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
                    _ => self.commit_number(),
                };
            }
            _ => {}
        }
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
            Some(Editor::Number {
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
                        cursor.saturating_sub(1)
                    };
                    *selected = false;
                }
                "ArrowRight" => {
                    *cursor = if *selected {
                        text.len()
                    } else {
                        (*cursor + 1).min(text.len())
                    };
                    *selected = false;
                }
                "Backspace" | "Delete" => {
                    if *selected {
                        text.clear();
                        *cursor = 0;
                    } else if key == "Backspace" && *cursor > 0 {
                        *cursor -= 1;
                        text.remove(*cursor);
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
                        || !input
                            .chars()
                            .all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '+'))
                    {
                        return Vec::new();
                    }
                    if !*selected && text.len() + input.len() > 24 || input.len() > 24 {
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

fn format_number(value: f32) -> String {
    value.to_string()
}

pub(super) fn visible_draft(text: &str, selected: bool, cursor: usize, width: f64) -> String {
    let capacity = ((width - 6.) / 5.).floor().max(3.) as usize;
    if selected {
        return text.chars().take(capacity).collect();
    }
    let before = cursor.min(capacity - 1);
    let start = cursor - before;
    let end = (start + capacity - 1).min(text.len());
    let mut visible = text[start..end].to_owned();
    visible.insert(before, '|');
    visible
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
        Editor::Number {
            control,
            rect,
            text,
            selected,
            cursor,
            invalid,
        } => {
            let width = rect[2].max(42.);
            let x = (rect[0] + rect[2] - width).clamp(4., viewport[0] - width - 4.);
            let height = 17.;
            let y = (rect[1] - 2.).clamp(4., viewport[1] - height - 4.);
            let mut raised = palette;
            raised.card = palette.raised;
            raised.border = if *invalid {
                palette.accent
            } else {
                palette.text
            };
            let mut field = chrome([width, height], raised, 3.);
            let display = visible_draft(text, *selected, *cursor, width);
            field.push(named(
                "value",
                label(
                    &display,
                    [width - 6., 14.],
                    [3., 4.],
                    if *selected {
                        palette.accent
                    } else {
                        palette.text
                    },
                    false,
                ),
            ));
            controls.push(named(
                "number",
                button("mod.editor", [width, height], [x, y], field),
            ));
            if *invalid && let Some(Control::Slider { min, max, .. }) = spec.controls.get(*control)
            {
                let message = format!("Use {} to {}", format_number(*min), format_number(*max));
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
