//! Stonecutter body and saw blade, both turned with the cardinal direction.
//!
//! The body is nine pixels tall and wears the pack's north art on every side. The saw is one
//! two-sided plane through the block centre that samples the west art's bottom seven rows over
//! the block's upper half. East and west facings run the saw along Z and turn the top art a
//! quarter turn; north and south run it along X.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// Body top in model units (nine of sixteen pixels).
const BODY_TOP: i16 = 9 * 16;
/// Saw plane bottom in model units; the plane reaches the block top.
const SAW_BOTTOM: i16 = 8 * 16;
/// Block centre in model units, where the saw plane sits.
const CENTRE: i16 = 8 * 16;
/// Texture V of the saw art's first sampled row (row nine of sixteen).
const SAW_TOP_V: u16 = 9 * 256;

/// Identifies the stonecutter before the provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    block_render_fact(&record.name).kind == R::Stonecutter
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Whether the typed cardinal direction runs the saw along Z (east or west).
fn along_z(record: &RegistryRecord) -> Option<bool> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 1 {
        return None;
    }
    match super::state::exact_tagged_string(
        state.get(assets::facts::BlockProperty::MinecraftCardinalDirection.name())?,
    )? {
        "north" | "south" => Some(false),
        "east" | "west" => Some(true),
        _ => None,
    }
}

/// Compiles both saw orientations with pack-resolved body and saw materials.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], bool), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some(along_z) = along_z(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, along_z);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(materials, along_z),
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

/// Emits the body cuboid followed by the two-sided saw plane.
fn quads(materials: [u32; 6], along_z: bool) -> Vec<ModelQuad> {
    let side = materials[BlockFace::North as usize];
    let mut faces = [side; 6];
    faces[BlockFace::Down as usize] = materials[BlockFace::Down as usize];
    faces[BlockFace::Up as usize] = materials[BlockFace::Up as usize];
    let mut quads =
        super::geometry::vanilla_cuboid_quads(faces, [0; 3], [256, BODY_TOP, 256]).to_vec();
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        if face == BlockFace::Up {
            if along_z {
                for uv in &mut quad.uvs {
                    *uv = [uv[1], 4096 - uv[0]];
                }
            }
        } else {
            // Every face but the inset top lies on the block boundary.
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
    }
    let saw = |across: i16, up: i16| {
        if along_z {
            [CENTRE, up, across]
        } else {
            [across, up, CENTRE]
        }
    };
    quads.push(ModelQuad {
        positions: [
            saw(0, 256),
            saw(0, SAW_BOTTOM),
            saw(256, SAW_BOTTOM),
            saw(256, 256),
        ],
        uvs: [[0, SAW_TOP_V], [0, 4096], [4096, 4096], [4096, SAW_TOP_V]],
        material: materials[BlockFace::West as usize],
        flags: MODEL_QUAD_FLAG_TWO_SIDED,
    });
    quads
}
