//! Recessed shelf geometry and the five front regions of its texture sheet.

use super::super::*;
use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;

/// Identifies vanilla shelves without admitting unrelated names ending in shelf.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
        && matches!(
            record.name.as_ref(),
            "minecraft:acacia_shelf"
                | "minecraft:bamboo_shelf"
                | "minecraft:birch_shelf"
                | "minecraft:cherry_shelf"
                | "minecraft:crimson_shelf"
                | "minecraft:dark_oak_shelf"
                | "minecraft:jungle_shelf"
                | "minecraft:mangrove_shelf"
                | "minecraft:oak_shelf"
                | "minecraft:pale_oak_shelf"
                | "minecraft:poplar_shelf"
                | "minecraft:spruce_shelf"
                | "minecraft:warped_shelf"
        )
}

/// Checks typed selectors against their registry projection and selects the front region.
fn state(record: &RegistryRecord) -> Option<(u8, u8)> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct State {
        #[serde(rename = "minecraft:cardinal_direction")]
        direction: Tagged<String>,
        powered_bit: Tagged<u8>,
        powered_shelf_type: Tagged<u8>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Tagged<T> {
        #[serde(rename = "type")]
        kind: String,
        value: T,
    }
    let state: State = serde_json::from_str(&record.canonical_state).ok()?;
    let direction = match state.direction.value.as_str() {
        "south" => 0,
        "west" => 1,
        "north" => 2,
        "east" => 3,
        _ => return None,
    };
    let powered = state.powered_bit.value;
    let kind = state.powered_shelf_type.value;
    let fields = [
        ModelStateField::Orientation,
        ModelStateField::Growth,
        ModelStateField::Flags,
    ];
    let mask = fields
        .into_iter()
        .fold(0, |mask, field| mask | (1 << (field as u8 - 1)));
    if state.direction.kind != "string"
        || state.powered_bit.kind != "byte"
        || state.powered_shelf_type.kind != "int"
        || powered > 1
        || kind > 3
        || record.model_family != ModelFamily::Cuboid
        || record.face_coverage != 0
        || record.model_state.mask() != mask
        || record.model_state.get(ModelStateField::Orientation) != Some(u32::from(direction))
        || record.model_state.get(ModelStateField::Growth) != Some(u32::from(kind))
        || record.model_state.get(ModelStateField::Flags) != Some(u32::from(powered))
    {
        return None;
    }
    Some((direction, if powered == 0 { 0 } else { kind + 1 }))
}

/// Compiles shelf poses with authored face routing, without a full-cube fallback.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], u8, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    let Some((direction, front)) = state(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, direction, front);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            shelf_quads(materials, direction, front),
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

/// Fits one face's UVs to a rectangle in the 32-pixel shelf sheet, retaining orientation.
fn face_uv(quad: &mut ModelQuad, region: [u16; 4]) {
    for axis in 0..2 {
        let source_min = quad.uvs.iter().map(|uv| uv[axis]).min().unwrap();
        let source_max = quad.uvs.iter().map(|uv| uv[axis]).max().unwrap();
        for uv in &mut quad.uvs {
            let fraction = f32::from(uv[axis] - source_min) / f32::from(source_max - source_min);
            uv[axis] = (f32::from(region[axis])
                + fraction * (f32::from(region[axis + 2]) - f32::from(region[axis])))
            .round() as u16;
        }
    }
}

/// Converts sheet pixel bounds to normalized model UVs with the authored edge inset.
fn region(pixels: [u16; 4]) -> [u16; 4] {
    const INSET: u16 = 10;
    std::array::from_fn(|axis| {
        let edge = pixels[axis] * 128;
        let reversed = pixels[axis % 2] > pixels[axis % 2 + 2];
        if (axis < 2) != reversed {
            edge + INSET
        } else {
            edge - INSET
        }
    })
}

/// Emits the back shell, recessed front, and two projecting ledges, then turns the pose.
fn shelf_quads(materials: [u32; 6], direction: u8, front: u8) -> Vec<ModelQuad> {
    let mut quads = Vec::with_capacity(16);
    let mut add = |min, max, regions: [Option<[u16; 4]>; 6]| {
        for ((face, mut quad), uv) in BlockFace::ALL
            .into_iter()
            .zip(super::geometry::vanilla_cuboid_quads(materials, min, max))
            .zip(regions)
        {
            if let Some(uv) = uv {
                let uv = if matches!(face, BlockFace::Down | BlockFace::Up) {
                    [uv[2], uv[3], uv[0], uv[1]]
                } else {
                    uv
                };
                face_uv(&mut quad, region(uv));
                quads.push(quad);
            }
        }
    };
    add(
        [0, 0, 0],
        [256, 256, 48],
        [
            Some([16, 0, 19, 16]),
            Some([29, 0, 32, 16]),
            Some([32, 12, 16, 9]),
            Some([32, 10, 16, 7]),
            Some([16, 0, 32, 16]),
            None,
        ],
    );
    add(
        [0, 192, 48],
        [256, 256, 80],
        [
            Some([3, 0, 5, 4]),
            Some([11, 0, 13, 4]),
            Some([16, 10, 32, 12]),
            Some([32, 12, 16, 10]),
            None,
            Some([0, 0, 16, 4]),
        ],
    );
    add(
        [0, 0, 48],
        [256, 64, 80],
        [
            Some([3, 12, 5, 16]),
            Some([11, 12, 13, 16]),
            Some([32, 9, 16, 7]),
            Some([16, 7, 32, 9]),
            None,
            Some([0, 12, 16, 16]),
        ],
    );
    let front_regions = [
        [0, 4, 16, 12],
        [16, 24, 32, 32],
        [16, 16, 32, 24],
        [0, 24, 16, 32],
        [0, 16, 16, 24],
    ];
    let mut panel = super::geometry::vanilla_cuboid_quads(materials, [0, 64, 0], [256, 192, 48])
        [BlockFace::South as usize];
    face_uv(&mut panel, region(front_regions[front as usize]));
    quads.push(panel);
    for quad in &mut quads {
        for _ in 0..direction {
            for position in &mut quad.positions {
                let [x, _, z] = *position;
                position[0] = 256 - z;
                position[2] = x;
            }
            let face = match quad.flags & MODEL_QUAD_FLAG_FACE_MASK {
                3 => 5,
                5 => 4,
                4 => 6,
                6 => 3,
                other => other,
            };
            quad.flags = face;
        }
    }
    quads
}
