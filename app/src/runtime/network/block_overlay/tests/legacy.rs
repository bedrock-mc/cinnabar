use serde_json::{Value, json};

use super::*;

const COLORS: [(&str, [u8; 4]); 9] = [
    ("west", [10, 0, 0, 255]),
    ("east", [20, 0, 0, 255]),
    ("down", [30, 0, 0, 255]),
    ("up", [40, 0, 0, 255]),
    ("north", [50, 0, 0, 255]),
    ("south", [60, 0, 0, 255]),
    ("side", [70, 0, 0, 255]),
    ("brick", [80, 0, 0, 255]),
    ("other", [90, 0, 0, 255]),
];

fn legacy_view(layers: &[Value]) -> LayeredPackView {
    let archives = layers
        .iter()
        .enumerate()
        .map(|(index, blocks)| {
            let id = format!("00000000-0000-0000-0000-{:012}", index + 1);
            let manifest = json!({"format_version":2,"header":{"uuid":id,"version":[1,0,0]},
            "modules":[{"type":"resources"}]})
            .to_string();
            let terrain = json!({"texture_data":COLORS.into_iter().map(|(key, _)| {
            (key.to_owned(), json!({"textures":format!("textures/blocks/{key}")}))
        }).collect::<serde_json::Map<_, _>>()})
            .to_string();
            let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
            for (path, bytes) in [
                ("manifest.json", manifest.into_bytes()),
                ("blocks.json", blocks.to_string().into_bytes()),
                ("textures/terrain_texture.json", terrain.into_bytes()),
                ("models/blocks/gen.geo.json", GEOMETRY.as_bytes().to_vec()),
            ] {
                writer
                    .start_file(path, zip::write::SimpleFileOptions::default())
                    .unwrap();
                writer.write_all(&bytes).unwrap();
            }
            for (key, color) in COLORS {
                writer
                    .start_file(
                        format!("textures/blocks/{key}.png"),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
                writer.write_all(&png(2, 2, |_, _| color)).unwrap();
            }
            protocol::ResourcePackArchive::unencrypted(
                id.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                writer.finish().unwrap().into_inner(),
            )
        })
        .collect();
    LayeredPackView::new(resource_pack::validate_handoff(
        protocol::ResourcePackHandoff::from_archives(archives),
    ))
}

fn empty_definitions(names: &[&str]) -> CustomBlocks {
    const EMPTY_NBT: &[u8] = &[10, 0, 0];
    CustomBlocks::from_definitions(names.iter().map(|&name| (name, EMPTY_NBT)))
}

fn face_color(overlay: &assets::BlockOverlay, block: usize, face: BlockFace) -> [u8; 4] {
    let material = overlay.materials[overlay.visuals[block].faces[face as usize] as usize];
    let page = overlay.texture.as_ref().unwrap();
    let mip = &page.mips[0];
    let start = material.texture.layer() as usize * (mip.size * mip.size * 4) as usize;
    mip.rgba8[start..start + 4].try_into().unwrap()
}

#[test]
fn legacy_texture_bindings_resolve_identical_empty_component_blocks() {
    let view = legacy_view(&[json!({
        "test:brick":{"textures":"brick"}, "test:other":{"textures":"other"}
    })]);
    let blocks = empty_definitions(&["test:brick", "test:other"]);
    assert_eq!(blocks.blocks.len(), 2);
    assert_eq!(blocks.blocks[0].visual.base, blocks.blocks[1].visual.base);
    for hashed in [false, true] {
        let compiled = compile_block_overlay(&view, &blocks, hashed, None).unwrap();
        assert_eq!(compiled.gaps.missing_textures, 0);
        let session = RuntimeAssets::diagnostic()
            .with_block_overlay(1, &compiled.overlay)
            .unwrap();
        for (index, block) in blocks.blocks.iter().enumerate() {
            let hash = block.hashed_states()[0].hash;
            assert_eq!(compiled.overlay.hashes[index], Some(hash));
            let resolved = session.resolve(NetworkIdMode::Sequential, index as u32 + 1);
            assert_eq!(resolved.kind(), VisualKind::Cube);
            assert!(
                compiled.overlay.visuals[index]
                    .faces
                    .into_iter()
                    .all(|face| face != 0)
            );
            assert_eq!(
                session
                    .resolve(NetworkIdMode::Hashed, hash)
                    .face(BlockFace::Up)
                    .material_id(),
                resolved.face(BlockFace::Up).material_id()
            );
            assert_eq!(compiled.overlay.light_properties[index].filter(), 15);
            let color = if block.name.as_ref() == "test:brick" {
                COLORS[7].1
            } else {
                COLORS[8].1
            };
            assert_eq!(face_color(&compiled.overlay, index, BlockFace::Up), color);
        }
    }
}

#[test]
fn legacy_texture_bindings_keep_scalar_three_and_six_face_layouts() {
    let blocks = empty_definitions(&["test:brick"]);
    for (textures, expected) in [
        (json!("brick"), [COLORS[7].1; 6]),
        (
            json!({"up":"up", "down":"down", "side":"side"}),
            [
                COLORS[6].1,
                COLORS[6].1,
                COLORS[2].1,
                COLORS[3].1,
                COLORS[6].1,
                COLORS[6].1,
            ],
        ),
        (
            json!({"west":"west", "east":"east", "down":"down", "up":"up", "north":"north", "south":"south"}),
            COLORS[..6]
                .iter()
                .map(|(_, color)| *color)
                .collect::<Vec<_>>()
                .try_into()
                .unwrap(),
        ),
    ] {
        let view = legacy_view(&[json!({"test:brick":{"textures":textures}})]);
        let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
        assert_eq!(compiled.gaps.missing_textures, 0);
        for (face, color) in [
            BlockFace::West,
            BlockFace::East,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::North,
            BlockFace::South,
        ]
        .into_iter()
        .zip(expected)
        {
            assert_eq!(face_color(&compiled.overlay, 0, face), color);
        }
    }
}

#[test]
fn legacy_texture_bindings_merge_valid_layers_and_skip_invalid_lists() {
    let blocks = empty_definitions(&["test:brick"]);
    let lower = json!({"test:brick":{"textures":"brick"}});
    for (higher, color) in [
        (json!({"test:brick":{"textures":"other"}}), COLORS[8].1),
        (json!({"test:brick":{"sound":"stone"}}), COLORS[7].1),
        (json!({"test:brick":{"textures":{"up":"up"}}}), COLORS[7].1),
        (
            json!({"test:brick":{"textures":{"up":"up","down":"down","side":"side","extra":"other"}}}),
            COLORS[7].1,
        ),
        (json!({"test:brick":{"textures":false}}), COLORS[7].1),
    ] {
        let view = legacy_view(&[lower.clone(), higher]);
        let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
        assert_eq!(face_color(&compiled.overlay, 0, BlockFace::Up), color);
    }
}

#[test]
fn legacy_texture_bindings_preserve_explicit_component_visuals() {
    let view = legacy_view(&[json!({"test:brick":{"textures":"other"}})]);
    let blocks = CustomBlocks {
        blocks: vec![block(
            "test:brick",
            1,
            CustomBlockVisuals {
                base: CustomVisualComponents {
                    geometry: Some("geometry.gen".into()),
                    materials: materials("brick"),
                    ..Default::default()
                },
                ..Default::default()
            },
        )]
        .into(),
        ..Default::default()
    };
    let compiled = compile_block_overlay(&view, &blocks, false, None).unwrap();
    assert_eq!(compiled.overlay.visuals[0].kind, VisualKind::Model);
    assert_eq!(face_color(&compiled.overlay, 0, BlockFace::Up), COLORS[7].1);
}

#[test]
fn legacy_texture_bindings_admitted_pack_fixture_resolves_floor_blocks() {
    let Some(dir) = std::env::var_os("CINNABAR_PACKCACHE_DIR") else {
        eprintln!(
            "skipping legacy_texture_bindings_admitted_pack_fixture_resolves_floor_blocks: fixture unavailable; requires CINNABAR_PACKCACHE_DIR containing offline cached packs"
        );
        return;
    };
    let names = [
        "hive:cream_brick",
        "hive:stone_herring_bone_bricks",
        "hive:terracotta_bricks",
    ];
    let mut checked = std::collections::HashSet::new();
    for entry in std::fs::read_dir(dir).expect("packcache dir").flatten() {
        let path = entry.path();
        if path.extension().is_none_or(|extension| extension != "zip") {
            continue;
        }
        let Some(view) = super::super::super::local_pack::local_pack_view_at(&path) else {
            continue;
        };
        let entries = view.merged_json_object("blocks.json", None);
        let admitted = names
            .into_iter()
            .filter(|name| {
                entries
                    .get(*name)
                    .is_some_and(|entry| entry["textures"].is_string())
            })
            .collect::<Vec<_>>();
        if admitted.is_empty() {
            continue;
        }
        let blocks = empty_definitions(&admitted);
        for hashed in [false, true] {
            let compiled = compile_block_overlay(&view, &blocks, hashed, None).unwrap();
            for (index, block) in blocks.blocks.iter().enumerate() {
                let visual = compiled.overlay.visuals[index];
                assert_eq!(visual.kind, VisualKind::Cube, "{}", block.name);
                assert!(
                    visual.faces.into_iter().all(|material| material != 0),
                    "{}",
                    block.name
                );
                assert_eq!(
                    compiled.overlay.hashes[index],
                    Some(block.hashed_states()[0].hash)
                );
                checked.insert(block.name.to_string());
            }
        }
    }
    if checked.is_empty() {
        eprintln!(
            "skipping legacy_texture_bindings_admitted_pack_fixture_resolves_floor_blocks: fixture unavailable; no admitted floor bindings in cached packs"
        );
        return;
    }
    assert_eq!(checked.len(), names.len());
}
