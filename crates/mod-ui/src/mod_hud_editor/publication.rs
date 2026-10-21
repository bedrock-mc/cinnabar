//! Live card content updates retain the editor's placement authority.

use super::*;
use ui::mod_hud::{Card, Placement};

/// Matches stable identity and outer geometry for retained pointer and anchor animation state.
pub(super) fn matching_geometry(old: &Card, next: &[Card]) -> Option<usize> {
    next.iter()
        .position(|card| card.id == old.id && cards::dimensions(card) == cards::dimensions(old))
}

impl ModUi {
    /// Refreshes a live editor preview while preserving placement, visibility and undo history.
    pub fn update_mod_hud_editor(&mut self, preview: &Hud) -> Result<(), String> {
        preview.validate()?;
        let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) else {
            return Err("HUD editor is not open".into());
        };
        editor.republish_cards(&preview.cards, true);
        if editor.draft.inspector_size != preview.inspector_size {
            editor.draft.inspector_size = preview.inspector_size;
            editor.data_dirty = true;
        }
        let definition_changed = match (&editor.draft.surface, &preview.surface) {
            (Some(old), Some(new)) => !old.same_definition(new),
            (None, None) => false,
            _ => true,
        };
        let rebuild = definition_changed
            || editor.draft.editor_style != preview.editor_style
            || editor.draft.resizable != preview.resizable
            || editor.draft.hide_editor_labels != preview.hide_editor_labels;
        if editor.draft.surface != preview.surface
            || editor.draft.editor_style != preview.editor_style
            || editor.draft.resizable != preview.resizable
            || editor.draft.hide_editor_labels != preview.hide_editor_labels
        {
            editor.draft.surface.clone_from(&preview.surface);
            editor.draft.editor_style = preview.editor_style;
            editor.draft.resizable = preview.resizable;
            editor.draft.hide_editor_labels = preview.hide_editor_labels;
            if rebuild {
                editor.catalog = None;
            }
            if definition_changed {
                editor.transitions = Default::default();
            }
            editor.data_dirty = true;
        }
        Ok(())
    }
}

/// Captures factory defaults once and shares native placeholder policy with the HUD owner.
pub(super) fn prepare_card(card: &mut Card) {
    card.reset_anchor.get_or_insert(card.anchor);
    card.reset_offset.get_or_insert(card.offset);
    card.reset_scale.get_or_insert(card.scale);
    card.ensure_editor_placeholder();
}

/// Applies editor-owned state to newly published artwork without retaining stale content.
fn preview_card(published: &Card, old: Option<&Card>) -> Card {
    let mut card = published.clone();
    if let Some(old) = old {
        Placement::capture(old).apply_to(&mut card);
        card.reset_anchor = old.reset_anchor;
        card.reset_offset = old.reset_offset;
        card.reset_scale = old.reset_scale;
        card.editor_native |= old.editor_native || old.source.is_some();
        if let (Some(old), Some(new)) = (&old.text_plate, &mut card.text_plate)
            && old == new
        {
            new.layout.clone_from(&old.layout);
        }
    }
    prepare_card(&mut card);
    card
}

impl HudEditor {
    /// Applies live artwork by ID; gameplay omissions retain disabled editor previews.
    pub(in super::super) fn republish_cards(&mut self, published: &[Card], replace_all: bool) {
        let selected = self
            .selected
            .map(|index| self.draft.cards[index].id.clone());
        let mut next: Vec<_> = if replace_all {
            published
                .iter()
                .map(|card| {
                    preview_card(card, self.draft.cards.iter().find(|old| old.id == card.id))
                })
                .collect()
        } else {
            self.draft
                .cards
                .iter()
                .map(|old| {
                    preview_card(
                        published
                            .iter()
                            .find(|card| card.id == old.id && (!old.sample || card.sample))
                            .unwrap_or(old),
                        Some(old),
                    )
                })
                .collect()
        };
        if next == self.draft.cards {
            return;
        }
        if let Some(index) = self.reconcile_drag(&next) {
            let old = &self.draft.cards[index];
            if let Some(card) = next.iter_mut().find(|card| card.id == old.id) {
                Placement::capture(old).apply_to(card);
            }
        }
        if self
            .anchor_motion
            .as_mut()
            .is_some_and(|motion| !motion.rebase(&self.draft.cards, &next))
        {
            self.anchor_motion = None;
        }
        let rebuild = !cards::same_cards_shape(&self.draft.cards, &next);
        if rebuild {
            self.catalog = None;
            self.snap_dirty = true;
        }
        for card in &next {
            if !card.visible && !self.draft.cards.iter().any(|old| old.id == card.id) {
                self.hidden.insert(card.id.clone());
                self.committed_hidden.insert(card.id.clone());
            }
        }
        self.committed.cards = next
            .iter()
            .map(|card| {
                let mut committed = card.clone();
                if let Some(old) = self.committed.cards.iter().find(|old| old.id == card.id) {
                    Placement::capture(old).apply_to(&mut committed);
                }
                committed
            })
            .collect();
        self.draft.cards = next;
        self.hidden
            .retain(|id| self.draft.cards.iter().any(|card| &card.id == id));
        self.committed_hidden
            .retain(|id| self.draft.cards.iter().any(|card| &card.id == id));
        self.selected =
            selected.and_then(|id| self.draft.cards.iter().position(|card| card.id == id));
        self.history.refresh(&self.draft, &self.hidden);
        self.data_dirty = true;
        if rebuild && self.drag.is_none() {
            self.frame = None;
        }
    }
}
