use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::{Value, json};

use super::*;
use crate::compiler::compile_pack_inner;

const GRASS_SIDE: &str = "textures/blocks/test_grass_side";
const SNOW_PIXEL: [u8; 4] = [241, 244, 249, 255];

fn fixture(native_array: bool) -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join("textures/blocks")).unwrap();
    fs::write(
        root.join("blocks.json"),
        json!({
            "grass": {"textures": {"side": "grass_side", "up": "grass_top", "down": "dirt"}},
            "snow_layer": {"textures": "snow"},
            "snow": {"textures": "snow"},
            "powder_snow": {"textures": "snow"},
            "stone": {"textures": "dirt"}
        })
        .to_string(),
    )
    .unwrap();
    let ordinary = json!({"path": GRASS_SIDE, "overlay_color": "#df6827"});
    let side = if native_array {
        json!([ordinary, SNOWED_GRASS_SIDE_TEXTURE])
    } else {
        ordinary
    };
    fs::write(root.join("textures/terrain_texture.json"), json!({
        "texture_data": {
            "grass_side": {"textures": side},
            "grass_top": {"textures": "textures/blocks/test_top"},
            "dirt": {"textures": "textures/blocks/test_dirt"},
            "snow": {"textures": "textures/blocks/test_snow"},
            "mycelium_side": {"textures": ["textures/blocks/test_dirt", SNOWED_GRASS_SIDE_TEXTURE]}
        }
    }).to_string()).unwrap();
    fs::write(root.join("textures/flipbook_textures.json"), "[]").unwrap();
    for (path, pixel) in [
        (GRASS_SIDE, [60, 120, 70, 255]),
        (SNOWED_GRASS_SIDE_TEXTURE, SNOW_PIXEL),
        ("textures/blocks/test_top", [0, 200, 0, 255]),
        ("textures/blocks/test_dirt", [120, 80, 40, 255]),
        ("textures/blocks/test_snow", [255; 4]),
    ] {
        let mut png = Vec::new();
        PngEncoder::new(&mut png)
            .write_image(&pixel.repeat(16 * 16), 16, 16, ExtendedColorType::Rgba8)
            .unwrap();
        fs::write(root.join(format!("{path}.png")), png).unwrap();
    }
    directory
}

fn current_records() -> (Vec<RegistryRecord>, u32) {
    let target: Value = serde_json::from_slice(include_bytes!(
        "../../../../../../assets/bedrock-target.json"
    ))
    .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    let records = assets::read_registry_for_protocol(&bytes, protocol)
        .unwrap()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air"
                    | "minecraft:grass_block"
                    | "minecraft:snow_layer"
                    | "minecraft:snow"
                    | "minecraft:powder_snow"
                    | "minecraft:stone"
            )
        })
        .collect();
    (records, protocol)
}

#[test]
fn current_registry_snowy_grass_compiles_opaque_untinted_alternate_without_changing_carried_faces()
{
    use assets::{
        BlockFace, CompiledBiomeAssets, MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_OVERLAY_MASK,
        NetworkIdMode, RuntimeAssets, encode_blob,
    };

    let (records, protocol) = current_records();
    let lights = vec![
        assets::LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    for native_array in [false, true] {
        let directory = fixture(native_array);
        let (compiled, _) = compile_pack_inner(
            directory.path(),
            &records,
            &lights,
            CompiledBiomeAssets::diagnostic(),
            protocol,
        )
        .unwrap();
        let grass = records
            .iter()
            .find(|record| record.name.as_ref() == "minecraft:grass_block")
            .unwrap();
        let visual = compiled.visuals[grass.sequential_id as usize];
        assert_eq!(visual.kind, VisualKind::Cube);
        assert_ne!(visual.variant & BLOCK_VISUAL_VARIANT_COVERED_GRASS, 0);
        let snowy_id = visual.variant & assets::BLOCK_VISUAL_VARIANT_MATERIAL_MASK;
        let material = compiled.materials[snowy_id as usize];
        assert_eq!(
            material.flags, 0,
            "snowy dirt fringe is already colored and opaque"
        );
        let texture = material.texture;
        let mip = &compiled.texture_pages[texture.page() as usize].texture.mips[0];
        let offset = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
        assert_eq!(&mip.rgba8[offset..offset + 4], SNOW_PIXEL);
        for face in BlockFace::ALL {
            let normal = compiled.materials[visual.faces[face as usize] as usize];
            assert_ne!(visual.faces[face as usize], snowy_id);
            assert_eq!(
                normal.flags & (MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK),
                match face {
                    BlockFace::Down => 0,
                    BlockFace::Up => MATERIAL_FLAG_GRASS_TINT,
                    _ => MATERIAL_FLAG_GRASS_TINT | MATERIAL_FLAG_OVERLAY_MASK,
                }
            );
        }
        for record in &records {
            let expected = match record.name.as_ref() {
                "minecraft:snow_layer" => assets::BLOCK_VISUAL_VARIANT_TOP_SNOW,
                "minecraft:snow" | "minecraft:powder_snow" => BLOCK_VISUAL_VARIANT_SNOW_COVER,
                "minecraft:grass_block" => visual.variant,
                _ => 0,
            };
            assert_eq!(
                compiled.visuals[record.sequential_id as usize].variant, expected,
                "{} {}",
                record.name, record.canonical_state
            );
        }
        let decoded = RuntimeAssets::decode(&encode_blob(&compiled).unwrap()).unwrap();
        for mode in [NetworkIdMode::Sequential, NetworkIdMode::Hashed] {
            let id = match mode {
                NetworkIdMode::Sequential => grass.sequential_id,
                NetworkIdMode::Hashed => grass.network_hash,
            };
            assert_eq!(decoded.resolve(mode, id).variant(), visual.variant);
            assert_eq!(
                BlockFace::ALL.map(|face| decoded.resolve(mode, id).face(face).material_id()),
                visual.faces
            );
        }
    }
}

#[test]
fn snowy_grass_descriptor_requires_exact_precolored_native_sprite() {
    let directory = fixture(false);
    let path = directory.path().join("textures/terrain_texture.json");
    let mut terrain: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    for invalid in [
        json!(["textures/blocks/test_dirt", GRASS_SIDE]),
        json!(["textures/blocks/test_dirt", {"path": SNOWED_GRASS_SIDE_TEXTURE, "overlay_color": "#ffffff"}]),
        json!([
            "textures/blocks/test_dirt",
            SNOWED_GRASS_SIDE_TEXTURE,
            GRASS_SIDE
        ]),
    ] {
        terrain["texture_data"]["mycelium_side"]["textures"] = invalid;
        fs::write(&path, terrain.to_string()).unwrap();
        assert!(material_descriptor(&crate::read_pack(directory.path()).unwrap()).is_none());
    }
}
