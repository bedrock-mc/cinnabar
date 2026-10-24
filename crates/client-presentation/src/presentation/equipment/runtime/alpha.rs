//! Held alpha modes follow the admitted model materials, independently of their grips.

use super::*;
use render_model::HandItemAlphaMode;

impl EquipmentRuntime {
    /// Uses session cube materials, then cutout block-entity models or startup block materials.
    pub(super) fn first_person_alpha_mode(
        &self,
        item: &WornItem,
        block: bool,
    ) -> HandItemAlphaMode {
        if !block {
            return HandItemAlphaMode::Cutout;
        }
        self.session_block_alpha(&item.identifier)
            .or_else(|| {
                HeldBlockEntity::from_item(&item.identifier)
                    .filter(|kind| self.held_block_entities.models.contains_key(kind))
                    .map(|_| HandItemAlphaMode::Cutout)
            })
            .or_else(|| match item.kind {
                HeldKind::Block(visual) => self.block_alpha_mode(visual),
                _ => None,
            })
            .unwrap_or_default()
    }
}
