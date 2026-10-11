use super::context::{
    ModelStorage, RuleInputs, ThinTemplateKey, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

const FAMILY: u8 = 1;
/// Plane inset from the supporting wall in 1/256 block units; needs native measurement.
const WALL_INSET: i16 = 13;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<ThinTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_ladder(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(facing) = canonical_state_u32(&record.canonical_state, "facing_direction")
    {
        let material = materials[BlockFace::Up as usize];
        // Values 0 and 1 are not placeable; render them like a north-facing ladder.
        let facing = if (2..=5).contains(&facing) {
            facing as u8
        } else {
            2
        };
        let key = ThinTemplateKey {
            family: FAMILY,
            shape: facing,
            materials: [material, 0],
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                vec![ladder_quad(material, facing)],
                0,
                storage.templates,
                storage.quads,
            )?;
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

/// One two-sided plane on the wall opposite `facing` (2 north, 3 south, 4 west, 5 east).
pub(in crate::compiler) fn ladder_quad(material: u32, facing: u8) -> ModelQuad {
    let far = 256 - WALL_INSET;
    let (positions, face_id): ([[i16; 3]; 4], u32) = match facing {
        2 => (
            [[0, 0, far], [0, 256, far], [256, 256, far], [256, 0, far]],
            5,
        ),
        3 => (
            [
                [0, 0, WALL_INSET],
                [256, 0, WALL_INSET],
                [256, 256, WALL_INSET],
                [0, 256, WALL_INSET],
            ],
            6,
        ),
        4 => (
            [[far, 0, 0], [far, 0, 256], [far, 256, 256], [far, 256, 0]],
            3,
        ),
        _ => (
            [
                [WALL_INSET, 0, 0],
                [WALL_INSET, 256, 0],
                [WALL_INSET, 256, 256],
                [WALL_INSET, 0, 256],
            ],
            4,
        ),
    };
    let along_z = matches!(face_id, 3 | 4);
    ModelQuad {
        positions,
        uvs: positions.map(|[x, y, z]| {
            let across = if along_z { z } else { x };
            [(across as u16) * 16, (4096 - i32::from(y) * 16) as u16]
        }),
        material,
        flags: MODEL_QUAD_FLAG_TWO_SIDED | face_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ladder_plane_hugs_the_wall_opposite_its_facing() {
        for (facing, axis, near_zero) in [(2, 2, false), (3, 2, true), (4, 0, false), (5, 0, true)]
        {
            let quad = ladder_quad(9, facing);
            assert!(
                quad.positions
                    .iter()
                    .all(|p| p[axis] == quad.positions[0][axis])
            );
            assert_eq!(quad.positions[0][axis] < 128, near_zero, "facing {facing}");
            assert_ne!(quad.flags & MODEL_QUAD_FLAG_TWO_SIDED, 0);
        }
    }

    #[test]
    fn ladder_uvs_cover_the_whole_tile() {
        let uvs = ladder_quad(9, 2).uvs;
        assert!(uvs.contains(&[0, 4096]) && uvs.contains(&[4096, 0]));
    }
}
