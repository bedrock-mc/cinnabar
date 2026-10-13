use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

/// Compiles both campfire blocks with distinct unlit, lit and flame sprites per block.
fn compile_campfires() -> (RuntimeAssets, Vec<RegistryRecord>) {
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
        .filter(|r| {
            matches!(
                r.name.as_ref(),
                "minecraft:air"
                    | "minecraft:stone"
                    | "minecraft:campfire"
                    | "minecraft:soul_campfire"
            )
        })
        .collect();
    let directory = tempfile::tempdir().unwrap();
    let names = ["stone", "log", "lit", "fire", "soul_lit", "soul_fire"];
    for (index, name) in names.into_iter().enumerate() {
        let pixels: Vec<_> = (0..TILE_SIZE * TILE_SIZE)
            .flat_map(|_| [index as u8 * 40, 80, 120, 255])
            .collect();
        write_png(
            directory.path().join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &pixels,
        );
    }
    write(
        directory.path().join("blocks.json"),
        r#"{"stone":{"textures":"stone"},
            "campfire":{"textures":{"down":"log","side":"lit","up":"fire"}},
            "soul_campfire":{"textures":{"down":"log","side":"soul_lit","up":"soul_fire"}}}"#,
    );
    let texture_data: serde_json::Map<_, _> = names
        .into_iter()
        .map(|name| {
            (
                name.to_owned(),
                serde_json::json!({ "textures": format!("textures/{name}") }),
            )
        })
        .collect();
    write(
        directory.path().join("textures/terrain_texture.json"),
        &serde_json::json!({ "texture_data": texture_data }).to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let count = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); count],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        records,
    )
}

/// Expected pixel bounds, lit texture selectors and final UVs in D/U/N/S/W/E order.
type ExpectedBox = ([i16; 3], [i16; 3], [(bool, [[u16; 2]; 4]); 6]);

const EXPECTED_BOXES: [(&str, [ExpectedBox; 5]); 4] = [
    (
        "north",
        [
            // Lower A.
            (
                [1, 0, 0],
                [5, 4, 16],
                [
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // down
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (false, [[0, 0], [16, 0], [16, 4], [0, 4]]), // west
                    (true, [[0, 5], [16, 5], [16, 1], [0, 1]]),  // east
                ],
            ),
            // Upper A.
            (
                [0, 3, 1],
                [16, 7, 5],
                [
                    (true, [[0, 8], [16, 8], [16, 4], [0, 4]]),  // down
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                    (true, [[0, 0], [16, 0], [16, 4], [0, 4]]),  // north
                    (true, [[0, 0], [0, 4], [16, 4], [16, 0]]),  // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Lower B.
            (
                [11, 0, 0],
                [15, 4, 16],
                [
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // down
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (true, [[0, 1], [16, 1], [16, 5], [0, 5]]),  // west
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // east
                ],
            ),
            // Upper B.
            (
                [0, 3, 11],
                [16, 7, 15],
                [
                    (true, [[0, 8], [16, 8], [16, 4], [0, 4]]),  // down
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                    (true, [[16, 0], [0, 0], [0, 4], [16, 4]]),  // north
                    (true, [[16, 0], [16, 4], [0, 4], [0, 0]]),  // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Ember bed.
            (
                [5, 0, 0],
                [11, 1, 16],
                [
                    (false, [[0, 14], [16, 14], [16, 8], [0, 8]]), // down
                    (true, [[16, 8], [0, 8], [0, 14], [16, 14]]),  // up
                    (false, [[6, 15], [0, 15], [0, 16], [6, 16]]), // north
                    (false, [[10, 15], [10, 16], [16, 16], [16, 15]]), // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),     // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),     // east
                ],
            ),
        ],
    ),
    (
        "west",
        [
            // Lower A.
            (
                [0, 0, 1],
                [16, 4, 5],
                [
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // down
                    (false, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                    (false, [[0, 0], [16, 0], [16, 4], [0, 4]]), // north
                    (true, [[0, 1], [0, 5], [16, 5], [16, 1]]),  // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Upper A.
            (
                [1, 3, 0],
                [5, 7, 16],
                [
                    (true, [[16, 4], [0, 4], [0, 8], [16, 8]]),  // down
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (true, [[16, 0], [0, 0], [0, 4], [16, 4]]),  // west
                    (true, [[16, 4], [0, 4], [0, 0], [16, 0]]),  // east
                ],
            ),
            // Lower B.
            (
                [0, 0, 11],
                [16, 4, 15],
                [
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // down
                    (false, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                    (true, [[0, 1], [16, 1], [16, 5], [0, 5]]),  // north
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Upper B.
            (
                [11, 3, 0],
                [15, 7, 16],
                [
                    (true, [[16, 4], [0, 4], [0, 8], [16, 8]]),  // down
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (true, [[0, 0], [16, 0], [16, 4], [0, 4]]),  // west
                    (true, [[0, 4], [16, 4], [16, 0], [0, 0]]),  // east
                ],
            ),
            // Ember bed.
            (
                [0, 0, 5],
                [16, 1, 11],
                [
                    (false, [[0, 8], [0, 14], [16, 14], [16, 8]]), // down
                    (true, [[16, 14], [16, 8], [0, 8], [0, 14]]),  // up
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),     // north
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),     // south
                    (false, [[16, 15], [10, 15], [10, 16], [16, 16]]), // west
                    (false, [[0, 16], [6, 16], [6, 15], [0, 15]]), // east
                ],
            ),
        ],
    ),
    (
        "south",
        [
            // Lower A.
            (
                [11, 0, 0],
                [15, 4, 16],
                [
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // down
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (true, [[16, 1], [0, 1], [0, 5], [16, 5]]),  // west
                    (false, [[16, 4], [0, 4], [0, 0], [16, 0]]), // east
                ],
            ),
            // Upper A.
            (
                [0, 3, 11],
                [16, 7, 15],
                [
                    (true, [[0, 8], [16, 8], [16, 4], [0, 4]]),  // down
                    (false, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                    (true, [[16, 0], [0, 0], [0, 4], [16, 4]]),  // north
                    (true, [[16, 0], [16, 4], [0, 4], [0, 0]]),  // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Lower B.
            (
                [1, 0, 0],
                [5, 4, 16],
                [
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // down
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // west
                    (true, [[16, 5], [0, 5], [0, 1], [16, 1]]),  // east
                ],
            ),
            // Upper B.
            (
                [0, 3, 1],
                [16, 7, 5],
                [
                    (true, [[0, 8], [16, 8], [16, 4], [0, 4]]),  // down
                    (false, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                    (true, [[0, 0], [16, 0], [16, 4], [0, 4]]),  // north
                    (true, [[0, 0], [0, 4], [16, 4], [16, 0]]),  // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Ember bed.
            (
                [5, 0, 0],
                [11, 1, 16],
                [
                    (false, [[16, 8], [0, 8], [0, 14], [16, 14]]), // down
                    (true, [[0, 14], [16, 14], [16, 8], [0, 8]]),  // up
                    (false, [[16, 15], [10, 15], [10, 16], [16, 16]]), // north
                    (false, [[0, 15], [0, 16], [6, 16], [6, 15]]), // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),     // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),     // east
                ],
            ),
        ],
    ),
    (
        "east",
        [
            // Lower A.
            (
                [0, 0, 11],
                [16, 4, 15],
                [
                    (false, [[16, 4], [16, 0], [0, 0], [0, 4]]), // down
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                    (true, [[16, 1], [0, 1], [0, 5], [16, 5]]),  // north
                    (false, [[16, 0], [16, 4], [0, 4], [0, 0]]), // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Upper A.
            (
                [11, 3, 0],
                [15, 7, 16],
                [
                    (true, [[16, 4], [0, 4], [0, 8], [16, 8]]),  // down
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (true, [[0, 0], [16, 0], [16, 4], [0, 4]]),  // west
                    (true, [[0, 4], [16, 4], [16, 0], [0, 0]]),  // east
                ],
            ),
            // Lower B.
            (
                [0, 0, 1],
                [16, 4, 5],
                [
                    (false, [[16, 4], [16, 0], [0, 0], [0, 4]]), // down
                    (false, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                    (false, [[16, 0], [0, 0], [0, 4], [16, 4]]), // north
                    (true, [[16, 1], [16, 5], [0, 5], [0, 1]]),  // south
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
                ],
            ),
            // Upper B.
            (
                [1, 3, 0],
                [5, 7, 16],
                [
                    (true, [[16, 4], [0, 4], [0, 8], [16, 8]]),  // down
                    (false, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                    (false, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                    (false, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                    (true, [[16, 0], [0, 0], [0, 4], [16, 4]]),  // west
                    (true, [[16, 4], [0, 4], [0, 0], [16, 0]]),  // east
                ],
            ),
            // Ember bed.
            (
                [0, 0, 5],
                [16, 1, 11],
                [
                    (false, [[16, 14], [16, 8], [0, 8], [0, 14]]), // down
                    (true, [[0, 8], [0, 14], [16, 14], [16, 8]]),  // up
                    (false, [[4, 8], [4, 4], [0, 4], [0, 8]]),     // north
                    (false, [[0, 8], [4, 8], [4, 4], [0, 4]]),     // south
                    (false, [[6, 15], [0, 15], [0, 16], [6, 16]]), // west
                    (false, [[10, 16], [16, 16], [16, 15], [10, 15]]), // east
                ],
            ),
        ],
    ),
];

/// Returns the minimum and maximum coordinate on one axis.
fn span(quads: &[assets::ModelQuad], axis: usize) -> [i16; 2] {
    let coordinates = quads.iter().flat_map(|q| q.positions.map(|p| p[axis]));
    [
        coordinates.clone().min().unwrap(),
        coordinates.max().unwrap(),
    ]
}

/// Expands the face's ordered corner indices into carrier coordinates.
fn face_positions(min: [i16; 3], max: [i16; 3], face: BlockFace) -> [[i16; 3]; 4] {
    let corners: [usize; 4] = match face {
        BlockFace::Down => [4, 0, 1, 5],
        BlockFace::Up => [7, 3, 2, 6],
        BlockFace::North => [2, 3, 1, 0],
        BlockFace::South => [6, 4, 5, 7],
        BlockFace::West => [6, 2, 0, 4],
        BlockFace::East => [5, 1, 3, 7],
    };
    corners.map(|corner| {
        std::array::from_fn(|axis| {
            if corner & (1 << axis) == 0 {
                min[axis] * 16
            } else {
                max[axis] * 16
            }
        })
    })
}

#[test]
fn campfires_match_every_facing_box_face_and_uv() {
    let (runtime, records) = compile_campfires();
    let face_order = [
        BlockFace::Down,
        BlockFace::Up,
        BlockFace::North,
        BlockFace::South,
        BlockFace::West,
        BlockFace::East,
    ];
    let mut seen = 0;
    let mut flames = Vec::new();
    for record in records.iter().filter(|r| r.name.contains("campfire")) {
        seen += 1;
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let lit = state["extinguished"]["value"] == 0;
        let direction = state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap();
        let expected = &EXPECTED_BOXES
            .iter()
            .find(|(name, _)| *name == direction)
            .unwrap()
            .1;
        let visual = runtime.resolve(NetworkIdMode::Sequential, record.sequential_id);
        assert_eq!(
            visual.kind(),
            VisualKind::Model,
            "{}",
            record.canonical_state
        );
        assert_eq!(visual.support(), VisualSupport::VanillaFallback);
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        let unlit = visual.face(BlockFace::Down).material_id();
        let embers = visual.face(BlockFace::South).material_id();
        let flame = visual.face(BlockFace::Up).material_id();
        let (boxes, planes) = quads.split_at(30);
        for (box_index, (actual, (min, max, faces))) in
            boxes.chunks_exact(6).zip(expected).enumerate()
        {
            for axis in 0..3 {
                assert_eq!(
                    span(actual, axis),
                    [min[axis] * 16, max[axis] * 16],
                    "{direction} box {box_index} axis {axis}"
                );
            }
            for ((quad, (uses_lit, uvs)), face) in actual.iter().zip(faces).zip(face_order) {
                assert_eq!(
                    quad.material,
                    if lit && *uses_lit { embers } else { unlit },
                    "{direction} box {box_index} {face:?}"
                );
                assert_eq!(
                    quad.uvs,
                    uvs.map(|uv| uv.map(|p| p * 256)),
                    "{direction} box {box_index} {face:?}"
                );
                assert_eq!(quad.flags, face.model_quad_face_id());
                assert_eq!(
                    quad.positions,
                    face_positions(*min, *max, face),
                    "{direction} box {box_index} {face:?} vertex order"
                );
            }
        }
        // Upper log bottoms are lit; the ember bed has exactly one lit face.
        for box_index in [1, 3] {
            assert_eq!(
                boxes[box_index * 6].material,
                if lit { embers } else { unlit }
            );
        }
        for (index, quad) in boxes[24..].iter().enumerate() {
            assert_eq!(
                quad.material,
                if lit && index == 1 { embers } else { unlit }
            );
        }
        if lit {
            assert_eq!(boxes.iter().filter(|q| q.material == embers).count(), 9);
            assert_eq!(planes.len(), 2);
            for plane in planes {
                assert_eq!(plane.material, flame);
                assert_ne!(plane.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
            }
            assert_eq!(span(planes, 1), [16, 256]);
            assert_eq!(span(planes, 0), [13, 243]);
            assert_eq!(span(planes, 2), [13, 243]);
            flames.push((record.name.clone(), flame, embers));
        } else {
            assert!(planes.is_empty());
            assert!(boxes.iter().all(|q| q.material == unlit));
        }
    }
    assert_eq!(seen, 16);
    let campfire = flames
        .iter()
        .find(|(name, _, _)| name.as_ref() == "minecraft:campfire")
        .unwrap();
    let soul = flames
        .iter()
        .find(|(name, _, _)| name.as_ref() == "minecraft:soul_campfire")
        .unwrap();
    assert_ne!(campfire.1, soul.1);
    assert_ne!(campfire.2, soul.2);
}
