use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};

#[test]
fn invisible_chests_retain_pack_face_materials_for_particles() {
    let names = [
        "minecraft:chest",
        "minecraft:trapped_chest",
        "minecraft:ender_chest",
        "minecraft:waxed_exposed_copper_chest",
    ];
    let (records, lights, protocol) =
        super::shelf_cake::active_records(&[&["minecraft:stone"][..], &names[..]].concat());
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    let mut blocks: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|name| {
            (
                name.trim_start_matches("minecraft:").to_owned(),
                serde_json::json!({"textures":{"down":"bottom","up":"top","side":"side"}}),
            )
        })
        .collect();
    blocks.insert("stone".into(), serde_json::json!({"textures":"bottom"}));
    write(
        root.join("blocks.json"),
        serde_json::to_vec(&blocks).unwrap(),
    );
    write(
        root.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"bottom":{"textures":"textures/bottom"},"top":{"textures":"textures/top"},"side":{"textures":"textures/side"}}}"#,
    );
    write(root.join("textures/flipbook_textures.json"), "[]");
    for (name, marker) in [("bottom", 31), ("top", 97), ("side", 151)] {
        write_png(
            root.join(format!("textures/{name}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[marker, 0, 0, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    let span = records
        .iter()
        .map(|r| r.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        root,
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    for name in names {
        let states: Vec<_> = records.iter().filter(|r| r.name.as_ref() == name).collect();
        assert!(!states.is_empty(), "missing fixture block {name}");
        for record in states {
            for (mode, id) in [
                (NetworkIdMode::Sequential, record.sequential_id),
                (NetworkIdMode::Hashed, record.network_hash),
            ] {
                let visual = runtime.resolve(mode, id);
                assert_eq!(visual.kind(), VisualKind::Invisible);
                for (face, marker) in [
                    (BlockFace::Down, 31),
                    (BlockFace::Up, 97),
                    (BlockFace::North, 151),
                ] {
                    let material = runtime.material(visual.face(face).material_id());
                    assert_ne!(
                        material.texture,
                        assets::TextureRef::DIAGNOSTIC,
                        "{name} {face:?}"
                    );
                    let page = &runtime.texture_pages()[material.texture.page() as usize].texture;
                    let mip = &page.mips[0];
                    let start = (material.texture.layer() * mip.size * mip.size * 4) as usize;
                    assert_eq!(
                        &mip.rgba8[start..start + 4],
                        &[marker, 0, 0, 255],
                        "{name} {face:?}"
                    );
                }
            }
        }
    }
}
