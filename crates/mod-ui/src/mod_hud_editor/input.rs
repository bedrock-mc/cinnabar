use super::*;
use ui::mod_hud::Placement;

#[cfg(test)]
mod tests;

pub(in super::super) struct Drag {
    index: usize,
    grab: [f64; 2],
    start: [f64; 2],
    last: [f64; 2],
    moved: bool,
    resize: Option<Resize>,
    pub(super) targets: [Option<usize>; 2],
    pub(super) guides: [Option<f64>; 2],
}

/// Captures the original geometry and opposite corner for one uniform resize.
#[derive(Clone, Copy)]
struct Resize {
    corner: usize,
    scale: f64,
    size: [f64; 2],
    fixed: [f64; 2],
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
                    .map(|c| {
                        let at = cards::origin(c, self.viewport);
                        let size = cards::dimensions(c);
                        let (anchor, offset) = if c.position.is_some() {
                            ui::mod_hud::Anchor::from_origin(at, size, self.viewport)
                        } else {
                            (c.anchor, c.offset)
                        };
                        Placement {
                            id: c.id.clone(),
                            anchor,
                            offset,
                            position: Some(c.position.unwrap_or_else(|| {
                                std::array::from_fn(|axis| {
                                    let available = (self.viewport[axis] - size[axis]).max(0.);
                                    if available > 0. {
                                        (at[axis] / available) as f32
                                    } else {
                                        0.
                                    }
                                })
                            })),
                            scale: c.scale,
                        }
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
        self.data_dirty |= self.drag.is_some();
        if let Some(drag) = self.drag.take()
            && self.autosave
        {
            self.draft.cards[drag.index] = self.committed.cards[drag.index].clone();
            if drag.resize.is_some() {
                self.catalog = None;
            }
        }
    }
    /// Releases pointer feedback and rolls back an unfinished captured gesture.
    pub(in super::super) fn cancel_pointer_input(&mut self) {
        self.data_dirty |= self.view.hovered.is_some();
        self.cancel_drag();
        self.view.hovered = None;
        self.view.pressed = None;
        self.view.pointer = None;
    }
    /// Converts a clamped GUI-pixel top-left into normalized available travel.
    fn move_card(&mut self, index: usize, at: [f64; 2]) {
        self.data_dirty = true;
        let card = &mut self.draft.cards[index];
        let size = cards::dimensions(card);
        card.position = Some(std::array::from_fn(|axis| {
            let available = (self.viewport[axis] - size[axis]).max(0.);
            let at = if self.snap
                && !self.bypass_snap
                && self
                    .drag
                    .as_ref()
                    .is_none_or(|drag| drag.guides[axis].is_none())
            {
                (at[axis] / snapping::GRID).round() * snapping::GRID
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
    /// Stores a completed move or resize as an anchor without changing its current origin.
    fn anchor_card(&mut self, index: usize) {
        let card = &mut self.draft.cards[index];
        let at = cards::origin(card, self.viewport);
        (card.anchor, card.offset) =
            ui::mod_hud::Anchor::from_origin(at, cards::dimensions(card), self.viewport);
        card.position = None;
    }
    /// Restores explicit factory values or the authored anchor and offset captured on open.
    fn reset_layout(&mut self) {
        self.data_dirty = true;
        for card in &mut self.draft.cards {
            card.anchor = card.reset_anchor.unwrap_or(card.anchor);
            card.offset = card.reset_offset.unwrap_or(card.offset);
            card.position = None;
            card.scale = card.reset_scale.unwrap_or(card.scale);
        }
        self.catalog = None;
        self.reset = true;
        self.drag = None;
        self.checkpoint();
    }
    /// Scales from the captured corner while retaining the opposite corner and viewport bounds.
    fn resize_card(&mut self, index: usize, resize: Resize, delta: [f64; 2]) {
        self.data_dirty = true;
        let base: [f64; 2] = resize.size.map(|size| size / resize.scale);
        let signed: [f64; 2] = std::array::from_fn(|axis| {
            if resize.corner & (1 << axis) == 0 {
                -base[axis]
            } else {
                base[axis]
            }
        });
        let projection =
            (delta[0] * signed[0] + delta[1] * signed[1]) / (base[0].powi(2) + base[1].powi(2));
        let max_scale = (self.viewport[0] / base[0])
            .min(self.viewport[1] / base[1])
            .clamp(
                f64::from(ui::mod_hud::MIN_CARD_SCALE),
                f64::from(ui::mod_hud::MAX_CARD_SCALE),
            );
        let scale = (resize.scale + projection)
            .clamp(f64::from(ui::mod_hud::MIN_CARD_SCALE), max_scale) as f32;
        let changed = self.draft.cards[index].scale != scale;
        self.draft.cards[index].scale = scale;
        let size = cards::dimensions(&self.draft.cards[index]);
        self.draft.cards[index].position = Some(std::array::from_fn(|axis| {
            let available = (self.viewport[axis] - size[axis]).max(0.);
            let at = resize.fixed[axis]
                - if resize.corner & (1 << axis) == 0 {
                    size[axis]
                } else {
                    0.
                };
            if available > 0. {
                (at.clamp(0., available) / available) as f32
            } else {
                0.
            }
        }));
        if changed {
            self.catalog = None;
        }
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
            self.data_dirty = true;
            self.view.hovered = hovered.map(str::to_owned);
        }
        if pressed {
            self.data_dirty = true;
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
                self.reset_layout();
                return Vec::new();
            }
            Some("hud.grid") => {
                self.data_dirty = true;
                self.snap = !self.snap;
                self.cancel_drag();
                return Vec::new();
            }
            _ => {}
        }
        if pressed {
            self.cancel_drag();
            let capture = action.as_deref().and_then(|action| {
                if let Some(index) = action.strip_prefix("hud.card:") {
                    return index.parse::<usize>().ok().map(|index| (index, None));
                }
                let (index, corner) = action.strip_prefix("hud.resize:")?.split_once(':')?;
                let corner = corner.parse::<usize>().ok().filter(|&corner| corner < 4)?;
                self.draft
                    .resizable
                    .then_some((index.parse::<usize>().ok()?, Some(corner)))
            });
            self.drag = capture
                .filter(|(index, _)| *index < self.draft.cards.len())
                .map(|(index, corner)| {
                    let card = &self.draft.cards[index];
                    let at = cards::origin(card, self.viewport);
                    let size = cards::dimensions(card);
                    Drag {
                        index,
                        grab: std::array::from_fn(|axis| point[axis] - at[axis]),
                        start: point,
                        last: point,
                        moved: false,
                        guides: [None; 2],
                        targets: [None; 2],
                        resize: corner.map(|corner| Resize {
                            corner,
                            scale: f64::from(card.scale),
                            size,
                            fixed: std::array::from_fn(|axis| {
                                at[axis]
                                    + if corner & (1 << axis) == 0 {
                                        size[axis]
                                    } else {
                                        0.
                                    }
                            }),
                        }),
                    }
                });
            self.selected = self.drag.as_ref().map(|drag| drag.index);
        }
        if let Some(drag) = &mut self.drag {
            drag.moved |= (0..2).any(|axis| (point[axis] - drag.start[axis]).abs() > 0.001);
            let index = drag.index;
            let at = std::array::from_fn(|axis| point[axis] - drag.grab[axis]);
            let changed = drag.last != point || self.snap_dirty;
            drag.last = point;
            self.snap_dirty = false;
            let moved = drag.moved;
            if !pressed && moved && changed {
                if let Some(resize) = drag.resize {
                    let delta: [f64; 2] =
                        std::array::from_fn(|axis| point[axis] - drag.start[axis]);
                    self.resize_card(index, resize, delta);
                } else {
                    let targets = snapping::targets(
                        &self.draft,
                        index,
                        at,
                        self.viewport,
                        &self.keep_out,
                        self.bypass_snap,
                    );
                    let mut aligned = at;
                    for axis in 0..2 {
                        drag.guides[axis] = targets[axis]
                            .map(|target| target.guide.min(self.viewport[axis] - GUIDE_WIDTH));
                        drag.targets[axis] = targets[axis].and_then(|target| target.card);
                        if let Some(target) = targets[axis] {
                            aligned[axis] = target.at;
                        }
                    }
                    self.move_card(index, aligned);
                }
            }
        }
        if !held && let Some(drag) = self.drag.take().filter(|drag| drag.moved) {
            self.data_dirty = true;
            self.anchor_card(drag.index);
            self.checkpoint();
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
                    } * if self.snap && !self.bypass_snap {
                        snapping::GRID
                    } else {
                        1.
                    };
                    self.move_card(index, at);
                    self.anchor_card(index);
                    self.checkpoint();
                }
            }
            _ => {}
        }
    }
}
