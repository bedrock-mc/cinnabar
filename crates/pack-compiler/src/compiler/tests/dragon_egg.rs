use super::*;
use assets::{
    CompiledBiomeAssets, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT, NetworkIdMode, RuntimeAssets,
};

#[test]
fn current_dragon_egg_keeps_all_eight_steps_through_the_world_carrier() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let target: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/bedrock-target.json"))
            .unwrap();
    let protocol = target["wire_protocol"].as_u64().unwrap() as u32;
    let registry_bytes =
        fs::read(root.join(target["artifacts"]["block_registry"].as_str().unwrap())).unwrap();
    let registry = assets::read_registry_for_protocol(&registry_bytes, protocol).unwrap();
    let lights = assets::read_light_registry_for_protocol(
        &fs::read(root.join(target["artifacts"]["light_registry"].as_str().unwrap())).unwrap(),
        &registry_bytes,
        registry.len(),
        protocol,
    )
    .unwrap();
    let records = registry
        .into_vec()
        .into_iter()
        .filter(|record| {
            matches!(
                record.name.as_ref(),
                "minecraft:air" | "minecraft:dragon_egg"
            )
        })
        .collect::<Vec<_>>();
    let egg = records
        .iter()
        .find(|record| record.name.as_ref() == "minecraft:dragon_egg")
        .unwrap();
    let span = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let directory = tempfile::tempdir().unwrap();
    write(
        directory.path().join("blocks.json"),
        r#"{"dragon_egg":{"textures":"dragon_egg"}}"#,
    );
    write(
        directory.path().join("textures/terrain_texture.json"),
        r#"{"texture_data":{"dragon_egg":{"textures":"textures/blocks/dragon_egg"}}}"#,
    );
    write(
        directory.path().join("textures/flipbook_textures.json"),
        "[]",
    );
    write_png(
        directory.path().join("textures/blocks/dragon_egg.png"),
        TILE_SIZE,
        TILE_SIZE,
        &[40, 20, 60, 255].repeat((TILE_SIZE * TILE_SIZE) as usize),
    );
    let (compiled, _) = super::super::compile_pack_inner(
        directory.path(),
        &records,
        &lights[..span],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let visual = runtime.resolve(NetworkIdMode::Sequential, egg.sequential_id);
    assert_eq!(visual.kind(), VisualKind::Model);
    let mut template = visual.model_template().unwrap() as usize;
    assert_eq!(
        runtime
            .resolve(NetworkIdMode::Hashed, egg.network_hash)
            .model_template(),
        visual.model_template()
    );
    let mut quads = Vec::new();
    loop {
        let part = runtime.model_templates()[template];
        quads.extend_from_slice(
            &runtime.model_quads()[part.quad_start as usize..][..part.quad_count as usize],
        );
        if part.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT == 0 {
            break;
        }
        template += 1;
    }
    assert_eq!(
        quads.len(),
        48,
        "the egg needs eight distinct stepped cuboids"
    );
    assert_eq!(visual.support(), VisualSupport::Exact);
    let bounds = [
        ([96, 240, 96], [160, 256, 160]),
        ([80, 224, 80], [176, 240, 176]),
        ([80, 208, 80], [176, 224, 176]),
        ([48, 176, 48], [208, 208, 208]),
        ([32, 128, 32], [224, 176, 224]),
        ([16, 48, 16], [240, 128, 240]),
        ([32, 16, 32], [224, 48, 224]),
        ([48, 0, 48], [208, 16, 208]),
    ];
    for (faces, (min, max)) in quads.as_chunks::<6>().0.iter().zip(bounds) {
        for axis in 0..3 {
            let positions = faces
                .iter()
                .flat_map(|face| face.positions.map(|point| point[axis]))
                .collect::<Vec<_>>();
            assert_eq!(positions.iter().min().copied(), Some(min[axis]));
            assert_eq!(positions.iter().max().copied(), Some(max[axis]));
        }
        assert!(
            faces
                .iter()
                .all(|face| face.material != assets::DIAGNOSTIC_MATERIAL)
        );
    }
}
