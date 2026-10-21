use super::*;
use assets::{CompiledBiomeAssets, NetworkIdMode, RuntimeAssets, facts::pack::PackBlock};

/// Compiles the beacon's active identity with independently colored original sprites.
fn fixture(shared_sprite: bool) -> (RuntimeAssets, RegistryRecord) {
    let (records, lights, protocol) = super::shelf_cake::active_records(&[
        PackBlock::Beacon.identifier(),
        PackBlock::Stone.identifier(),
    ]);
    let beacon = records
        .iter()
        .find(|record| record.name.as_ref() == PackBlock::Beacon.identifier())
        .unwrap()
        .clone();
    let mut blocks = serde_json::json!({});
    blocks[PackBlock::Beacon.identifier()] = if shared_sprite {
        serde_json::json!({"textures":"core"})
    } else {
        serde_json::json!({"textures":{"down":"base","up":"core","side":"shell"}})
    };
    for (block, texture) in [
        (PackBlock::Stone, "stone"),
        (PackBlock::Obsidian, "base"),
        (PackBlock::Glass, "shell"),
    ] {
        blocks[block.identifier()] = serde_json::json!({"textures":texture});
    }
    let directory = tempfile::tempdir().unwrap();
    write(directory.path().join("blocks.json"), blocks.to_string());
    let mut terrain = serde_json::json!({"texture_data":{}});
    for (texture, color) in [
        ("stone", [100, 100, 100, 255]),
        ("base", [40, 20, 60, 255]),
        ("core", [80, 220, 230, 255]),
        ("shell", [200, 210, 220, 0]),
    ] {
        let path = format!("textures/fixture/{texture}");
        terrain["texture_data"][texture] = serde_json::json!({"textures":path});
        write_png(
            directory.path().join(format!("{path}.png")),
            TILE_SIZE,
            TILE_SIZE,
            &color.repeat((TILE_SIZE * TILE_SIZE) as usize),
        );
    }
    write(
        directory.path().join("textures/terrain_texture.json"),
        terrain.to_string(),
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (
        RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap(),
        beacon,
    )
}

#[test]
fn beacon_retains_distinct_base_core_and_transparent_shell_through_the_carrier() {
    for shared_sprite in [false, true] {
        let (runtime, record) = fixture(shared_sprite);
        let visual = runtime.resolve(NetworkIdMode::Hashed, record.network_hash);
        assert_eq!(visual.kind(), VisualKind::Model);
        assert_eq!(visual.support(), VisualSupport::VanillaFallback);
        assert_eq!(
            runtime.resolve(NetworkIdMode::Sequential, record.sequential_id),
            visual
        );
        let template = runtime.model_templates()[visual.model_template().unwrap() as usize];
        let quads =
            &runtime.model_quads()[template.quad_start as usize..][..template.quad_count as usize];
        assert_eq!(quads.len(), 18, "each beacon part has its own six faces");
        let mut materials = Vec::new();
        for (part, (min, max, alpha, color)) in quads.as_chunks::<6>().0.iter().zip([
            ([32, 2, 32], [224, 48, 224], false, [40, 20, 60, 255]),
            ([48, 48, 48], [208, 224, 208], true, [80, 220, 230, 255]),
            ([0, 0, 0], [256, 256, 256], true, [200, 210, 220, 0]),
        ]) {
            let material = part[0].material;
            assert_ne!(material, assets::DIAGNOSTIC_MATERIAL);
            assert!(part.iter().all(|quad| quad.material == material));
            assert_eq!(
                runtime.material(material).flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT != 0,
                alpha
            );
            materials.push(material);
            let texture = runtime.material(material).texture;
            let mip = &runtime.texture_pages()[texture.page() as usize]
                .texture
                .mips[0];
            let offset = texture.layer() as usize * (mip.size * mip.size * 4) as usize;
            if color[3] == 0 {
                assert_eq!(mip.rgba8[offset + 3], 0);
            } else {
                assert_eq!(&mip.rgba8[offset..offset + 4], &color);
            }
            for axis in 0..3 {
                let coordinates = part
                    .iter()
                    .flat_map(|quad| quad.positions.map(|position| position[axis]));
                assert_eq!(coordinates.clone().min(), Some(min[axis]));
                assert_eq!(coordinates.max(), Some(max[axis]));
            }
            assert!(part.iter().all(|quad| quad.flags & (0xf << 4) == 0));
        }
        materials.sort_unstable();
        materials.dedup();
        assert_eq!(materials.len(), 3);
        let facts = assets::facts::BlockEntityFacts::pinned();
        let backing = facts.get(record.sequential_id).unwrap();
        assert!(backing.entity.geometry.accepts(visual.kind(), false));
        assert_eq!(
            backing.entity.renderer,
            assets::facts::BlockEntityRenderer::Beacon
        );
    }
}
