use super::*;
use assets::{BlockFace, CompiledBiomeAssets, MATERIAL_FLAG_ISOTROPIC};

#[test]
fn grass_cube_faces_keep_the_pack_authored_isotropic_mask() {
    check_face_mask("minecraft:grass_block", "grass", VisualKind::Cube);
}

#[test]
fn dirt_path_model_faces_keep_the_pack_authored_isotropic_mask() {
    check_face_mask("minecraft:grass_path", "grass_path", VisualKind::Model);
}

/// Compiles authored face rotation through base and variation materials for each geometry route.
fn check_face_mask(name: &str, pack_key: &str, kind: VisualKind) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let records = assets::read_registry_for_protocol(&bytes, protocol)
        .unwrap()
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(record.name.as_ref(), "minecraft:air" | "minecraft:stone")
                || record.name.as_ref() == name
        })
        .collect::<Vec<_>>();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        format!(
            r#"{{"stone":{{"textures":"dirt"}},"{pack_key}":{{"textures":{{"side":"grass_side","up":"grass_top","down":"dirt"}},"isotropic":{{"up":true,"down":true}}}}}}"#
        ),
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        if kind == VisualKind::Cube {
            r#"{"texture_data":{
            "grass_side":{"textures":"textures/blocks/grass_side"},
            "grass_top":{"textures":{"variations":[{"path":"textures/blocks/grass_top","weight":1},{"path":"textures/blocks/dirt","weight":3}]}},
            "dirt":{"textures":"textures/blocks/dirt"}
        }}"#
        } else {
            r#"{"texture_data":{
            "grass_side":{"textures":"textures/blocks/grass_side"},
            "grass_top":{"textures":"textures/blocks/grass_top"},
            "dirt":{"textures":"textures/blocks/dirt"}
        }}"#
        },
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    for path in ["grass_side", "grass_top", "dirt"] {
        write_png(
            directory.path().join(format!("textures/blocks/{path}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &[90, 160, 70, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &vec![assets::LightProperties::default(); span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let grass = records
        .iter()
        .find(|record| record.name.as_ref() == name)
        .unwrap();
    let visual = compiled.visuals[grass.sequential_id as usize];
    assert_eq!(visual.kind, kind);
    for face in BlockFace::ALL {
        let material = compiled.materials[visual.faces[face as usize] as usize];
        let expected = matches!(face, BlockFace::Down | BlockFace::Up);
        assert_eq!(
            material.flags & MATERIAL_FLAG_ISOTROPIC != 0,
            expected,
            "{face:?}"
        );
        for choice in &compiled.materials[material.variation_start as usize..]
            [..material.variation_count as usize]
        {
            assert_eq!(
                choice.flags & MATERIAL_FLAG_ISOTROPIC != 0,
                expected,
                "{face:?} variation"
            );
        }
    }
    if kind == VisualKind::Cube {
        assert!(
            compiled.materials[visual.faces[BlockFace::Up as usize] as usize].variation_count > 1
        );
    }
}
