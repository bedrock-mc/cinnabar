use super::context::{
    ModelStorage, RuleInputs, ThinTemplateKey, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::cross::crossed_quads;
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

const FAMILY: u8 = 4;
const UP: u8 = 0;
const CENTRE: i16 = 128;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<ThinTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_crystal(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(facing) = crystal_facing(record)
    {
        let material = materials[BlockFace::Up as usize];
        let key = ThinTemplateKey {
            family: FAMILY,
            shape: facing,
            materials: [material, 0],
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                crystal_quads(material, facing).to_vec(),
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

/// 0..=5 for up, down, north, south, east, west: the direction the tip points.
fn crystal_facing(record: &RegistryRecord) -> Option<u8> {
    if record.name.ends_with("_coral_fan") {
        return Some(UP);
    }
    Some(
        match canonical_state_str(&record.canonical_state, "minecraft:block_face")?.as_ref() {
            "up" => UP,
            "down" => 1,
            "north" => 2,
            "south" => 3,
            "east" => 4,
            "west" => 5,
            _ => return None,
        },
    )
}

/// The crossed sprite planes standing on the floor, turned about the block centre so the tip points along `facing`.
pub(in crate::compiler) fn crystal_quads(material: u32, facing: u8) -> [ModelQuad; 2] {
    let turn = |[x, y, z]: [i16; 3]| {
        let (x, y, z) = (x - CENTRE, y - CENTRE, z - CENTRE);
        let (x, y, z) = match facing {
            UP => (x, y, z),
            1 => (x, -y, -z),
            2 => (x, z, -y),
            3 => (x, -z, y),
            4 => (y, -x, z),
            _ => (-y, x, z),
        };
        [x + CENTRE, y + CENTRE, z + CENTRE]
    };
    crossed_quads([material; 2]).map(|quad| ModelQuad {
        positions: quad.positions.map(turn),
        ..quad
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip_vertices(facing: u8) -> Vec<[i16; 3]> {
        // The standing sprite's tip edge is its four vertices at y = 256.
        let standing = crystal_quads(7, UP);
        let turned = crystal_quads(7, facing);
        standing
            .iter()
            .zip(&turned)
            .flat_map(|(a, b)| {
                a.positions
                    .into_iter()
                    .zip(b.positions)
                    .filter(|(from, _)| from[1] == 256)
                    .map(|(_, to)| to)
            })
            .collect()
    }

    #[test]
    fn tip_edge_lands_on_the_face_the_crystal_points_to() {
        for (facing, axis, value) in [
            (UP, 1, 256),
            (1, 1, 0),
            (2, 2, 0),
            (3, 2, 256),
            (4, 0, 256),
            (5, 0, 0),
        ] {
            let tips = tip_vertices(facing);
            assert_eq!(tips.len(), 4, "facing {facing}");
            assert!(tips.iter().all(|tip| tip[axis] == value), "facing {facing}");
        }
    }

    #[test]
    fn turned_planes_stay_inside_the_block_and_keep_two_sided_flags() {
        for facing in 0..6 {
            for quad in crystal_quads(7, facing) {
                assert_eq!(quad.flags, MODEL_QUAD_FLAG_TWO_SIDED);
                assert!(
                    quad.positions
                        .iter()
                        .flatten()
                        .all(|c| (0..=256).contains(c))
                );
            }
        }
    }
}
