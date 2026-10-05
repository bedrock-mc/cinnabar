use std::collections::BTreeSet;

use crate::{EntityRenderData, EntityRenderMaterial};

/// Dissolve depth masks retain fractional alpha because their threshold changes each frame.
pub fn actor_dissolve_mask_sources(render: &EntityRenderData) -> BTreeSet<u32> {
    render
        .layers
        .iter()
        .filter(|layer| layer.material == EntityRenderMaterial::DissolveDepth)
        .flat_map(|layer| {
            render
                .slots
                .get(layer.first_slot as usize..)
                .unwrap_or_default()
                .iter()
                .take(usize::from(layer.slot_count))
        })
        .flat_map(|slot| {
            render
                .candidates
                .get(slot.first_candidate as usize..)
                .unwrap_or_default()
                .iter()
                .take(usize::from(slot.candidate_count))
        })
        .map(|candidate| candidate.source)
        .collect()
}
