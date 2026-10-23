//! Host-measured geometry can change with the viewport without becoming a user edit.

use super::*;
use ui::mod_hud::{Card, Placement};

impl HudEditor {
    /// Replaces fixed control geometry and factory anchors, retaining custom travel and history.
    /// The host must supply the same IDs; active pointer edits roll back before reflowing.
    pub fn reflow_factory(&mut self, geometry: &[Card]) -> Result<(), String> {
        let mut measured = self.draft.clone();
        measured.cards = geometry.to_vec();
        measured.validate()?;
        if geometry.len() != self.draft.cards.len()
            || geometry
                .iter()
                .any(|card| !self.draft.cards.iter().any(|old| old.id == card.id))
        {
            return Err("Factory reflow must retain control identities".into());
        }
        self.cancel_pointer_input();
        self.settle_anchor_motion();
        let old = self.draft.cards.clone();
        let rebase = |placement: &mut Placement| {
            let previous = old.iter().find(|card| card.id == placement.id).unwrap();
            let next = geometry
                .iter()
                .find(|card| card.id == placement.id)
                .unwrap();
            rebase_placement(placement, previous, next, self.viewport);
        };
        self.history.reflow_factory(rebase);
        for hud in [&mut self.draft, &mut self.committed] {
            for card in &mut hud.cards {
                let mut placement = Placement::capture(card);
                rebase(&mut placement);
                let next = geometry.iter().find(|next| next.id == card.id).unwrap();
                card.clone_from(next);
                placement.apply_to(card);
            }
        }
        self.anchor_motion = None;
        self.catalog = None;
        self.frame = None;
        self.data_dirty = true;
        self.snap_dirty = true;
        Ok(())
    }
}

/// Untouched defaults follow the host; custom placements retain fractions of available travel.
fn rebase_placement(placement: &mut Placement, old: &Card, next: &Card, viewport: [f64; 2]) {
    let factory = placement.position.is_none()
        && placement.anchor == old.reset_anchor.unwrap_or(old.anchor)
        && placement.offset == old.reset_offset.unwrap_or(old.offset);
    if !factory && placement.position.is_none() {
        let mut card = old.clone();
        placement.apply_to(&mut card);
        let at = cards::origin(&card, viewport);
        let size = cards::dimensions(&card);
        placement.position = Some(std::array::from_fn(|axis| {
            let travel = (viewport[axis] - size[axis]).max(0.);
            if travel > 0. {
                (at[axis] / travel).clamp(0., 1.) as f32
            } else {
                0.
            }
        }));
    }
    placement.anchor = next.anchor;
    placement.offset = next.offset;
}
