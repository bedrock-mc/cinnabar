//! Held cube alpha modes follow the admitted materials, independently of their pixels.

use super::*;
use render::HandItemAlphaMode;

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
    pub(super) fn first_person_alpha_mode(
        &self,
        item: &WornItem,
        block: bool,
    ) -> HandItemAlphaMode {
        if !block {
            return HandItemAlphaMode::Cutout;
        }
        match item.kind {
            HeldKind::Block(visual) => self.block_alpha.get(&visual).copied().unwrap_or_default(),
            _ => self
                .session_block_alpha(&item.identifier)
                .unwrap_or_default(),
        }
    }
}
