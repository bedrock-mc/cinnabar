//! Independently versioned equipment and effect observations, prepared once per callback.

use super::cinnabar::extension::player_state::{Effects, Equipment, Revisions};
use crate::PlayerStateSnapshot;

#[derive(Default)]
pub(super) struct SplitState {
    pub equipment: Option<Equipment>,
    pub effects: Option<Effects>,
    current: Option<Revisions>,
    observed: bool,
    sequence: u64,
    equipment_revision: u64,
    effects_revision: u64,
}

impl SplitState {
    /// Retains exact comparisons while requiring fresh current-session authority next callback.
    pub fn begin_frame(&mut self) {
        self.observed = false;
        self.current = None;
    }

    /// Resolves tokens without copying unchanged inventory strings or vectors.
    pub fn observe(
        &mut self,
        snapshot: Option<&PlayerStateSnapshot>,
    ) -> anyhow::Result<Option<Revisions>> {
        if self.observed {
            return Ok(self.current);
        }
        self.observed = true;
        let Some(s) = snapshot else {
            self.equipment = None;
            self.effects = None;
            return Ok(None);
        };
        if self.equipment.as_ref().is_none_or(|e| {
            e.session != s.session
                || e.dimension != s.dimension
                || e.selected_slot != s.selected_slot
                || e.inventory != s.inventory
                || e.armor != s.armor
                || e.offhand != s.offhand
        }) {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("equipment revision exhausted"))?;
            self.equipment_revision = self.sequence;
            self.equipment = Some(Equipment {
                session: s.session,
                dimension: s.dimension,
                selected_slot: s.selected_slot,
                inventory: s.inventory.clone(),
                armor: s.armor.clone(),
                offhand: s.offhand.clone(),
            });
        }
        if self.effects.as_ref().is_none_or(|e| {
            e.session != s.session || e.dimension != s.dimension || e.effects != s.effects
        }) {
            self.sequence = self
                .sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("effects revision exhausted"))?;
            self.effects_revision = self.sequence;
            self.effects = Some(Effects {
                session: s.session,
                dimension: s.dimension,
                effects: s.effects.clone(),
            });
        }
        self.current = Some(Revisions {
            session: s.session,
            dimension: s.dimension,
            equipment: self.equipment_revision,
            effects: self.effects_revision,
        });
        Ok(self.current)
    }
}
