//! Vanilla Nether portal cuboids and animated material admission.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

// The native half-width is 1/8 about the block center: six through ten pixels.
const NEAR: i16 = 96;
const FAR: i16 = 160;

pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == assets::NETHER_PORTAL_IDENTIFIER
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 6], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let axis = canonical_state_str(&record.canonical_state, "portal_axis");
    let Some(axis @ ("x" | "z" | "unknown")) = axis.as_deref() else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    if let Some(materials) = inputs.materials(record) {
        // Z is the unknown-axis default. X follows it for the neighbor override.
        let base = if let Some(&base) = templates.get(&materials) {
            base
        } else {
            let base = push_model_template(
                portal_quads(materials, false).to_vec(),
                assets::MODEL_TEMPLATE_FLAG_NETHER_PORTAL,
                storage.templates,
                storage.quads,
            )?;
            push_model_template(
                portal_quads(materials, true).to_vec(),
                assets::MODEL_TEMPLATE_FLAG_NETHER_PORTAL,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(materials, base);
            base
        };
        set_model_visual(&mut visual, materials, base + u32::from(axis == "x"));
        if axis == "unknown" {
            visual.variant = assets::BLOCK_VISUAL_VARIANT_PORTAL_UNKNOWN;
        }
    }
    Ok(CompileRuleResult::Compiled(visual))
}

fn portal_quads(materials: [u32; 6], axis_x: bool) -> [ModelQuad; 6] {
    let (min, max) = if axis_x {
        ([0, 0, NEAR], [256, 256, FAR])
    } else {
        ([NEAR, 0, 0], [FAR, 256, 256])
    };
    let mut quads = super::geometry::vanilla_cuboid_quads(materials, min, max);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        // The broad portal faces are inset; only touching boundaries can cull.
        if !matches!(
            (axis_x, face),
            (true, BlockFace::North | BlockFace::South)
                | (false, BlockFace::West | BlockFace::East)
        ) {
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
    }
    quads
}

#[cfg(test)]
#[path = "portal_tests.rs"]
mod tests;
