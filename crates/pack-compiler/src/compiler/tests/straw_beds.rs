use super::*;
use assets::{BlockFace, CompiledBiomeAssets, NetworkIdMode, RuntimeAssets};
use serde_json::json;

/// Compiles original head, foot and fringe art without a behavior-pack block definition.
fn fixture() -> (assets::CompiledAssets, Vec<RegistryRecord>) {
    let protocol = assets::active_content_registry_protocol();
    let records =
        assets::read_registry_for_protocol(assets::pinned_block_registry_bytes(), protocol)
            .unwrap()
            .into_vec()
            .into_iter()
            .filter(|record| {
                matches!(
                    record.name.as_ref(),
                    "minecraft:air" | "minecraft:straw_bed"
                )
            })
            .collect::<Vec<_>>();
    assert_eq!(
        records
            .iter()
            .filter(|record| record.name.as_ref() == "minecraft:straw_bed")
            .count(),
        16
    );
    let root = tempfile::tempdir().unwrap();
    let resource = root.path().join("resource_pack");
    write(
        resource.join("blocks.json"),
        r#"{"straw_bed":{"sound":"straw_bed"}}"#,
    );
    write(resource.join("textures/flipbook_textures.json"), "[]");
    write(
        resource.join("textures/terrain_texture.json"),
        r#"{"texture_data":{"straw_bed":{"textures":"textures/straw"}}}"#,
    );
    write_png(
        resource.join("textures/straw.png"),
        16,
        16,
        &[190, 140, 40, 255].repeat(256),
    );
    for (part, height, fringe_z) in [("head", 6, 8), ("foot", 4, -11)] {
        let faces = json!({
            "up":{"uv":[0,0],"uv_size":[8,8],"material_instance":"all"},
            "down":{"uv":[8,8],"uv_size":[8,-8],"material_instance":"all"},
            "east":{"uv":[0,8],"uv_size":[8,2],"material_instance":"all"}
        });
        let fringe = json!({
            "up":{"uv":[0,0],"uv_size":[3,2],"uv_rotation":180,"material_instance":"all"},
            "down":{"uv":[3,2],"uv_size":[-3,-2],"uv_rotation":180,"material_instance":"all"}
        });
        write(resource.join(format!("models/blocks/{part}.geo.json")), serde_json::to_vec(&json!({
            "minecraft:geometry":[{"description":{"identifier":format!("minecraft:geometry.straw_bed_{part}"),"texture_width":16,"texture_height":16},
            "bones":[{"name":"base","cubes":[{"origin":[-8,0,-8],"size":[16,height,16],"uv":faces}]},
            {"name":"fringe","cubes":[{"origin":[-8,0.125,fringe_z],"size":[16,0,3],"uv":fringe}]}]}]
        })).unwrap());
    }
    let pack = pack_compile_input::pack::read_pack_with_behavior(
        &resource,
        &root.path().join("behavior_pack"),
        &records,
    )
    .unwrap();
    let count = records
        .iter()
        .map(|record| record.sequential_id as usize + 1)
        .max()
        .unwrap();
    let (compiled, _) = super::super::compile_pack_sources(
        &resource,
        pack,
        &records,
        &vec![assets::LightProperties::default(); count],
        CompiledBiomeAssets::diagnostic(),
        protocol,
    )
    .unwrap();
    (compiled, records)
}

#[test]
fn straw_beds_render_both_halves_in_every_direction_without_block_entity_data() {
    let (compiled, records) = fixture();
    let runtime = RuntimeAssets::decode(&assets::encode_blob(&compiled).unwrap()).unwrap();
    let mut variants = std::collections::BTreeSet::new();
    for record in records
        .iter()
        .filter(|record| record.name.as_ref() == "minecraft:straw_bed")
    {
        assert!(
            !record
                .traits
                .contains(assets::registry::BlockTraits::BLOCK_ENTITY_DRAWN)
        );
        assert!(
            assets::facts::BlockEntityFacts::pinned()
                .get(record.sequential_id)
                .is_none()
        );
        let state: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        let head = state["head_piece_bit"]["value"] == 1;
        let turns = match state["minecraft:cardinal_direction"]["value"]
            .as_str()
            .unwrap()
        {
            "south" => 0,
            "east" => 1,
            "north" => 2,
            "west" => 3,
            _ => unreachable!(),
        };
        for (mode, id) in [
            (NetworkIdMode::Sequential, record.sequential_id),
            (NetworkIdMode::Hashed, record.network_hash),
        ] {
            let visual = runtime.resolve(mode, id);
            assert_eq!(
                visual.kind(),
                VisualKind::Model,
                "{}",
                record.canonical_state
            );
            let template_id = visual.model_template().unwrap();
            variants.insert(template_id);
            let template = compiled.model_templates[template_id as usize];
            let quads = &compiled.model_quads[template.quad_start as usize
                ..(template.quad_start + template.quad_count) as usize];
            assert_eq!(
                quads.len(),
                5,
                "both sides of the zero-height fringe must survive"
            );
            let mut bounds = [[i16::MAX; 3], [i16::MIN; 3]];
            for quad in quads {
                assert_eq!(quad.flags & assets::MODEL_QUAD_FLAG_TWO_SIDED, 0);
                for point in quad.positions {
                    for axis in 0..3 {
                        bounds[0][axis] = bounds[0][axis].min(point[axis]);
                        bounds[1][axis] = bounds[1][axis].max(point[axis]);
                    }
                }
            }
            let mut expected = if head {
                [[0, 0, 0], [256, 96, 304]]
            } else {
                [[0, 0, -48], [256, 64, 256]]
            };
            for _ in 0..turns {
                expected = [
                    [expected[0][2], expected[0][1], 256 - expected[1][0]],
                    [expected[1][2], expected[1][1], 256 - expected[0][0]],
                ];
            }
            assert_eq!(bounds, expected, "{}", record.canonical_state);
            assert_eq!(quads[4].uvs, [[768, 512], [768, 0], [0, 0], [0, 512]]);
            let material = runtime.material(visual.face(BlockFace::Up).material_id());
            assert_ne!(material.flags & assets::MATERIAL_FLAG_ALPHA_CUTOUT, 0);
            assert_eq!(material.flags & assets::MATERIAL_FLAG_DISABLE_AO, 0);
            let mip = &compiled.texture_pages[material.texture.page() as usize]
                .texture
                .mips[0];
            let pixel = (material.texture.layer() * mip.size * mip.size * 4) as usize;
            assert_eq!(&mip.rgba8[pixel..pixel + 4], &[190, 140, 40, 255]);
        }
    }
    assert_eq!(
        variants.len(),
        8,
        "occupancy reuses each half and direction"
    );
}
