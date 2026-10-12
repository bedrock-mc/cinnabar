//! Brewing-stand post, feet and bottle-selected radial sprite panels.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Identifies the terrain model before its provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == "minecraft:brewing_stand"
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Admits exactly the three typed bottle occupancy selectors.
fn bottles(record: &RegistryRecord) -> Option<[bool; 3]> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 3 {
        return None;
    }
    let selector = |key| Some(super::state::exact_tagged_byte(state.get(key)?, 1)? != 0);
    Some([
        selector("brewing_stand_slot_a_bit")?,
        selector("brewing_stand_slot_b_bit")?,
        selector("brewing_stand_slot_c_bit")?,
    ])
}

/// Compiles bottle combinations with pack-resolved post, base and panel materials.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], [bool; 3]), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(bottles) = bottles(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, bottles);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(materials, bottles),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Emits the post and three feet, followed by double-sided panels facing each bottle slot.
fn quads(materials: [u32; 6], bottles: [bool; 3]) -> Vec<ModelQuad> {
    let mut quads =
        super::geometry::vanilla_cuboid_quads(materials, [112, 0, 112], [144, 224, 144]).to_vec();
    let base = materials[BlockFace::Down as usize];
    for (min, max) in [
        ([144, 0, 80], [240, 32, 176]),
        ([16, 0, 16], [112, 32, 112]),
        ([16, 0, 144], [112, 32, 240]),
    ] {
        quads.extend(super::geometry::vanilla_cuboid_quads([base; 6], min, max));
    }
    // Positions are quantized once into the carrier's existing 1/256-block grid.
    for ([x, z], occupied) in [[256, 128], [37, 37], [37, 219]].into_iter().zip(bottles) {
        let outer_u = if occupied { 0 } else { 4096 };
        quads.push(ModelQuad {
            positions: [[128, 256, 128], [128, 0, 128], [x, 0, z], [x, 256, z]],
            uvs: [[2048, 0], [2048, 4096], [outer_u, 4096], [outer_u, 0]],
            material: materials[BlockFace::Up as usize],
            flags: MODEL_QUAD_FLAG_TWO_SIDED,
        });
    }
    quads
}
