use super::*;

impl UiPresentationRuntime {
    /// Releases pointer capture while preserving the panel and its editor draft.
    pub fn cancel_mod_panel_pointer_input(&mut self) {
        if let Some(editor) = self.form_presentation.mod_hud_editor.as_mut() {
            editor.cancel_pointer_input();
        }
        if let Some(panel) = self.form_presentation.mod_panel.as_mut() {
            panel.cancel_pointer_input();
        }
    }
}

impl ModPanel {
    /// Releases captured movement and visual pointer states without closing the panel.
    fn cancel_pointer_input(&mut self) {
        self.drag = None;
        self.scroll_drag = None;
        self.pointer = None;
        self.held = false;
        self.view.hovered = None;
        self.view.pressed = None;
        self.view.pointer = None;
    }

    pub(super) fn pointer_events(
        &mut self,
        position: [f32; 2],
        pressed: bool,
        held: bool,
    ) -> Vec<Event> {
        if !self.open || !position.iter().all(|value| value.is_finite()) {
            self.cancel_pointer_input();
            return Vec::new();
        }
        let Some(frame) = self.frame.as_ref() else {
            return Vec::new();
        };
        if !pressed
            && self.pointer == Some(position)
            && self.held == held
            && (held || self.view.pressed.is_none())
        {
            return Vec::new();
        }
        self.pointer = Some(position);
        self.held = held;
        let point = [
            f64::from((position[0] - frame.origin[0]) / frame.scale),
            f64::from((position[1] - frame.origin[1]) / frame.scale),
        ];
        if self.scroll_pointer(point, pressed, held) {
            self.drag = None;
            self.view.pressed = None;
            return Vec::new();
        }
        let Some(frame) = self.frame.as_ref() else {
            return Vec::new();
        };
        let hit =
            frame.hits.iter().rev().find(|region| {
                region.enabled && region.pressed.is_some() && region.contains(point)
            });
        let hovered = hit.map(|hit| hit.key.as_str());
        if self.view.hovered.as_deref() != hovered {
            self.view.hovered = hovered.map(str::to_owned);
        }
        if pressed {
            self.view.pressed = hovered.map(str::to_owned);
        } else if !held {
            self.view.pressed = None;
        }
        if !held && !pressed {
            self.drag = None;
        }
        let action = pressed
            .then(|| hit.and_then(|hit| hit.pressed.as_deref()))
            .flatten();
        if pressed {
            if action == Some("mod.dismiss") {
                self.cancel_edit();
                return Vec::new();
            }
            if action == Some("mod.editor") {
                return Vec::new();
            }
            if let Some(index) = action
                .and_then(|action| action.strip_prefix("mod.option:"))
                .and_then(|index| index.parse().ok())
            {
                return self.choose(index);
            }
            if let Some(index) = action
                .and_then(|action| action.strip_prefix("mod.edit:"))
                .and_then(|index| index.parse::<usize>().ok())
            {
                if !self.panel.capture_key {
                    let rect = hit.map(|hit| [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h]);
                    if let Some(rect) = rect {
                        self.open_editor(index, rect);
                    }
                }
                return Vec::new();
            }
            if let Some(index) = action
                .and_then(|action| action.strip_prefix("mod.control:"))
                .and_then(|index| index.parse::<usize>().ok())
                && matches!(self.panel.controls.get(index), Some(Control::Choice { .. }))
            {
                if !self.panel.capture_key {
                    let rect = hit.map(|hit| [hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h]);
                    if let Some(rect) = rect {
                        self.open_editor(index, rect);
                    }
                }
                return Vec::new();
            }
            if self.edit.is_some() {
                self.cancel_edit();
                return Vec::new();
            }
        }
        match action {
            Some("mod.close") => {
                self.open = false;
                self.frame = None;
                self.cancel_pointer_input();
                return Vec::new();
            }
            Some("mod.prev") | Some("mod.next") => {
                self.page = if action == Some("mod.prev") {
                    self.page.saturating_sub(1)
                } else {
                    (self.page + 1).min(self.last_page())
                };
                self.catalog = None;
                self.frame = None;
                self.drag = None;
                self.pointer = None;
                return Vec::new();
            }
            _ => {}
        }
        if let Some(category) = action
            .and_then(|action| action.strip_prefix("mod.category:"))
            .and_then(|value| value.parse::<usize>().ok())
        {
            if category < ui::mod_panel::MAX_PANEL_CATEGORIES && category != self.category {
                self.category = category;
                self.page = 0;
                self.catalog = None;
                self.frame = None;
                self.drag = None;
                self.pointer = None;
                self.view = ViewState::default();
            }
            return Vec::new();
        }
        let selected = action
            .and_then(|action| action.strip_prefix("mod.control:"))
            .and_then(|index| index.parse::<usize>().ok());
        if let Some(index) = selected {
            self.drag = matches!(self.panel.controls.get(index), Some(Control::Slider { .. }))
                .then_some(index);
        }
        let index = selected.or(self.drag.filter(|_| held));
        let Some(index) = index else {
            return Vec::new();
        };
        let Some(control) = self.panel.controls.get_mut(index) else {
            return Vec::new();
        };
        let value = match control {
            Control::Toggle { value, .. } if pressed => {
                *value = !*value;
                f32::from(u8::from(*value))
            }

            Control::Button { .. } | Control::Keybind { .. } if pressed => 1.0,
            Control::Slider {
                value,
                min,
                max,
                step,
                ..
            } => {
                let action = format!("mod.control:{index}");
                let Some(region) = frame
                    .hits
                    .iter()
                    .find(|hit| hit.pressed.as_deref() == Some(&action))
                else {
                    self.drag = None;
                    return Vec::new();
                };
                let raw = *min + region.fraction_at(point[0]) as f32 * (*max - *min);
                let next = (*min + ((raw - *min) / *step).round() * *step).clamp(*min, *max);
                if *value == next {
                    return Vec::new();
                }
                *value = next;
                next
            }
            _ => return Vec::new(),
        };
        let event = Event {
            id: control.id().to_owned(),
            value,
        };
        self.refresh_data();
        vec![event]
    }
}
