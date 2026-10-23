//! The heavy core is a fixed, unrotated 8×8×8 block model.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use assets::facts::pack::PackBlock;
use {super::super::*, assets::BlockFace};

const MIN: [i16; 3] = [4 * 16, 0, 4 * 16];
const MAX: [i16; 3] = [12 * 16, 8 * 16, 12 * 16];

/// Admits the primary heavy-core record shared by geometry and fallback routing.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.name.as_ref() == PackBlock::HeavyCore.identifier()
        && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Compiles the native heavy-core cube without applying a block-facing rotation.
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
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state);
    if !state.is_ok_and(|state| state.is_empty()) {
        return Ok(CompileRuleResult::Compiled(visual));
    }
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let template = if let Some(&template) = templates.get(&materials) {
        template
    } else {
        let template = push_model_template(
            heavy_core_quads(materials).to_vec(),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(materials, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    Ok(CompileRuleResult::Compiled(visual))
}

/// Returns the six vanilla heavy-core faces and their 8×8 atlas regions.
pub(in crate::compiler) fn heavy_core_quads(materials: [u32; 6]) -> [ModelQuad; 6] {
    let mut quads = super::geometry::vanilla_cuboid_quads(materials, MIN, MAX);
    let uvs = [
        [[0, 4096], [2048, 4096], [2048, 2048], [0, 2048]],
        [[2048, 4096], [2048, 2048], [0, 2048], [0, 4096]],
        [[2048, 2048], [4096, 2048], [4096, 0], [2048, 0]],
        [[0, 0], [0, 2048], [2048, 2048], [2048, 0]],
        [[2048, 4096], [2048, 2048], [0, 2048], [0, 4096]],
        [[0, 4096], [2048, 4096], [2048, 2048], [0, 2048]],
    ];
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        quad.uvs = uvs[face as usize];
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heavy_core_is_an_unrotated_eight_pixel_cube_with_native_uvs() {
        let quads = heavy_core_quads([0; 6]);
        assert!(quads.iter().all(|quad| quad.material == 0));
        for axis in 0..3 {
            assert_eq!(
                quads
                    .iter()
                    .flat_map(|quad| quad.positions.map(|position| position[axis]))
                    .min(),
                Some(MIN[axis])
            );
            assert_eq!(
                quads
                    .iter()
                    .flat_map(|quad| quad.positions.map(|position| position[axis]))
                    .max(),
                Some(MAX[axis])
            );
        }
        assert_eq!(
            heavy_core_quads([0; 6]).map(|quad| quad.uvs),
            [
                [[0, 4096], [2048, 4096], [2048, 2048], [0, 2048]],
                [[2048, 4096], [2048, 2048], [0, 2048], [0, 4096]],
                [[2048, 2048], [4096, 2048], [4096, 0], [2048, 0]],
                [[0, 0], [0, 2048], [2048, 2048], [2048, 0]],
                [[2048, 4096], [2048, 2048], [0, 2048], [0, 4096]],
                [[0, 4096], [2048, 4096], [2048, 2048], [0, 2048]],
            ]
        );
    }
}
