use super::*;
use assets::{BlockFace, CompiledBiomeAssets, MATERIAL_FLAG_OVERLAY_MASK};

const FRINGE: [u8; 4] = [150, 150, 150, 255];
const DIRT: [u8; 4] = [121, 85, 58, 0];

/// Vanilla's grass side stores opaque dirt under alpha zero; alpha only weights the biome tint,
/// so distant mips must keep the dirt instead of treating it as transparent.
#[test]
fn grass_side_mips_keep_the_dirt_under_its_tint_mask() {
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
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:grass_block"
            )
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
        r#"{"grass":{"textures":{"side":"grass_side","up":"grass_top","down":"dirt"}}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r##"{"texture_data":{
                "grass_side":{"textures":[{"path":"textures/blocks/grass_side","overlay_color":"#df6827"}]},
                "grass_top":{"textures":"textures/blocks/grass_top"},
                "dirt":{"textures":"textures/blocks/dirt"}
            }}"##,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let side = (0..TILE_SIZE)
        .flat_map(|y| (0..TILE_SIZE).map(move |_| if y < 3 { FRINGE } else { DIRT }))
        .flatten()
        .collect::<Vec<_>>();
    write_png(
        directory.path().join("textures/blocks/grass_side.png"),
        TILE_SIZE,
        TILE_SIZE,
        &side,
    );
    for path in ["grass_top", "dirt"] {
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
        .find(|record| record.name.as_ref() == "minecraft:grass_block")
        .unwrap();
    let visual = compiled.visuals[grass.sequential_id as usize];
    let material = compiled.materials[visual.faces[BlockFace::North as usize] as usize];
    assert_ne!(material.flags & MATERIAL_FLAG_OVERLAY_MASK, 0);
    let page = &compiled.texture_pages[material.texture.page() as usize].texture;
    let expected = assets::build_legacy_terrain_mip_chain(&side, TILE_SIZE).unwrap();
    for (level, mip) in page.mips.iter().enumerate() {
        let layer_bytes = (mip.size * mip.size * 4) as usize;
        let start = material.texture.layer() as usize * layer_bytes;
        let layer = &mip.rgba8[start..start + layer_bytes];
        if mip.size > 1 {
            // Vanilla truncates each averaged byte, so the dirt may drop by one.
            let bottom = &layer[layer_bytes - 4..layer_bytes - 1];
            assert!(
                bottom
                    .iter()
                    .zip(DIRT)
                    .all(|(&got, dirt)| dirt.abs_diff(got) <= 1),
                "mip {level} keeps the dirt colour: {bottom:?}"
            );
        }
        assert_eq!(layer, expected[level].rgba8.as_ref(), "mip {level}");
    }
}
