use std::{fs, path::Path};

use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
use serde_json::{Value, json};

use super::*;

/// Compiles the active registry's oak and bamboo fences against identical textures, so
/// any difference between their templates comes from the bamboo geometry alone.
fn compile_fences() -> (CompiledAssets, Vec<RegistryRecord>) {
    let target: Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let protocol = u32::try_from(target["wire_protocol"].as_u64().unwrap()).unwrap();
    let records: Vec<_> = assets::read_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap(),
        protocol,
    )
    .unwrap()
    .into_iter()
    .filter(|record| {
        matches!(
            record.name.as_ref(),
            "minecraft:air" | "minecraft:oak_fence" | "minecraft:bamboo_fence"
        )
    })
    .collect();
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir_all(directory.path().join("textures/blocks")).unwrap();
    fs::write(
        directory.path().join("blocks.json"),
        serde_json::to_vec(&json!({
            "oak_fence":{"textures":"oak_fence"},
            "bamboo_fence":{"textures":"bamboo_fence"}
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/terrain_texture.json"),
        serde_json::to_vec(&json!({
            "texture_data": {
                "oak_fence":{"textures":"textures/blocks/oak_fence"},
                "bamboo_fence":{"textures":"textures/blocks/bamboo_fence"}
            }
        }))
        .unwrap(),
    )
    .unwrap();
    fs::write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    )
    .unwrap();
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            &[200, 160, 90, 255].repeat(16 * 16),
            16,
            16,
            ExtendedColorType::Rgba8,
        )
        .unwrap();
    for name in ["oak_fence", "bamboo_fence"] {
        fs::write(
            directory.path().join(format!("textures/blocks/{name}.png")),
            &png,
        )
        .unwrap();
    }
    let lights = vec![
        LightProperties::default();
        records
            .iter()
            .map(|record| record.sequential_id as usize + 1)
            .max()
            .unwrap()
    ];
    let (compiled, _) = compile_pack_inner(
        directory.path(),
        &records,
        &lights,
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

/// The quads of the `offset`-th template after a fence's post template.
fn template_quads(
    compiled: &CompiledAssets,
    records: &[RegistryRecord],
    name: &str,
    offset: u32,
) -> Vec<ModelQuad> {
    let record = records
        .iter()
        .find(|record| record.name.as_ref() == name)
        .unwrap_or_else(|| panic!("active registry has {name}"));
    let visual = compiled.visuals[record.sequential_id as usize];
    assert_eq!(visual.kind, VisualKind::Model, "{name}");
    let template = compiled.model_templates[(visual.model_template + offset) as usize];
    compiled.model_quads
        [template.quad_start as usize..(template.quad_start + template.quad_count) as usize]
        .to_vec()
}

/// Every distinct UV a set of quads reads, as `[u, v]` pairs.
fn uv_set(quads: &[ModelQuad]) -> std::collections::BTreeSet<[u16; 2]> {
    quads.iter().flat_map(|quad| quad.uvs).collect()
}

#[test]
fn bamboo_fence_post_reads_only_its_native_regions() {
    let (compiled, records) = compile_fences();
    let post = template_quads(&compiled, &records, "minecraft:bamboo_fence", 0);
    assert_eq!(post.len(), 6);
    for quad in &post {
        let [u0, u1, v0, v1] = if is_horizontal(quad) {
            BAMBOO_POST_TOP
        } else {
            BAMBOO_POST_SIDE
        };
        for [u, v] in quad.uvs {
            assert!([u0, u1].contains(&u) && [v0, v1].contains(&v), "{quad:?}");
        }
    }
    let oak = template_quads(&compiled, &records, "minecraft:oak_fence", 0);
    assert_ne!(
        uv_set(&post),
        uv_set(&oak),
        "bamboo must not share the oak post"
    );
}

#[test]
fn bamboo_fence_slats_reach_past_the_post_centre_and_turn_east_west_tops() {
    let (compiled, records) = compile_fences();
    // Mask 2 is the east slat; mask 1 the north one.
    let east = template_quads(&compiled, &records, "minecraft:bamboo_fence", 1 + 2);
    let north = template_quads(&compiled, &records, "minecraft:bamboo_fence", 1 + 1);
    for quads in [&east, &north] {
        assert_eq!(quads.len(), 8, "two rails of four kept faces");
    }
    let min_x = east
        .iter()
        .flat_map(|quad| quad.positions)
        .map(|position| position[0])
        .min();
    assert_eq!(
        min_x,
        Some(112),
        "nine-pixel slat starts one pixel past the centre"
    );
    let top = |quads: &[ModelQuad]| {
        *quads
            .iter()
            .find(|quad| is_horizontal(quad) && quad.positions[0][1] == 240)
            .expect("upper rail top")
    };
    let [u0, u1, v0, v1] = BAMBOO_SLAT_TOP;
    let corner = |quad: ModelQuad, x: i16, z: i16| {
        let index = quad
            .positions
            .iter()
            .position(|position| position[0] == x && position[2] == z)
            .expect("top corner");
        quad.uvs[index]
    };
    // North-south slats run the region's long v axis along z.
    let north_top = top(&north);
    assert_eq!(corner(north_top, 112, 0), [u0, v0]);
    assert_eq!(corner(north_top, 144, 144), [u1, v1]);
    // East-west slats turn it a quarter: u follows z, v runs back along x.
    let east_top = top(&east);
    assert_eq!(corner(east_top, 256, 112), [u0, v0]);
    assert_eq!(corner(east_top, 112, 144), [u1, v1]);
    for quad in east
        .iter()
        .chain(&north)
        .filter(|quad| !is_horizontal(quad))
    {
        let [u0, u1, v0, v1] = BAMBOO_SLAT_SIDE;
        for [u, v] in quad.uvs {
            assert!([u0, u1].contains(&u) && [v0, v1].contains(&v), "{quad:?}");
        }
    }
}

#[test]
fn ordinary_fence_arms_keep_position_derived_uvs() {
    let (compiled, records) = compile_fences();
    let east = template_quads(&compiled, &records, "minecraft:oak_fence", 1 + 2);
    assert_eq!(east, fence_arm_quads(east[0].material, 2));
    let top = east
        .iter()
        .find(|quad| is_horizontal(quad) && quad.positions[0][1] == 240)
        .unwrap();
    for (position, uv) in top.positions.iter().zip(top.uvs) {
        assert_eq!(uv, [position[0] as u16 * 16, position[2] as u16 * 16]);
    }
}
