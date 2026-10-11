use super::context::{
    ModelStorage, RuleInputs, ThinTemplateKey, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

const FAMILY: u8 = 2;
/// Rail plane height above the block floor in 1/256 block units; needs native measurement.
const RAIL_LIFT: i16 = 16;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<ThinTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_rail(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some((direction, alternate)) = rail_state(record)
    {
        // The pack keys the alternate sprite (curve or powered) on the up face.
        let face = if alternate {
            BlockFace::Up
        } else {
            BlockFace::Down
        };
        let material = materials[face as usize];
        let key = ThinTemplateKey {
            family: FAMILY,
            shape: direction,
            materials: [material, 0],
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                vec![rail_quad(material, direction)],
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

/// Returns the rail direction (0..=9) and whether the alternate sprite applies.
pub(in crate::compiler) fn rail_state(record: &RegistryRecord) -> Option<(u8, bool)> {
    let direction = canonical_state_u32(&record.canonical_state, "rail_direction")?;
    if record.name.as_ref() == "minecraft:rail" {
        return (direction <= 9).then_some((direction as u8, direction >= 6));
    }
    let powered = canonical_state_u32(&record.canonical_state, "rail_data_bit")?;
    (direction <= 5 && powered <= 1).then_some((direction as u8, powered == 1))
}

/// Turns of clockwise-from-above rotation applied to the north-south straight sprite,
/// or to the south-east curve sprite for directions 6..=9.
const fn sprite_turns(direction: u8) -> u8 {
    match direction {
        1..=3 => 1,
        8 => 2,
        9 => 3,
        7 => 1,
        _ => 0,
    }
}

/// Height in block units of 1/256 above `RAIL_LIFT` at the given corner.
fn corner_rise(direction: u8, x: i16, z: i16) -> i16 {
    let high = match direction {
        2 => x == 256,
        3 => x == 0,
        4 => z == 0,
        5 => z == 256,
        _ => return 0,
    };
    if high { 256 } else { 0 }
}

/// One up-facing plane: flat, ascending, or curved, with the sprite turned to follow the track.
pub(in crate::compiler) fn rail_quad(material: u32, direction: u8) -> ModelQuad {
    let corners = [(0, 0), (0, 256), (256, 256), (256, 0)];
    let turns = sprite_turns(direction);
    let mut positions = [[0_i16; 3]; 4];
    let mut uvs = [[0_u16; 2]; 4];
    for (index, (x, z)) in corners.into_iter().enumerate() {
        positions[index] = [x, RAIL_LIFT + corner_rise(direction, x, z), z];
        // Inverse-rotate the world corner about the tile centre to find its sprite coordinate.
        let (mut u, mut v) = (i32::from(x) - 128, i32::from(z) - 128);
        for _ in 0..turns {
            (u, v) = (v, -u);
        }
        uvs[index] = [((u + 128) * 16) as u16, ((v + 128) * 16) as u16];
    }
    ModelQuad {
        positions,
        uvs,
        material,
        flags: 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slopes_rise_one_block_toward_their_named_side() {
        for (direction, axis, high_at) in [(2, 0, 256), (3, 0, 0), (4, 2, 0), (5, 2, 256)] {
            let quad = rail_quad(5, direction);
            for position in quad.positions {
                let expected = if position[axis] == high_at { 272 } else { 16 };
                assert_eq!(position[1], expected, "direction {direction}");
            }
        }
    }

    #[test]
    fn flat_and_curved_rails_share_the_lifted_plane() {
        for direction in [0, 1, 6, 7, 8, 9] {
            assert!(rail_quad(5, direction).positions.iter().all(|p| p[1] == 16));
        }
    }

    #[test]
    fn east_west_sprite_is_the_north_south_sprite_turned_a_quarter() {
        let north_south = rail_quad(5, 0).uvs;
        let east_west = rail_quad(5, 1).uvs;
        assert_eq!(north_south[0], [0, 0]);
        assert_ne!(north_south, east_west);
        assert_eq!(east_west[0], [0, 4096]);
    }
}
