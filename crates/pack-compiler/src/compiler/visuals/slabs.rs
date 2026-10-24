use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 7], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_slab(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(half @ 0..=2) = record.model_state.get(ModelStateField::Half)
    {
        let [west, east, down, up, north, south] = materials;
        let key = [west, east, down, up, north, south, half];
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                slab_quads(materials, half).to_vec(),
                0,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
        visual.flags.set(BlockFlags::OCCLUDES_FULL_FACE, half == 2);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// Builds outside-view face UVs and culls only faces touching the block boundary.
pub(in crate::compiler) fn slab_quads(materials: [u32; 6], half: u32) -> [ModelQuad; 6] {
    let (min_y, max_y) = match half {
        0 => (0, 128),
        1 => (128, 256),
        2 => (0, 256),
        _ => unreachable!("slab half is checked before template generation"),
    };
    let mut quads =
        super::geometry::vanilla_cuboid_quads(materials, [0, min_y, 0], [256, max_y, 256]);
    for (face, quad) in BlockFace::ALL.into_iter().zip(&mut quads) {
        let boundary = match face {
            BlockFace::Down => min_y == 0,
            BlockFace::Up => max_y == 256,
            _ => true,
        };
        if boundary {
            quad.flags |= (quad.flags & MODEL_QUAD_FLAG_FACE_MASK) << 4;
        }
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slab_faces_keep_outside_view_uv_axes_for_every_half() {
        for half in 0..=2 {
            for (face, quad) in BlockFace::ALL.into_iter().zip(slab_quads([1; 6], half)) {
                for ([x, y, z], uv) in quad.positions.into_iter().zip(quad.uvs) {
                    let [x, y, z] = [x, y, z].map(|value| value as u16 * 16);
                    let expected = match face {
                        BlockFace::West => [z, 4096 - y],
                        BlockFace::East => [4096 - z, 4096 - y],
                        BlockFace::Down => [x, 4096 - z],
                        BlockFace::Up => [x, z],
                        BlockFace::North => [4096 - x, 4096 - y],
                        BlockFace::South => [x, 4096 - y],
                    };
                    assert_eq!(uv, expected, "half {half}, face {face:?}");
                }
            }
        }
    }
}
