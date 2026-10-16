//! Held alpha modes follow the admitted model materials, independently of their grips.

use super::*;
use render_model::HandItemAlphaMode;

pub(super) fn mode_for_material_flags(flags: u32) -> HandItemAlphaMode {
    if flags & assets::MATERIAL_FLAG_ALPHA_BLEND != 0 {
        HandItemAlphaMode::Blend
    } else if flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT != 0 {
        HandItemAlphaMode::Cutout
    } else {
        HandItemAlphaMode::Opaque
    }
}

pub(super) fn block_alpha_modes(
    world: Option<&RuntimeAssets>,
    sheets: &BTreeMap<u32, usize>,
) -> BTreeMap<u32, HandItemAlphaMode> {
    let Some(world) = world else {
        return BTreeMap::new();
    };
    sheets
        .keys()
        .filter_map(|&visual| {
            if visual as usize >= world.visual_count() {
                return None;
            }
            let block = world.resolve(assets::NetworkIdMode::Sequential, visual);
            let flags = assets::BlockFace::ALL.into_iter().fold(0, |flags, face| {
                flags | world.material(block.face(face).material_id()).flags
            });
            let mode = mode_for_material_flags(flags);
            Some((visual, mode))
        })
        .collect()
}

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
