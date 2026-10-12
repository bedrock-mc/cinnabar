//! Directional bubble shells and air-exposed surface sprites, independent of water.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies primary columns before the provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:bubble_column"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Reads the column's single typed direction selector.
fn drag_down(record: &RegistryRecord) -> Option<bool> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 1 {
        return None;
    }
    Some(super::state::exact_tagged_byte(state.get("drag_down")?, 1)? != 0)
}

/// Resolves four outer sprites, one inner sprite and four directional surface sprites.
pub(in crate::compiler) fn material_descriptors(
    pack: &PackSources,
    record: &RegistryRecord,
) -> Option<Vec<(Descriptor, Box<str>)>> {
    let down = drag_down(record)?;
    let mut result = Vec::with_capacity(9);
    for (face, variants) in [
        (BlockFace::North, if down { 4..8 } else { 0..4 }),
        (BlockFace::South, usize::from(down)..usize::from(down) + 1),
        (if down { BlockFace::Down } else { BlockFace::Up }, 0..4),
    ] {
        let TextureKey { key, .. } = resolve_texture_key(&pack.blocks, record, face);
        let key = key?;
        for variant in variants {
            let (path, state_variant) = pack.terrain.get_clamped_untinted_variant(&key, variant)?;
            result.push((
                Descriptor {
                    path: path.into(),
                    texture_key: key.clone(),
                    state_variant,
                    flipbook_selector: Some((state_variant, state_variant)),
                    flags: MATERIAL_FLAG_ALPHA_BLEND
                        | MATERIAL_FLAG_WATER_TINT
                        | assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
                },
                key.clone(),
            ));
        }
    }
    Some(result)
}

/// Compiles positional shell groups without sampling the auxiliary east/west slots.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 9], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    if drag_down(record).is_none() {
        return Ok(CompileRuleResult::Reject);
    }
    let mut visual = diagnostic_visual(record);
    let Some(descriptors) = material_descriptors(inputs.pack, record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let Some(materials) = descriptors
        .iter()
        .map(|(descriptor, _)| inputs.material_by_descriptor.get(descriptor).copied())
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let materials: [u32; 9] = materials.try_into().expect("column descriptor count");
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        let base = storage.templates.len() as u32;
        for offset in 0..assets::BUBBLE_COLUMN_TEMPLATE_COUNT {
            let variant = (offset % assets::BUBBLE_COLUMN_POSITION_VARIANTS) as usize;
            let mut quads = Vec::with_capacity(assets::bubble_column_quad_count(offset) as usize);
            for (material, inset) in [(materials[variant], 16), (materials[4], 80)] {
                for face in [
                    BlockFace::West,
                    BlockFace::East,
                    BlockFace::North,
                    BlockFace::South,
                ] {
                    let mut quad = super::geometry::vanilla_cuboid_quads(
                        [material; 6],
                        [inset, 0, inset],
                        [256 - inset, 256, 256 - inset],
                    )[face as usize];
                    quad.flags &= MODEL_QUAD_FLAG_FACE_MASK;
                    quads.push(quad);
                }
            }
            if offset >= assets::BUBBLE_COLUMN_POSITION_VARIANTS {
                let mut quad = super::geometry::vanilla_cuboid_quads(
                    [materials[5 + variant]; 6],
                    [0; 3],
                    [256; 3],
                )[BlockFace::Up as usize];
                quad.flags &= MODEL_QUAD_FLAG_FACE_MASK;
                quads.push(quad);
            }
            push_model_template(
                quads,
                assets::MODEL_TEMPLATE_FLAG_BUBBLE_COLUMN,
                storage.templates,
                storage.quads,
            )?;
        }
        templates.insert(materials, base);
        base
    };
    set_model_visual(&mut visual, [materials[0]; 6], template);
    Ok(CompileRuleResult::Compiled(visual))
}
