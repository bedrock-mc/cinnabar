use std::collections::{BTreeMap, BTreeSet};

use crate::{EntityAssetSource, EntityRenderData, EntityRenderLayer, EntityRenderMaterial};

use super::native_actor_texture_preserves_fractional_alpha;

/// Dissolve depth masks retain fractional alpha because their threshold changes each frame.
fn actor_dissolve_mask_sources(render: &EntityRenderData) -> BTreeSet<u32> {
    render
        .layers
        .iter()
        .filter(|layer| layer.material == EntityRenderMaterial::DissolveDepth)
        .flat_map(|layer| layer_sources(render, layer))
        .collect()
}

/// Sources whose fractional alpha reaches the shader: dissolve masks, witnessed colour-mask and
/// three-sampler rasters, art any drawing material blends or lights by alpha, and art every
/// drawing material alpha-tests. Every other raster's alpha rounds at 0.5.
pub fn actor_sources_keeping_alpha(
    render: &EntityRenderData,
    sources: &[EntityAssetSource],
) -> BTreeSet<u32> {
    // Per source: (every drawing material alpha-tests, some drawing material reads alpha).
    let mut drawn = BTreeMap::<u32, (bool, bool)>::new();
    for layer in &render.layers {
        let state = layer.material_state;
        for source in layer_sources(render, layer) {
            let (tests, reads) = drawn.entry(source).or_insert((true, false));
            *tests &= state.is_some_and(|state| state.alpha_test);
            *reads |= state.is_some_and(|state| state.blend || state.emissive);
        }
    }
    let mut keep = actor_dissolve_mask_sources(render);
    keep.extend(
        drawn
            .into_iter()
            .filter(|(_, (tests, reads))| *tests || *reads)
            .map(|(source, _)| source),
    );
    keep.extend(
        sources
            .iter()
            .enumerate()
            .filter(|(_, source)| native_actor_texture_preserves_fractional_alpha(source))
            .map(|(index, _)| index as u32),
    );
    keep
}

fn layer_sources<'a>(
    render: &'a EntityRenderData,
    layer: &'a EntityRenderLayer,
) -> impl Iterator<Item = u32> + 'a {
    render
        .slots
        .get(layer.first_slot as usize..)
        .unwrap_or_default()
        .iter()
        .take(usize::from(layer.slot_count))
        .flat_map(|slot| {
            render
                .candidates
                .get(slot.first_candidate as usize..)
                .unwrap_or_default()
                .iter()
                .take(usize::from(slot.candidate_count))
        })
        .map(|candidate| candidate.source)
}
