use std::{collections::BTreeMap, sync::Arc};

use assets::{BiomeRule, RuntimeAssets};
use chunk_pipeline::WorldStream;
use protocol::BiomeDefinitionEvent;
use render::PRECIPITATION_SAMPLE_OFFSETS;

/// The fog and weather renderers use the same native precipitation lattice.
fn sample_positions(position: [f32; 3]) -> [[f32; 3]; PRECIPITATION_SAMPLE_OFFSETS.len()] {
    let origin = position.map(f32::floor);
    PRECIPITATION_SAMPLE_OFFSETS
        .map(|offset| std::array::from_fn(|axis| origin[axis] + offset[axis] as f32))
}

/// Samples the current client's 27-position biome layer.
pub(crate) fn fog_biome_samples(
    stream: &WorldStream,
    assets: &RuntimeAssets,
    position: [f32; 3],
) -> Vec<Option<Box<str>>> {
    let rules = &assets.biome_assets().rules;
    sample_positions(position)
        .into_iter()
        .map(|position| {
            let id = stream.camera_biome_id(position)?;
            let index = rules.binary_search_by_key(&id, |rule| rule.id).ok()?;
            Some(rules[index].name.clone())
        })
        .collect()
}

/// Biome precipitation starts enabled, and climate application
/// disables it when the optional downfall is below the float epsilon.
/// Unknown/non-finite climate is not assigned a fabricated precipitation flag.
fn precipitation_eligible(downfall: f32) -> Option<bool> {
    downfall.is_finite().then_some(downfall >= f32::EPSILON)
}

fn precipitation_registry(
    rules: &[BiomeRule],
    definitions: &[BiomeDefinitionEvent],
) -> BTreeMap<u32, Option<bool>> {
    let mut by_id: BTreeMap<_, _> = rules
        .iter()
        .map(|rule| (rule.id, precipitation_eligible(rule.downfall())))
        .collect();
    let by_name: BTreeMap<_, _> = rules
        .iter()
        .map(|rule| (rule.name.as_ref(), rule.id))
        .collect();
    for definition in definitions {
        // Match the committed assets resolver: a native name binds to its
        // canonical palette ID; otherwise a custom biome needs an explicit ID.
        let id = by_name.get(definition.name.as_ref()).copied().or_else(|| {
            definition
                .biome_id
                .map(u32::from)
                .filter(|id| !by_id.contains_key(id))
        });
        if let Some(id) = id {
            by_id.insert(id, precipitation_eligible(definition.downfall));
        }
    }
    by_id
}

fn count_precipitation_samples(samples: impl IntoIterator<Item = Option<bool>>) -> Option<usize> {
    let mut known = false;
    let mut count = 0;
    for eligible in samples.into_iter().flatten() {
        known = true;
        count += usize::from(eligible);
    }
    known.then_some(count)
}

/// Climate admission is refreshed on registry publication, not rebuilt every
/// render frame. Sampling stays bounded to the native lattice; absent columns
/// contribute no precipitation and never cause a denominator renormalization.
#[derive(Default)]
pub(crate) struct FogPrecipitationSamples {
    definitions: Option<Arc<[BiomeDefinitionEvent]>>,
    biome_registry_sha256: Option<[u8; 32]>,
    eligibility: BTreeMap<u32, Option<bool>>,
}

impl FogPrecipitationSamples {
    pub(crate) fn count(
        &mut self,
        stream: &WorldStream,
        assets: &RuntimeAssets,
        position: [f32; 3],
    ) -> Option<usize> {
        let definitions = stream.biome_definitions_snapshot();
        let registry = assets.provenance().biome_registry_sha256;
        if self.biome_registry_sha256 != Some(registry)
            || self
                .definitions
                .as_ref()
                .is_none_or(|previous| !Arc::ptr_eq(previous, &definitions))
        {
            self.eligibility = precipitation_registry(&assets.biome_assets().rules, &definitions);
            self.definitions = Some(definitions);
            self.biome_registry_sha256 = Some(registry);
        }
        count_precipitation_samples(sample_positions(position).map(|position| {
            let id = stream.camera_biome_id(position)?;
            self.eligibility.get(&id).copied().flatten()
        }))
    }
}

#[cfg(test)]
mod tests;
