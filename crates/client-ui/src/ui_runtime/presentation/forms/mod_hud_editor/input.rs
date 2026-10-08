use super::*;
use ui::mod_hud::Placement;

pub(in super::super) struct Drag {
    index: usize,
    grab: [f64; 2],
    start: [f64; 2],
    moved: bool,
}

impl HudEditor {
    pub(super) fn finish(&mut self, saved: bool) {
        self.result = Some(EditorResult {
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
        });
        self.open = false;
        self.frame = None;
        self.drag = None;
        self.view = ViewState::default();
    }
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
    pub(in super::super) fn pointer(&mut self, position: [f32; 2], pressed: bool, held: bool) {
        if !self.open || !position.into_iter().all(f32::is_finite) {
            self.drag = None;
            return;
        }
        let Some(frame) = self.frame.as_ref() else {
            return;
        };
        let point = std::array::from_fn(|axis| {
            f64::from((position[axis] - frame.origin[axis]) / frame.scale)
        });
        let action = pressed
            .then(|| {
                frame
                    .hits
                    .iter()
                    .rev()
                    .find(|hit| hit.enabled && hit.pressed.is_some() && hit.contains(point))
            })
            .flatten()
            .and_then(|hit| hit.pressed.clone());
        match action.as_deref() {
            Some("hud.save") => {
                self.finish(true);
                return;
            }
            Some("hud.cancel") => {
                self.finish(false);
                return;
            }
            Some("hud.reset") => {
                for card in &mut self.draft.cards {
                    card.anchor = card.reset_anchor.unwrap_or(card.anchor);
                    card.offset = card.reset_offset.unwrap_or(card.offset);
                    card.position = None;
                }
                self.reset = true;
                self.drag = None;
                return;
            }
            Some("hud.grid") => {
                self.snap = !self.snap;
                self.drag = None;
                return;
            }
            _ => {}
        }
        if pressed {
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
            self.drag = None;
        }
    }
    pub(in super::super) fn key(&mut self, key: &str) {
        if !self.open {
            return;
        }
        match key {
            "Escape" => self.finish(false),
            "Enter" | "NumpadEnter" => self.finish(true),
            "ArrowLeft" | "ArrowRight" | "ArrowUp" | "ArrowDown" => {
                if let Some(index) = self.selected {
                    let mut at = cards::origin(&self.draft.cards[index], self.viewport);
                    let axis = usize::from(matches!(key, "ArrowUp" | "ArrowDown"));
                    at[axis] += if matches!(key, "ArrowLeft" | "ArrowUp") {
                        -1.
                    } else {
                        1.
                    } * if self.snap { 8. } else { 1. };
                    self.move_card(index, at);
                }
            }
            _ => {}
        }
    }
}
