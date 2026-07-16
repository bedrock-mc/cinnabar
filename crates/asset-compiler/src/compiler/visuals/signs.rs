use super::super::*;
use super::context::SignState;
use super::{
    button::{model_quad_face_from_id, model_quad_face_id},
    carpets::typed_model_state_value,
    gates::rotate_gate_face,
};

pub(in crate::compiler) fn sign_state(record: &RegistryRecord) -> Option<SignState> {
    const ORIENTATION_MASK: u8 = 1 << (ModelStateField::Orientation as u8 - 1);
    const FLAGS_MASK: u8 = 1 << (ModelStateField::Flags as u8 - 1);
    const ATTACHED: u32 = 1 << 2;
    const HANGING: u32 = 1 << 3;
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    let orientation = record.model_state.get(ModelStateField::Orientation)?;

    if record.name.ends_with("standing_sign") {
        if record.model_state.mask() != ORIENTATION_MASK || state.len() != 1 {
            return None;
        }
        let rotation = typed_model_state_value(&state, "ground_sign_direction", "int")?.as_u64()?;
        let rotation = u8::try_from(rotation).ok()?;
        return (rotation <= 15 && orientation == u32::from(rotation))
            .then_some(SignState::Standing { rotation });
    }
    if record.name.ends_with("wall_sign") {
        if record.model_state.mask() != ORIENTATION_MASK || state.len() != 1 {
            return None;
        }
        let facing = typed_model_state_value(&state, "facing_direction", "int")?.as_u64()?;
        let facing = u8::try_from(facing).ok()?;
        return (facing <= 5 && orientation == u32::from(facing))
            .then_some(SignState::Wall { facing });
    }
    if !record.name.ends_with("hanging_sign")
        || record.model_state.mask() != ORIENTATION_MASK | FLAGS_MASK
        || state.len() != 4
    {
        return None;
    }
    let facing = typed_model_state_value(&state, "facing_direction", "int")?.as_u64()?;
    let rotation = typed_model_state_value(&state, "ground_sign_direction", "int")?.as_u64()?;
    let attached = typed_model_state_value(&state, "attached_bit", "byte")?.as_u64()?;
    let hanging = typed_model_state_value(&state, "hanging", "byte")?.as_u64()?;
    let facing = u8::try_from(facing).ok()?;
    let rotation = u8::try_from(rotation).ok()?;
    if facing > 5 || rotation > 15 || attached > 1 || hanging > 1 {
        return None;
    }
    let flags = record.model_state.get(ModelStateField::Flags)?;
    let expected_flags = u32::from(attached != 0) * ATTACHED + u32::from(hanging != 0) * HANGING;
    if orientation != u32::from(rotation) | (u32::from(facing) << 4) || flags != expected_flags {
        return None;
    }
    if hanging == 0 {
        Some(SignState::HangingWall { facing })
    } else {
        Some(SignState::HangingCeiling {
            rotation,
            attached: attached != 0,
        })
    }
}

pub(in crate::compiler) fn sign_quads(material: u32, state: SignState) -> Vec<ModelQuad> {
    let materials = [material; 6];
    match state {
        SignState::Standing { rotation } => {
            // Classic vanilla's SignModel declares a 24x12 board and its
            // block-entity render pose applies a 2/3 scale before converting
            // model pixels to world units. The resulting 16x8-pixel (1x1/2
            // block) world silhouette is therefore [0,256] x [112,240] here;
            // using the raw 24x12 model box would make signs 50% oversized.
            let mut quads = Vec::with_capacity(12);
            quads.extend(cuboid_quads(materials, [0, 112, 120], [256, 240, 136]));
            quads.extend(cuboid_quads(materials, [120, 0, 120], [136, 112, 136]));
            rotate_sign_quads(quads, rotation)
        }
        SignState::Wall { facing } => wall_sign_quads(materials, facing).into(),
        SignState::HangingWall { facing } => hanging_wall_sign_quads(materials, facing),
        SignState::HangingCeiling { rotation, attached } => {
            let mut quads = Vec::with_capacity(if attached { 18 } else { 30 });
            quads.extend(cuboid_quads(materials, [16, 48, 120], [240, 176, 136]));
            let supports: &[[[i16; 3]; 2]] = if attached {
                &[
                    [[48, 176, 120], [64, 256, 136]],
                    [[192, 176, 120], [208, 256, 136]],
                ]
            } else {
                &[
                    [[32, 176, 120], [48, 256, 136]],
                    [[64, 176, 120], [80, 256, 136]],
                    [[176, 176, 120], [192, 256, 136]],
                    [[208, 176, 120], [224, 256, 136]],
                ]
            };
            for [min, max] in supports {
                quads.extend(cuboid_quads(materials, *min, *max));
            }
            rotate_sign_quads(quads, rotation)
        }
    }
}

pub(in crate::compiler) fn wall_sign_quads(materials: [u32; 6], facing: u8) -> [ModelQuad; 6] {
    let (min, max) = match facing {
        0 => ([0, 240, 0], [256, 256, 256]),
        1 => ([0, 0, 0], [256, 16, 256]),
        2 => ([0, 72, 240], [256, 200, 256]),
        3 => ([0, 72, 0], [256, 200, 16]),
        4 => ([240, 72, 0], [256, 200, 256]),
        5 => ([0, 72, 0], [16, 200, 256]),
        _ => unreachable!("sign selector validates all wall facings"),
    };
    cuboid_quads(materials, min, max)
}

pub(in crate::compiler) fn hanging_wall_sign_quads(
    materials: [u32; 6],
    facing: u8,
) -> Vec<ModelQuad> {
    let mut quads = Vec::with_capacity(12);
    let (board_min, board_max, support_min, support_max) = match facing {
        0 => (
            [16, 120, 48],
            [240, 136, 176],
            [96, 128, 176],
            [160, 256, 256],
        ),
        1 => ([16, 120, 80], [240, 136, 208], [96, 0, 0], [160, 128, 80]),
        2 => (
            [16, 48, 120],
            [240, 176, 136],
            [96, 224, 128],
            [160, 256, 256],
        ),
        3 => (
            [16, 48, 120],
            [240, 176, 136],
            [96, 224, 0],
            [160, 256, 128],
        ),
        4 => (
            [120, 48, 16],
            [136, 176, 240],
            [128, 224, 96],
            [256, 256, 160],
        ),
        5 => (
            [120, 48, 16],
            [136, 176, 240],
            [0, 224, 96],
            [128, 256, 160],
        ),
        _ => unreachable!("sign selector validates all hanging-wall facings"),
    };
    quads.extend(cuboid_quads(materials, board_min, board_max));
    quads.extend(cuboid_quads(materials, support_min, support_max));
    quads
}

pub(in crate::compiler) fn rotate_sign_quads(
    mut quads: Vec<ModelQuad>,
    rotation: u8,
) -> Vec<ModelQuad> {
    // 16-way Bedrock sign rotation, in exact 1/256-block output coordinates.
    // The table is Q10 sin/cos so generation is byte-deterministic on every host.
    const TRIG: [(i32, i32); 16] = [
        (1024, 0),
        (946, 392),
        (724, 724),
        (392, 946),
        (0, 1024),
        (-392, 946),
        (-724, 724),
        (-946, 392),
        (-1024, 0),
        (-946, -392),
        (-724, -724),
        (-392, -946),
        (0, -1024),
        (392, -946),
        (724, -724),
        (946, -392),
    ];
    let (cos, sin) = TRIG[rotation as usize];
    let rounded = |value: i32| {
        if value < 0 {
            (value - 512) / 1024
        } else {
            (value + 512) / 1024
        }
    };
    for quad in &mut quads {
        for [x, _, z] in &mut quad.positions {
            let dx = i32::from(*x) - 128;
            let dz = i32::from(*z) - 128;
            *x = i16::try_from(128 + rounded(dx * cos - dz * sin)).expect("bounded rotated sign X");
            *z = i16::try_from(128 + rounded(dx * sin + dz * cos)).expect("bounded rotated sign Z");
        }
        let face = quad.flags & MODEL_QUAD_FLAG_FACE_MASK;
        quad.flags &= !MODEL_QUAD_FLAG_FACE_MASK;
        if matches!(face, 1 | 2) {
            quad.flags |= face;
        } else if rotation.is_multiple_of(4) {
            let source = model_quad_face_from_id(face)
                .expect("generated sign cuboids always carry a model face id");
            quad.flags |= model_quad_face_id(rotate_gate_face(source, u32::from(rotation / 4)));
        }
        // The packed model-lighting sidecar only represents the six axis
        // normals. At intermediate 22.5-degree rotations, clearing the side
        // face id deliberately selects `default_lighting`; top and bottom
        // retain their exact axis ids and AO sampling. This is the bounded
        // pre-arbitrary-normal approximation, not an accidental stale normal.
    }
    quads
}
