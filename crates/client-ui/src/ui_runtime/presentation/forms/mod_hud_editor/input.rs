use super::*;
use ui::mod_hud::Placement;

pub(in super::super) struct Drag {
    index: usize,
    grab: [f64; 2],
    start: [f64; 2],
    moved: bool,
}

impl HudEditor {
    /// Ends capture and retains one bounded Save or Cancel result for the host.
    pub(super) fn finish(&mut self, saved: bool) {
        if self.autosave {
            self.cancel_drag();
        }
        // A completed gesture can precede dismissal in the same input batch.
        // Ownership-loss cancellation separately drains this retained result.
        if saved || !self.autosave || !self.result.as_ref().is_some_and(|result| result.saved) {
            self.result = Some(self.snapshot(saved));
        }
        self.open = false;
        self.frame = None;
        self.drag = None;
        self.view = ViewState::default();
    }
    /// Captures the complete bounded placement set for one persistence callback.
    fn snapshot(&self, saved: bool) -> EditorResult {
        EditorResult {
            saved,
            reset: saved && self.reset,
            placements: if saved {
                self.draft
                    .cards
                    .iter()
                    .map(|c| Placement {
                        id: c.id.clone(),
                        position: c.position,
                    })
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
    /// Persists completed opted-in gestures while retaining the live editor.
    fn checkpoint(&mut self) {
        if self.autosave && self.draft != self.committed {
            self.result = Some(self.snapshot(true));
            self.committed = self.draft.clone();
            self.reset = false;
        }
    }
    /// Releases an unfinished drag without promoting it to a saved placement.
    pub(in super::super) fn cancel_drag(&mut self) {
        if let Some(drag) = self.drag.take()
            && self.autosave
        {
            self.draft.cards[drag.index] = self.committed.cards[drag.index].clone();
        }
    }
    /// Releases pointer feedback and rolls back an unfinished captured gesture.
    pub(in super::super) fn cancel_pointer_input(&mut self) {
        self.cancel_drag();
        self.view.hovered = None;
        self.view.pressed = None;
        self.view.pointer = None;
    }
    /// Converts a clamped GUI-pixel top-left into normalized available travel.
    fn move_card(&mut self, index: usize, at: [f64; 2]) {
        let card = &mut self.draft.cards[index];
        let size = cards::dimensions(card);
        card.position = Some(std::array::from_fn(|axis| {
            let available = (self.viewport[axis] - size[axis]).max(0.);
            let at = if self.snap {
                (at[axis] / 8.).round() * 8.
            } else {
                at[axis]
            };
            if available > 0. {
                (at.clamp(0., available) / available) as f32
            } else {
                0.
            }
        }));
    }
    /// Uses rendered hit regions and retains a drag until its physical release.
    pub(in super::super) fn pointer(
        &mut self,
        position: [f32; 2],
        pressed: bool,
        held: bool,
        controls: &[ui::mod_panel::Control],
    ) -> Vec<ui::mod_panel::Event> {
        if !self.open || !position.into_iter().all(f32::is_finite) {
            self.cancel_pointer_input();
            return Vec::new();
        }
        let Some(frame) = self.frame.as_ref() else {
            return Vec::new();
        };
        let point = std::array::from_fn(|axis| {
            f64::from((position[axis] - frame.origin[axis]) / frame.scale)
        });
        let hit = frame
            .hits
            .iter()
            .rev()
            .find(|hit| hit.enabled && hit.pressed.is_some() && hit.contains(point));
        let hovered = hit.map(|hit| hit.key.as_str());
        if self.view.hovered.as_deref() != hovered {
            self.view.hovered = hovered.map(str::to_owned);
        }
        if pressed {
            self.view.pressed = hovered.map(str::to_owned);
        } else if !held {
            self.view.pressed = None;
        }
        let action = pressed
            .then(|| hit.and_then(|hit| hit.pressed.clone()))
            .flatten();
        if let Some(id) = action
            .as_deref()
            .and_then(|action| action.strip_prefix("hud.done:"))
            .and_then(|index| index.parse::<usize>().ok())
            .and_then(|index| controls.get(index))
            .and_then(|control| match control {
                ui::mod_panel::Control::Button { id, .. } => Some(id.clone()),
                _ => None,
            })
        {
            self.finish(true);
            return vec![ui::mod_panel::Event { id, value: 1. }];
        }
        match action.as_deref() {
            Some("hud.close") => {
                self.finish(self.autosave);
                self.close_requested = true;
                return Vec::new();
            }
            Some("hud.save") => {
                self.finish(true);
                return Vec::new();
            }
            Some("hud.cancel") => {
                self.finish(false);
                return Vec::new();
            }
            Some("hud.reset") => {
                for card in &mut self.draft.cards {
                    card.anchor = card.reset_anchor.unwrap_or(card.anchor);
                    card.offset = card.reset_offset.unwrap_or(card.offset);
                    card.position = None;
                }
                self.reset = true;
                self.drag = None;
                self.checkpoint();
                return Vec::new();
            }
            Some("hud.grid") => {
                self.snap = !self.snap;
                self.cancel_drag();
                return Vec::new();
            }
            _ => {}
        }
        if pressed {
            self.cancel_drag();
            self.drag = action
                .as_deref()
                .and_then(|a| a.strip_prefix("hud.card:"))
                .and_then(|n| n.parse::<usize>().ok())
                .filter(|&n| n < self.draft.cards.len())
                .map(|index| {
                    let at = cards::origin(&self.draft.cards[index], self.viewport);
                    Drag {
                        index,
                        grab: std::array::from_fn(|axis| point[axis] - at[axis]),
                        start: point,
                        moved: false,
                    }
                });
            self.selected = self.drag.as_ref().map(|drag| drag.index);
        }
        if let Some(drag) = &mut self.drag {
            drag.moved |= (0..2).any(|axis| (point[axis] - drag.start[axis]).abs() > 0.001);
            let index = drag.index;
            let at = std::array::from_fn(|axis| point[axis] - drag.grab[axis]);
            let moved = drag.moved;
            if !pressed && moved {
                self.move_card(index, at);
            }
        }
        if !held {
            let completed = self.drag.take().is_some_and(|drag| drag.moved);
            if completed {
                self.checkpoint();
            }
        }
        Vec::new()
    }
    /// Handles editor keys while keeping them away from gameplay input.
    pub(in super::super) fn key(&mut self, key: &str) {
        if !self.open {
            return;
        }
        match key {
            "Escape" => self.finish(false),
            "Enter" | "NumpadEnter" => self.finish(true),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                if self.autosave && self.drag.is_some() {
                    return;
                }
                if let Some(index) = self.selected {
                    let mut at = cards::origin(&self.draft.cards[index], self.viewport);
                    let axis = usize::from(matches!(key, "ArrowUp" | "ArrowDown"));
                    at[axis] += if matches!(key, "ArrowLeft" | "ArrowUp") {
                        -1.
                    } else {
                        1.
                    } * if self.snap { 8. } else { 1. };
                    self.move_card(index, at);
                    self.checkpoint();
                }
            }
            _ => {}
        }
    }
}
