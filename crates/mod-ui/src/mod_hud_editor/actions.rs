//! Studio toolbar actions that do not capture a pointer gesture.

use super::*;
use ui::mod_panel::{Control, Event};

impl HudEditor {
    /// Routes a bounded surface action; `None` leaves card capture to pointer input.
    pub(super) fn action(&mut self, action: &str, controls: &mut [Control]) -> Option<Vec<Event>> {
        if let Some(index) = action
            .strip_prefix("hud.control:")
            .and_then(|s| s.parse::<usize>().ok())
        {
            self.cancel_pointer_input();
            let control = controls.get_mut(index)?;
            let value = match control {
                Control::Button { .. } => 1.,
                Control::Toggle { value, .. } => {
                    *value = !*value;
                    f32::from(u8::from(*value))
                }
                _ => return Some(Vec::new()),
            };
            return Some(vec![Event {
                id: control.id().to_owned(),
                value,
            }]);
        }
        match action {
            "hud.page_prev" | "hud.page_next" | "hud.toolbar_prev" | "hud.toolbar_next"
                if self.history_buttons =>
            {
                self.cancel_pointer_input();
                let layout = template::host::Layout::new(
                    self.viewport,
                    self.draft.cards.len(),
                    self.chrome_scale,
                );
                let (page, pages) = if action.starts_with("hud.toolbar_") {
                    (&mut self.toolbar_page, layout.toolbar_pages)
                } else {
                    (&mut self.handle_page, layout.pages)
                };
                *page = if action.ends_with("_prev") {
                    page.saturating_sub(1)
                } else {
                    page.saturating_add(1).min(pages.saturating_sub(1))
                };
                self.data_dirty = true;
            }
            "hud.blocked" if self.history_buttons => self.cancel_pointer_input(),
            "hud.grid" | "hud.snap" => {
                self.cancel_drag();
                if action == "hud.grid" {
                    self.grid = !self.grid;
                } else {
                    self.snap = !self.snap;
                }
                self.data_dirty = true;
            }
            "hud.undo" => self.step_history(false),
            "hud.redo" => self.step_history(true),
            "hud.scale_down" | "hud.scale_up" => {
                self.cancel_pointer_input();
                self.settle_anchor_motion();
                if let Some(index) = self.selected {
                    let card = &mut self.draft.cards[index];
                    let scale = (card.scale
                        + if action == "hud.scale_up" {
                            0.05
                        } else {
                            -0.05
                        })
                    .clamp(ui::mod_hud::MIN_CARD_SCALE, ui::mod_hud::MAX_CARD_SCALE);
                    if card.scale != scale {
                        card.scale = scale;
                        self.catalog = None;
                        self.data_dirty = true;
                        self.checkpoint();
                    }
                }
            }
            _ => {
                let (kind, index) = action.strip_prefix("hud.")?.split_once(':')?;
                let index = index.parse::<usize>().ok()?;
                match kind {
                    "select" if index < self.draft.cards.len() => {
                        self.cancel_pointer_input();
                        self.selected = Some(index);
                        self.data_dirty = true;
                    }
                    "visibility" if index < self.draft.cards.len() => {
                        self.cancel_pointer_input();
                        self.settle_anchor_motion();
                        let card = &self.draft.cards[index];
                        if !self.hidden.remove(&card.id) {
                            self.hidden.insert(card.id.clone());
                            self.toast =
                                motion::Toast::new(format!("{} hidden", card.editor_name()));
                        } else {
                            self.toast = Default::default();
                        }
                        self.data_dirty = true;
                        self.checkpoint();
                    }
                    "anchor" => {
                        self.cancel_pointer_input();
                        let anchor = *ui::mod_hud::Anchor::ALL.get(index)?;
                        if let Some(index) = self.selected {
                            let from = self.card_origin(index);
                            let card = &mut self.draft.cards[index];
                            card.anchor = anchor;
                            card.offset = anchor.editor_offset(EDITOR_MARGIN as f32);
                            card.position = None;
                            self.anchor_motion = Some(motion::AnchorMotion::new(index, from));
                            self.data_dirty = true;
                            self.checkpoint();
                        }
                    }
                    _ => return None,
                }
            }
        }
        Some(Vec::new())
    }
}
