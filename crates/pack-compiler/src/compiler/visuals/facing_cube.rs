//! Face-directional cubes (the loom) flip their top and bottom artwork with their direction.
//!
//! Material selection already turns each pack face toward the direction
//! (`pack_compile_input::pack::facing_cube_face`); this template carries the matching UV flips.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::{BlockRenderKind as R, block_render_fact};
use {super::super::*, assets::BlockFace};

/// Compiles the loom family's flipped face UVs; malformed directions stay diagnostic.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], [u8; 6]), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if block_render_fact(&record.name).kind != R::FacingCube {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut flips = [0; 6];
    for face in BlockFace::ALL {
        let Some((_, flip)) = pack_compile_input::pack::facing_cube_face(record, face) else {
            return Ok(CompileRuleResult::Reject);
        };
        flips[face as usize] = flip;
    }
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, flips);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let quads = super::geometry::flipped_cube_quads(materials, flips);
        let template = push_model_template(quads.to_vec(), 0, storage.templates, storage.quads)?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    visual.flags.insert(BlockFlags::OCCLUDES_FULL_FACE);
    visual.support = VisualSupport::Exact;
    Ok(CompileRuleResult::Compiled(visual))
}
