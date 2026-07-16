use super::super::*;
use super::carpets::typed_model_state_value;

pub(in crate::compiler) fn button_state(record: &RegistryRecord) -> Option<(u8, bool)> {
    const BUTTON_STATE_MASK: u8 = 0x81;
    const PRESSED: u32 = 1 << 1;
    if record.model_state.mask() != BUTTON_STATE_MASK {
        return None;
    }
    let orientation = record.model_state.get(ModelStateField::Orientation)?;
    let flags = record.model_state.get(ModelStateField::Flags)?;
    if orientation > 5 || !matches!(flags, 0 | PRESSED) {
        return None;
    }
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if state.len() != 2 {
        return None;
    }
    let pressed = match typed_model_state_value(&state, "button_pressed_bit", "byte")?.as_u64()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let facing = typed_model_state_value(&state, "facing_direction", "int")?.as_u64()?;
    if facing > 5 || facing != u64::from(orientation) || pressed != (flags == PRESSED) {
        return None;
    }
    Some((orientation as u8, pressed))
}

pub(in crate::compiler) const fn model_quad_face_from_id(face: u32) -> Option<BlockFace> {
    match face {
        1 => Some(BlockFace::Down),
        2 => Some(BlockFace::Up),
        3 => Some(BlockFace::West),
        4 => Some(BlockFace::East),
        5 => Some(BlockFace::North),
        6 => Some(BlockFace::South),
        _ => None,
    }
}

pub(in crate::compiler) const fn model_quad_face_id(face: BlockFace) -> u32 {
    match face {
        BlockFace::Down => 1,
        BlockFace::Up => 2,
        BlockFace::West => 3,
        BlockFace::East => 4,
        BlockFace::North => 5,
        BlockFace::South => 6,
    }
}

pub(in crate::compiler) fn button_bounds(orientation: u8, pressed: bool) -> ([i16; 3], [i16; 3]) {
    // Java's pressed model is 1.02 pixels high. Packed model coordinates are
    // 1/16 pixel, so use the nearest deterministic one-pixel representation;
    // this also matches the pressed side UV strip exactly.
    let height = if pressed { 16 } else { 32 };
    match orientation {
        0 => ([80, 256 - height, 96], [176, 256, 160]),
        1 => ([80, 0, 96], [176, height, 160]),
        2 => ([80, 96, 256 - height], [176, 160, 256]),
        3 => ([80, 96, 0], [176, 160, height]),
        4 => ([256 - height, 96, 80], [256, 160, 176]),
        5 => ([0, 96, 80], [height, 160, 176]),
        _ => unreachable!("button selectors are validated before geometry generation"),
    }
}

pub(in crate::compiler) fn button_rotated_face(face: BlockFace, orientation: u8) -> BlockFace {
    const X_90: [BlockFace; 6] = [
        BlockFace::West,
        BlockFace::East,
        BlockFace::South,
        BlockFace::North,
        BlockFace::Down,
        BlockFace::Up,
    ];
    const Y_90: [BlockFace; 6] = [
        BlockFace::North,
        BlockFace::South,
        BlockFace::Down,
        BlockFace::Up,
        BlockFace::East,
        BlockFace::West,
    ];
    let yaw = |mut face: BlockFace, turns: u8| {
        for _ in 0..turns {
            face = Y_90[face as usize];
        }
        face
    };
    match orientation {
        0 => match face {
            BlockFace::Down => BlockFace::Up,
            BlockFace::Up => BlockFace::Down,
            BlockFace::North => BlockFace::South,
            BlockFace::South => BlockFace::North,
            horizontal => horizontal,
        },
        1 => face,
        2 => X_90[face as usize],
        3 => yaw(X_90[face as usize], 2),
        4 => yaw(X_90[face as usize], 3),
        5 => yaw(X_90[face as usize], 1),
        _ => unreachable!("button selectors are validated before geometry generation"),
    }
}

pub(in crate::compiler) fn button_rotate_position(
    [x, y, z]: [i16; 3],
    orientation: u8,
) -> [i16; 3] {
    match orientation {
        0 => [x, 256 - y, 256 - z],
        1 => [x, y, z],
        2 => [x, z, 256 - y],
        3 => [256 - x, z, y],
        4 => [256 - y, z, 256 - x],
        5 => [y, z, x],
        _ => unreachable!("button selectors are validated before geometry generation"),
    }
}

pub(in crate::compiler) fn button_face_uv(face: BlockFace, pressed: bool) -> [u16; 4] {
    match face {
        BlockFace::Down | BlockFace::Up => [5, 6, 11, 10],
        BlockFace::North | BlockFace::South => [5, 14, 11, if pressed { 15 } else { 16 }],
        BlockFace::West | BlockFace::East => [6, 14, 10, if pressed { 15 } else { 16 }],
    }
}

pub(in crate::compiler) fn button_quad(
    material: u32,
    min: [i16; 3],
    max: [i16; 3],
    face: BlockFace,
    rect: [u16; 4],
) -> ModelQuad {
    let mut quad = cuboid_quads([material; 6], min, max)[face as usize];
    let [u1, v1, u2, v2] = rect.map(|coordinate| coordinate * 256);
    quad.uvs = match face {
        BlockFace::West | BlockFace::South => [[u1, v2], [u2, v2], [u2, v1], [u1, v1]],
        BlockFace::East | BlockFace::North => [[u1, v2], [u1, v1], [u2, v1], [u2, v2]],
        BlockFace::Down => [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        BlockFace::Up => [[u1, v1], [u1, v2], [u2, v2], [u2, v1]],
    };
    quad.flags = face as u32;
    quad
}

pub(in crate::compiler) fn button_uvlock_rect(
    face: BlockFace,
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
) -> [u16; 4] {
    let [min_x, min_y, min_z, max_x, max_y, max_z] = [min_x, min_y, min_z, max_x, max_y, max_z]
        .map(|coordinate| u16::try_from(coordinate / 16).expect("button bounds are nonnegative"));
    match face {
        BlockFace::West | BlockFace::East => [min_z, 16 - max_y, max_z, 16 - min_y],
        BlockFace::North | BlockFace::South => [min_x, 16 - max_y, max_x, 16 - min_y],
        BlockFace::Down | BlockFace::Up => [min_x, min_z, max_x, max_z],
    }
}

pub(in crate::compiler) fn button_quads(
    materials: [u32; 6],
    orientation: u8,
    pressed: bool,
) -> [ModelQuad; 6] {
    let height = if pressed { 16 } else { 32 };
    let source_min = [80, 0, 96];
    let source_max = [176, height, 160];
    let (target_min, target_max) = button_bounds(orientation, pressed);
    BlockFace::ALL.map(|source_face| {
        let target_face = button_rotated_face(source_face, orientation);
        if orientation <= 1 {
            let mut quad = button_quad(
                materials[target_face as usize],
                source_min,
                source_max,
                source_face,
                button_face_uv(source_face, pressed),
            );
            quad.positions = quad
                .positions
                .map(|position| button_rotate_position(position, orientation));
            quad.flags = target_face as u32;
            quad
        } else {
            // Java wall variants are UV-locked: the rotated element is
            // projected in target space, rather than carrying the source
            // face's rectangle through the rotation.
            button_quad(
                materials[target_face as usize],
                target_min,
                target_max,
                target_face,
                button_uvlock_rect(target_face, target_min, target_max),
            )
        }
    })
}
