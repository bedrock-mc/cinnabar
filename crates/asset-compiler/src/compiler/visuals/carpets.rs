use super::super::*;
use super::context::{CarpetState, PaleMossCarpetSide, PaleMossCarpetState};

pub(in crate::compiler) fn carpet_state(record: &RegistryRecord) -> Option<CarpetState> {
    if !is_pale_moss_carpet(record) {
        let state = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
            &record.canonical_state,
        )
        .ok()?;
        return (record.model_state.mask() == 0 && state.is_empty())
            .then_some(CarpetState::Ordinary);
    }

    const FLAGS_MASK: u8 = 1 << (ModelStateField::Flags as u8 - 1);
    const UPPER: u32 = 1 << 7;
    if record.model_state.mask() != FLAGS_MASK {
        return None;
    }
    let flags = record.model_state.get(ModelStateField::Flags)?;
    if !matches!(flags, 0 | UPPER) {
        return None;
    }
    let state =
        serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&record.canonical_state)
            .ok()?;
    if state.len() != 5 {
        return None;
    }
    let side = |direction| {
        let name = format!("pale_moss_carpet_side_{direction}");
        match typed_model_state_value(&state, &name, "string")?.as_str()? {
            "none" => Some(PaleMossCarpetSide::None),
            "short" => Some(PaleMossCarpetSide::Short),
            "tall" => Some(PaleMossCarpetSide::Tall),
            _ => None,
        }
    };
    let upper = match typed_model_state_value(&state, "upper_block_bit", "byte")?.as_u64()? {
        0 => false,
        1 => true,
        _ => return None,
    };
    if upper != (flags == UPPER) {
        return None;
    }
    Some(CarpetState::Pale(PaleMossCarpetState {
        sides: [side("east")?, side("north")?, side("south")?, side("west")?],
        upper,
    }))
}

pub(in crate::compiler) fn typed_model_state_value<'a>(
    state: &'a serde_json::Map<String, serde_json::Value>,
    name: &str,
    expected_type: &str,
) -> Option<&'a serde_json::Value> {
    let typed = state.get(name)?.as_object()?;
    if typed.len() != 2 || typed.get("type")?.as_str()? != expected_type {
        return None;
    }
    typed.get("value")
}

pub(in crate::compiler) fn pale_moss_carpet_quads(
    materials: [u32; 6],
    side_materials: [u32; 2],
    state: PaleMossCarpetState,
) -> Vec<ModelQuad> {
    let isolated_upper = state.upper
        && state
            .sides
            .iter()
            .all(|side| matches!(side, PaleMossCarpetSide::None));
    let mut quads = Vec::with_capacity(14);
    if !state.upper || isolated_upper {
        quads.extend(cuboid_quads(materials, [0, 0, 0], [256, 16, 256]));
    }

    // The vanilla Java plane is inset by 0.1 model pixels, or 1.6 of our
    // 1/256-block units. Two units is the nearest representable symmetric
    // position, paired with 254 on the opposite face.
    type PaleMossPlane = (u32, u32, [[i16; 3]; 4], [[u16; 2]; 4], [[u16; 2]; 4]);
    const PLANES: [PaleMossPlane; 4] = [
        (
            4,
            3,
            [[254, 0, 0], [254, 256, 0], [254, 256, 256], [254, 0, 256]],
            [[4096, 4096], [4096, 0], [0, 0], [0, 4096]],
            [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        ),
        (
            5,
            6,
            [[0, 0, 2], [0, 256, 2], [256, 256, 2], [256, 0, 2]],
            [[4096, 4096], [4096, 0], [0, 0], [0, 4096]],
            [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
        ),
        (
            6,
            5,
            [[0, 0, 254], [256, 0, 254], [256, 256, 254], [0, 256, 254]],
            [[4096, 4096], [0, 4096], [0, 0], [4096, 0]],
            [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        ),
        (
            3,
            4,
            [[2, 0, 0], [2, 0, 256], [2, 256, 256], [2, 256, 0]],
            [[4096, 4096], [0, 4096], [0, 0], [4096, 0]],
            [[0, 4096], [0, 0], [4096, 0], [4096, 4096]],
        ),
    ];
    for ((outward_face, inward_face, positions, outward_uvs, inward_uvs), side) in
        PLANES.into_iter().zip(state.sides)
    {
        let side = if isolated_upper {
            PaleMossCarpetSide::Tall
        } else {
            side
        };
        let material = match side {
            PaleMossCarpetSide::None => continue,
            // The pinned Bedrock pair is [side_base, side_tip], which is
            // pixel-identical to Java [tall, small] in the opposite naming.
            PaleMossCarpetSide::Short => side_materials[1],
            PaleMossCarpetSide::Tall => side_materials[0],
        };
        quads.push(ModelQuad {
            positions,
            uvs: outward_uvs,
            material,
            // Neither face advertises a boundary cull direction: support
            // connectivity must not remove it before alpha testing.
            flags: outward_face,
        });
        quads.push(ModelQuad {
            positions: [positions[0], positions[3], positions[2], positions[1]],
            uvs: inward_uvs,
            material,
            flags: inward_face,
        });
    }
    quads
}
