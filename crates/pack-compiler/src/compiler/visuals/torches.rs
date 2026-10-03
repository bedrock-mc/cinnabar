use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, ThinTemplateKey, diagnostic_visual, push_model_template,
    set_model_visual,
};
use super::dispatcher::CompileRuleResult;

const FAMILY: u8 = 0;
const UPRIGHT: u8 = 0;
const SIN_TILT: f32 = 0.382_683_43;
const COS_TILT: f32 = 0.923_879_5;
/// Wall-mount pivot height above the block floor, in pixels; needs native measurement.
const WALL_PIVOT_Y: f32 = 3.5;

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<ThinTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_torch(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(materials) = inputs.materials(record)
        && let Some(mount) = torch_mount(record)
    {
        let material = materials[BlockFace::Up as usize];
        let key = ThinTemplateKey {
            family: FAMILY,
            shape: mount,
            materials: [material, 0],
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let template = push_model_template(
                torch_quads(material, mount),
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

/// Returns `UPRIGHT` or the `BlockFace` index of the supporting wall.
pub(in crate::compiler) fn torch_mount(record: &RegistryRecord) -> Option<u8> {
    let direction = canonical_state_str(&record.canonical_state, "torch_facing_direction")?;
    Some(match direction.as_ref() {
        "unknown" | "top" => UPRIGHT,
        "west" => 1 + BlockFace::West as u8,
        "east" => 1 + BlockFace::East as u8,
        "north" => 1 + BlockFace::North as u8,
        "south" => 1 + BlockFace::South as u8,
        _ => return None,
    })
}

type LocalFace = (u32, [[i8; 3]; 4]);

/// The 2x10x2 pixel torch pillar around its base centre, corner order matching `cuboid_quads`.
fn pillar_faces() -> [LocalFace; 6] {
    let (lo, hi, top) = (-1_i8, 1_i8, 10_i8);
    [
        (3, [[lo, 0, lo], [lo, 0, hi], [lo, top, hi], [lo, top, lo]]),
        (4, [[hi, 0, lo], [hi, top, lo], [hi, top, hi], [hi, 0, hi]]),
        (1, [[lo, 0, lo], [hi, 0, lo], [hi, 0, hi], [lo, 0, hi]]),
        (
            2,
            [[lo, top, lo], [lo, top, hi], [hi, top, hi], [hi, top, lo]],
        ),
        (5, [[lo, 0, lo], [lo, top, lo], [hi, top, lo], [hi, 0, lo]]),
        (6, [[lo, 0, hi], [hi, 0, hi], [hi, top, hi], [lo, top, hi]]),
    ]
}

/// Texture-pixel coordinate of a pillar corner; the sprite keeps the pillar in its lower-centre strip.
fn pillar_uv(face: u32, [x, y, z]: [i8; 3]) -> [u16; 2] {
    let (x, y, z) = (i32::from(x), i32::from(y), i32::from(z));
    let (u, v) = match face {
        3 | 4 => (z + 8, 16 - y),
        5 | 6 => (x + 8, 16 - y),
        2 => (x + 8, z + 7),
        _ => (x + 8, z + 15),
    };
    [(u * 256) as u16, (v * 256) as u16]
}

/// Pixel-space position of a pillar corner, leaning away from `support` when wall-mounted.
fn place(corner: [i8; 3], support: Option<BlockFace>) -> [i16; 3] {
    let [x, y, z] = corner.map(f32::from);
    let Some(support) = support else {
        return [8.0 + x, y, 8.0 + z].map(pixels_to_units);
    };
    // Canonical frame: support on the west wall, torch leaning toward +x.
    let (lean_x, lean_y) = (x * COS_TILT + y * SIN_TILT, -x * SIN_TILT + y * COS_TILT);
    let (rx, rz) = (lean_x + COS_TILT - 8.0, z);
    let (rx, rz) = match support {
        BlockFace::West => (rx, rz),
        BlockFace::East => (-rx, -rz),
        BlockFace::North => (-rz, rx),
        _ => (rz, -rx),
    };
    [8.0 + rx, WALL_PIVOT_Y + lean_y, 8.0 + rz].map(pixels_to_units)
}

fn pixels_to_units(pixels: f32) -> i16 {
    (pixels * 16.0).round() as i16
}

pub(in crate::compiler) fn torch_quads(material: u32, mount: u8) -> Vec<ModelQuad> {
    let support = match mount {
        UPRIGHT => None,
        mount => Some(BlockFace::ALL[usize::from(mount - 1)]),
    };
    pillar_faces()
        .into_iter()
        .map(|(face, corners)| ModelQuad {
            positions: corners.map(|corner| place(corner, support)),
            uvs: corners.map(|corner| pillar_uv(face, corner)),
            material,
            flags: face,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upright_torch_is_a_two_pixel_pillar_centred_in_the_block() {
        let quads = torch_quads(7, UPRIGHT);
        assert_eq!(quads.len(), 6);
        let xs = quads.iter().flat_map(|q| q.positions.map(|p| p[0]));
        assert_eq!(xs.clone().min(), Some(112));
        assert_eq!(xs.max(), Some(144));
        assert!(
            quads
                .iter()
                .flat_map(|q| q.positions)
                .all(|p| (0..=160).contains(&p[1]))
        );
    }

    #[test]
    fn wall_torch_leans_away_from_its_support_and_stays_inside_the_block() {
        for (support, axis, away_positive) in [
            (BlockFace::West, 0, true),
            (BlockFace::East, 0, false),
            (BlockFace::North, 2, true),
            (BlockFace::South, 2, false),
        ] {
            let mount = 1 + support as u8;
            let quads = torch_quads(7, mount);
            let coordinates = quads
                .iter()
                .flat_map(|q| q.positions)
                .map(|p| i32::from(p[axis]))
                .collect::<Vec<_>>();
            assert!(
                coordinates.iter().all(|c| (0..=256).contains(c)),
                "{support:?}"
            );
            let top = quads[BlockFace::Up as usize].positions[0][axis];
            let base = quads[BlockFace::Down as usize].positions[0][axis];
            assert_eq!(top > base, away_positive, "{support:?}");
        }
    }

    #[test]
    fn pillar_top_uses_the_sprite_top_strip() {
        let top = torch_quads(7, UPRIGHT)[BlockFace::Up as usize].uvs;
        assert!(top.iter().all(|uv| (6 * 256..=8 * 256).contains(&uv[1])));
    }
}
