use super::*;
use assets::facts::BlockProperty as P;
use assets::{
    BlockFace, CompiledBiomeAssets, LightProperties, MODEL_POSITION_SCALE, MODEL_UV_SCALE,
    ModelQuad,
};

/// Compiles the registered command states with original sprites identifying four material roles.
fn compile_commands(animated: bool) -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap();
    let records: Vec<_> = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                || record
                    .properties
                    .iter()
                    .any(|(key, _)| *key == P::ConditionalBit)
        })
        .collect();
    let mut blocks = serde_json::Map::new();
    for record in &records {
        if record.name.as_ref() != "minecraft:air" {
            blocks.insert(record.name.strip_prefix("minecraft:").unwrap().into(), serde_json::json!({
                "textures": {"north":"front", "south":"back", "east":"side", "west":"side", "up":"conditional", "down":"conditional"}
            }));
        }
    }
    assert!(!blocks.is_empty());
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    let mut terrain = serde_json::Map::new();
    let mut flipbooks = Vec::new();
    for (index, role) in ["front", "back", "side", "conditional"]
        .into_iter()
        .enumerate()
    {
        terrain.insert(
            role.into(),
            serde_json::json!({"textures":format!("textures/{role}")}),
        );
        let pixels: Vec<_> = (0..TILE_SIZE * TILE_SIZE)
            .flat_map(|pixel| {
                [
                    (index + 1) as u8,
                    (pixel % TILE_SIZE) as u8 * 12,
                    (pixel / TILE_SIZE) as u8 * 12,
                    255,
                ]
            })
            .collect();
        write_png(
            directory.path().join(format!("textures/{role}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
        if animated {
            let mut strip = pixels.clone();
            let mut next = pixels;
            for pixel in next.as_chunks_mut::<4>().0.iter_mut() {
                pixel[0] += 10;
            }
            strip.extend(next);
            write_png(
                directory
                    .path()
                    .join(format!("textures/animated-{role}.png")),
                TILE_SIZE,
                TILE_SIZE * 2,
                &strip,
            );
            flipbooks.push(serde_json::json!({"atlas_tile":role,
                "flipbook_texture":format!("textures/animated-{role}"),
                "ticks_per_frame":2,"blend_frames":false}));
        }
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&serde_json::json!({"texture_data":terrain})).unwrap(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        serde_json::to_vec(&flipbooks).unwrap(),
    );
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// Reads the original sprite's role marker independently of material ordering.
fn material_role(compiled: &assets::CompiledAssets, material: u32) -> u8 {
    let texture = compiled.materials[material as usize].texture;
    let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
    mip.rgba8[(texture.layer() * mip.size * mip.size * 4) as usize]
}

#[test]
fn command_block_faces_follow_facing_and_conditional_state() {
    let (compiled, records) = compile_commands(false);
    for record in records.iter().filter(|record| {
        record
            .properties
            .iter()
            .any(|(key, _)| *key == P::ConditionalBit)
    }) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let facing = state[P::FacingDirection.name()]["value"].as_u64().unwrap();
        let conditional = state[P::ConditionalBit.name()]["value"].as_u64().unwrap() != 0;
        let front = [
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::North,
            BlockFace::South,
            BlockFace::West,
            BlockFace::East,
        ][facing as usize];
        let back = match front {
            BlockFace::Down => BlockFace::Up,
            BlockFace::Up => BlockFace::Down,
            BlockFace::North => BlockFace::South,
            BlockFace::South => BlockFace::North,
            BlockFace::West => BlockFace::East,
            BlockFace::East => BlockFace::West,
        };
        for face in BlockFace::ALL {
            let expected = if face == front {
                1
            } else if face == back {
                2
            } else if conditional {
                4
            } else {
                3
            };
            let visual = compiled.visuals[record.sequential_id as usize];
            assert_eq!(
                material_role(&compiled, visual.faces[face as usize]),
                expected,
                "{} {} {face:?}",
                record.name,
                record.canonical_state
            );
        }
    }
}

/// Returns a physical unit normal, independent of the template's UV convention.
fn normal(face: BlockFace) -> [i64; 3] {
    match face {
        BlockFace::West => [-1, 0, 0],
        BlockFace::East => [1, 0, 0],
        BlockFace::Down => [0, -1, 0],
        BlockFace::Up => [0, 1, 0],
        BlockFace::North => [0, 0, -1],
        BlockFace::South => [0, 0, 1],
    }
}

/// Relates increasing texture coordinates to a physical direction across a whole face.
fn coordinate_trend(quad: &ModelQuad, direction: [i64; 3], coordinate: usize) -> i64 {
    quad.positions
        .iter()
        .zip(quad.uvs)
        .map(|(position, uv)| {
            let projected = position
                .iter()
                .zip(direction)
                .map(|(p, d)| i64::from(*p) * d)
                .sum::<i64>();
            projected * (2 * i64::from(uv[coordinate]) - i64::from(MODEL_UV_SCALE))
        })
        .sum()
}

/// Retrieves a state's complete opaque boundary without relying on a template index.
fn quads<'a>(compiled: &'a assets::CompiledAssets, record: &RegistryRecord) -> &'a [ModelQuad] {
    let visual = compiled.visuals[record.sequential_id as usize];
    assert_eq!(visual.kind, VisualKind::Model);
    let template = compiled.model_templates[visual.model_template as usize];
    let quads = &compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize];
    assert_eq!(quads.len(), BlockFace::ALL.len());
    quads
}

#[test]
fn command_side_arrows_point_in_the_block_facing_direction() {
    let (compiled, records) = compile_commands(false);
    for record in records.iter().filter(|r| {
        r.properties
            .iter()
            .any(|(key, _)| *key == P::ConditionalBit)
    }) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let facing = state[P::FacingDirection.name()]["value"].as_u64().unwrap();
        let forward = match facing {
            0 => [0, -1, 0],
            1 => [0, 1, 0],
            2 => [0, 0, -1],
            3 => [0, 0, 1],
            4 => [-1, 0, 0],
            5 => [1, 0, 0],
            _ => unreachable!(),
        };
        for (face, quad) in BlockFace::ALL.into_iter().zip(quads(&compiled, record)) {
            assert_eq!(
                (quad.flags & assets::MODEL_QUAD_FLAG_CULL_FACE_MASK) >> 4,
                face.model_quad_face_id()
            );
            let role = material_role(&compiled, quad.material);
            if role == 3 || role == 4 {
                assert!(
                    coordinate_trend(quad, forward, 1) < 0,
                    "{} {facing} {face:?}",
                    record.name
                );
            } else if face.is_horizontal() {
                let [nx, _, nz] = normal(face);
                assert!(coordinate_trend(quad, [0, 1, 0], 1) < 0);
                assert!(coordinate_trend(quad, [nz, 0, -nx], 0) > 0);
            }
        }
    }
}

#[test]
fn vertical_command_panels_keep_their_cap_orientation() {
    let (compiled, records) = compile_commands(false);
    for record in records.iter().filter(|r| {
        r.properties
            .iter()
            .any(|(key, _)| *key == P::ConditionalBit)
    }) {
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let facing = state[P::FacingDirection.name()]["value"].as_u64().unwrap();
        if facing > 1 {
            continue;
        }
        let boundary = quads(&compiled, record);
        for (face, expected) in [
            (
                BlockFace::Down,
                if facing == 0 {
                    [0, MODEL_UV_SCALE]
                } else {
                    [0, 0]
                },
            ),
            (
                BlockFace::Up,
                if facing == 0 {
                    [0, 0]
                } else {
                    [0, MODEL_UV_SCALE]
                },
            ),
        ] {
            let position = [
                0,
                if face == BlockFace::Up {
                    MODEL_POSITION_SCALE
                } else {
                    0
                },
                0,
            ];
            let quad = &boundary[face as usize];
            let index = quad.positions.iter().position(|p| *p == position).unwrap();
            assert_eq!(
                quad.uvs[index], expected,
                "{} {facing} {face:?}",
                record.name
            );
        }
    }
    let runtime = assets::RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    assert_eq!(runtime.model_quads(), compiled.model_quads.as_ref());
}

#[test]
fn command_materials_bind_separate_flipbook_strips_by_atlas_role() {
    let (compiled, records) = compile_commands(true);
    for record in records.iter().filter(|r| {
        r.properties
            .iter()
            .any(|(key, _)| *key == P::ConditionalBit)
    }) {
        let visual = compiled.visuals[record.sequential_id as usize];
        for material in visual.faces {
            let source = compiled.materials[material as usize];
            assert_ne!(source.animation, assets::NO_ANIMATION);
            let animation = compiled.animations[source.animation as usize];
            assert_eq!(animation.frame_count, 2);
            assert_eq!(animation.ticks_per_frame, 2);
            let frame = compiled.animation_frames[animation.frame_start as usize + 1];
            let mip = &compiled.texture_pages[frame.page() as usize].texture.mips[0];
            let role = mip.rgba8[(frame.layer() * mip.size * mip.size * 4) as usize];
            assert_eq!(role, material_role(&compiled, material) + 10);
        }
    }
}
