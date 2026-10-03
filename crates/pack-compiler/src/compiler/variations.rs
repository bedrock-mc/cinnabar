use super::*;

type Groups = Vec<(Descriptor, Vec<(Descriptor, f32)>)>;

/// Adds every weighted path to texture staging without changing state selection.
pub(super) fn expand(
    pack: &PackSources,
    descriptors: &mut BTreeMap<Descriptor, Box<str>>,
) -> Groups {
    let mut groups = Vec::new();
    for descriptor in descriptors.keys() {
        let Some(paths) = pack
            .terrain
            .position_variations
            .get(&(descriptor.texture_key.clone(), descriptor.state_variant))
        else {
            continue;
        };
        let alternatives = paths
            .iter()
            .map(|entry| {
                let mut alternative = descriptor.clone();
                alternative.path = entry.path.clone();
                (alternative, entry.weight)
            })
            .collect::<Vec<_>>();
        groups.push((descriptor.clone(), alternatives));
    }
    for (_, alternatives) in &groups {
        for (descriptor, _) in alternatives {
            descriptors.insert(descriptor.clone(), descriptor.texture_key.clone());
        }
    }
    groups
}

/// Appends independent leaf materials and a selector for each state/face group.
pub(super) fn install(
    materials: Box<[Material]>,
    mut descriptors: BTreeMap<Descriptor, u32>,
    groups: Groups,
) -> Result<CompiledMaterials, AssetError> {
    let mut materials = materials.into_vec();
    // Resolve all leaf IDs before replacing any descriptor with its selector.
    let originals = descriptors.clone();
    for (descriptor, alternatives) in groups {
        let start = materials.len() as u32;
        if materials.len() + alternatives.len() + 1 > MAX_MATERIALS {
            return Err(AssetError::TooManyMaterials {
                count: materials.len() + alternatives.len() + 1,
                max: MAX_MATERIALS,
            });
        }
        for (alternative, weight) in &alternatives {
            let id =
                originals
                    .get(alternative)
                    .ok_or_else(|| AssetError::InvalidCompiledAssets {
                        detail: format!(
                            "weighted texture {} could not be compiled",
                            alternative.path
                        )
                        .into(),
                    })?;
            let mut material = materials[*id as usize];
            material.variation_weight = weight.to_bits();
            materials.push(material);
        }
        let mut selector = materials[start as usize];
        selector.variation_start = start;
        selector.variation_count = alternatives.len() as u32;
        selector.variation_weight = 0;
        descriptors.insert(descriptor, materials.len() as u32);
        materials.push(selector);
    }
    Ok((materials.into_boxed_slice(), descriptors))
}
