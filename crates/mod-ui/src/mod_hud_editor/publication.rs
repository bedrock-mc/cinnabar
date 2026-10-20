//! Live card content updates retain the editor's placement authority.

use super::*;
use ui::mod_hud::{Card, Placement};

impl ModUi {
    /// Refreshes a live editor preview while preserving placement, visibility and undo history.
    pub fn update_mod_hud_editor(&mut self, preview: &Hud) -> Result<(), String> {
        preview.validate()?;
        let Some(editor) = self.mod_hud_editor.as_mut().filter(|e| e.open) else {
            return Err("HUD editor is not open".into());
        };
        editor.republish_cards(&preview.cards, true);
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

impl HudEditor {
    /// Applies an owner's republished content by ID; gameplay omissions keep disabled previews.
    pub(in super::super) fn republish_cards(&mut self, published: &[Card], replace_all: bool) {
        let selected = self
            .selected
            .map(|index| self.draft.cards[index].id.clone());
        let mut next = self.draft.clone();
        if replace_all {
            next.cards = published.to_vec();
        } else {
            for card in &mut next.cards {
                if let Some(new) = published.iter().find(|new| new.id == card.id) {
                    *card = new.clone();
                }
            }
        }
        for card in &mut next.cards {
            if let Some(old) = self.draft.cards.iter().find(|old| old.id == card.id) {
                Placement::capture(old).apply_to(card);
                card.reset_anchor = old.reset_anchor;
                card.reset_offset = old.reset_offset;
                card.reset_scale = old.reset_scale;
                if let (Some(old), Some(new)) = (&old.text_plate, &mut card.text_plate)
                    && old == new
                {
                    new.layout.clone_from(&old.layout);
                }
            } else {
                card.reset_anchor.get_or_insert(card.anchor);
                card.reset_offset.get_or_insert(card.offset);
                card.reset_scale.get_or_insert(card.scale);
            }
        }
        if next == self.draft {
            return;
        }
        // Captured resize geometry belongs to the previous artwork.
        self.cancel_drag();
        for card in &mut next.cards {
            if let Some(old) = self.draft.cards.iter().find(|old| old.id == card.id) {
                Placement::capture(old).apply_to(card);
            }
        }
        if !cards::same_shape(&self.draft, &next) {
            self.catalog = None;
        }
        self.draft = next;
        self.hidden
            .retain(|id| self.draft.cards.iter().any(|card| &card.id == id));
        self.selected =
            selected.and_then(|id| self.draft.cards.iter().position(|card| card.id == id));
        self.anchor_motion = None;
        self.history.refresh(&self.draft, &self.hidden);
        // Update committed content too, so cancelling a later drag cannot restore stale artwork.
        let mut committed = self.draft.clone();
        for card in &mut committed.cards {
            if let Some(old) = self.committed.cards.iter().find(|old| old.id == card.id) {
                Placement::capture(old).apply_to(card);
            }
        }
        self.committed = committed;
        self.data_dirty = true;
        self.frame = None;
    }
}
